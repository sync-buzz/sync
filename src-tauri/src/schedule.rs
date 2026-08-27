//! What the clock needs to know, kept where the clock can read it.
//!
//! A scheduled handler runs for a project with no window open, and what a
//! project declares lives in that project's own memory — so the clock would
//! have to open every repository on the machine to find out whether anything is
//! scheduled at all. It does not: the declaration is remembered when the
//! project is opened, in this installation's own configuration, and the clock
//! reads it with no repository touched.
//!
//! # What is remembered, and what deliberately is not
//!
//! Only what the clock cannot find out for itself: **which projects, and which
//! extension ids each one declares**. Not the handler, and not the interval.
//!
//! Those are the manifest's, and a manifest is already on this machine: a
//! package is unpacked into the installation's own store and
//! `extensions/refs/<id>.json` says which artefact serves an id right now
//! ([`sync_extensions::store`]). The only thing genuinely inside the repository
//! is the list of declared ids. So remembering the interval would be a copy of
//! data the clock can read for itself, and a copy that goes stale twice over:
//! the artefact pointer is machine-wide, so updating a package leaves every
//! remembered copy naming yesterday's interval until each project is opened
//! again — and a package installed from a folder is one somebody is writing
//! right now, whose manifest changes between one tick and the next.
//!
//! An earlier draft of `docs/background.md` §4.1 said to remember the
//! derivative — path, handler, interval. That was reversed, and this file is
//! the reversal.
//!
//! # Two sections, and only one of them is derived
//!
//! [`Store::declared`] is rewritten whole every time a project is opened. It is
//! a cache: delete the file and the next open rebuilds it.
//!
//! [`Store::state`] is not. It holds the switch a person turned off and when
//! each handler last ran, and the write above never touches it. One file rather
//! than two so that there is no join at tick time and no second place a project
//! can be missing from — but two *sections*, because merging them is exactly
//! where a switch somebody turned off gets quietly turned back on.
//!
//! # Why the whole file is written every time
//!
//! [`crate::project::write_configuration`] writes whole files, and the warning
//! against storing anything that ticks that way is about files that grow. This
//! one is bounded by projects × handlers, and a last-run stamp is written once
//! per interval per handler. A hundred projects is a few kilobytes.
//!
//! # Why the reads and writes are guarded
//!
//! Sync opens any number of windows in one process, and every one of them
//! writes here when its project opens. A read-modify-write with no guard loses
//! one of two simultaneous opens — which for [`Store::declared`] costs a
//! project that does not tick until next time, and for [`Store::state`] costs a
//! switch that turns itself back on. The guard is a lock on the file rather
//! than a copy of it in memory: the file stays the one truth, and nothing has
//! to be kept in step with it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sync_extensions::manifest::Scheduled;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::project::{ProjectError, configuration_file, write_configuration};

/// The file, in this installation's configuration directory.
const FILE: &str = "scheduled-projects.json";

/// How often the clock looks.
///
/// A minute, because a minute is the shortest interval a manifest can express
/// ([`sync_extensions::Scheduled`]) and a clock that looked less often than the
/// shortest thing it can be asked for would be quietly refusing what it
/// accepted. It is not a resolution anybody can observe: nothing here corrects
/// drift or makes up for lateness, so a handler on `1h` runs about hourly and
/// the "about" is the design rather than a shortfall.
const TICK: Duration = Duration::from_secs(60);

/// Serialises this process's read-modify-writes of [`FILE`].
///
/// A unit rather than the store itself: what needs to be exclusive is the
/// read-then-write, not the data, and holding the data here would be a second
/// copy of a file that is already the answer.
#[derive(Default)]
pub struct ScheduleFile(Mutex<()>);

/// What the host decided about one project, as opposed to what the project
/// declares.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    /// Extensions whose clock a person turned off here.
    ///
    /// The exception is recorded, never the rule. Installing an extension that
    /// declares a schedule *was* the consent — the card said so — so a file
    /// listing every extension as switched on would be that second consent
    /// written down, and a project would stop ticking the day something failed
    /// to write a `true` nobody had asked for.
    #[serde(default)]
    pub off: Vec<String>,
    /// When each handler last ran, as milliseconds since the epoch, keyed
    /// `<extension id>/<handler>`.
    ///
    /// This is what makes an interval about wall-clock time rather than about
    /// how long the application has been up. Without it a handler on a one-hour
    /// interval would never run at all for somebody who restarts Sync more
    /// often than that, and nothing would ever say so.
    #[serde(default)]
    pub last_run_ms: BTreeMap<String, u64>,
}

/// Everything the clock reads, in one file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    /// Project path → the extension ids that project declares.
    ///
    /// Derived, and rewritten whole when the project is opened.
    #[serde(default)]
    pub declared: BTreeMap<String, Vec<String>>,
    /// Project path → what the host decided about it.
    ///
    /// Authored. Nothing that rewrites [`Self::declared`] touches this.
    #[serde(default)]
    pub state: BTreeMap<String, ProjectState>,
}

impl Store {
    /// Reads the file, answering with an empty store when there is none.
    ///
    /// An unreadable file is treated as absent, for the reason the conversation
    /// pointers are: the worst it costs is that nothing ticks until the next
    /// open rewrites it, and refusing to start the application over it would be
    /// far more than the thing is worth.
    #[must_use]
    fn read<R: Runtime>(app: &AppHandle<R>) -> Self {
        configuration_file(app, FILE)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), ProjectError> {
        write_configuration(&configuration_file(app, FILE)?, self)
    }

    /// Records what a project declares, leaving everything decided about it
    /// exactly as it was.
    ///
    /// A project that declares nothing is dropped rather than kept as an empty
    /// list: an entry that exists and means "nothing here" is a thing the clock
    /// has to walk past on every tick and a thing a reader has to interpret.
    fn remember(&mut self, project: &str, extensions: Vec<String>) {
        if extensions.is_empty() {
            self.declared.remove(project);
        } else {
            self.declared.insert(project.to_owned(), extensions);
        }
    }

    /// Turns one extension's clock off here, or on again.
    ///
    /// Turning it on removes the exception rather than writing a `true`: the
    /// rule is that a declared schedule runs, so the file holds only what
    /// somebody decided against. A project left with no exceptions and no
    /// stamps is dropped from [`Self::state`] entirely — an empty entry is a
    /// decision that was taken and then untaken, and keeping it would make the
    /// file grow with projects that decided nothing.
    fn switch(&mut self, project: &str, id: &str, on: bool) {
        if on {
            let Some(state) = self.state.get_mut(project) else {
                return;
            };
            state.off.retain(|off| off != id);
            if state.off.is_empty() && state.last_run_ms.is_empty() {
                self.state.remove(project);
            }
        } else {
            let state = self.state.entry(project.to_owned()).or_default();
            if !state.off.iter().any(|off| off == id) {
                state.off.push(id.to_owned());
            }
        }
    }

    /// Drops a project from both sections.
    ///
    /// Both, because to a person there is one project and they asked to be rid
    /// of it — the same reasoning that makes forgetting a project take it out
    /// of the menu *and* the registry. A switch left behind here would come
    /// back into force the day somebody opened the folder again, having been
    /// invisible in between.
    fn forget(&mut self, project: &str) {
        self.declared.remove(project);
        self.state.remove(project);
    }
}

/// Remember what a project declares, so that its handlers can be found with no
/// repository opened.
///
/// Called when a project is opened, with the ids it declares — including the
/// ones that declare no schedule. Filtering to the scheduled ones here would
/// put the manifest's answer into the cache, which is the copy this file exists
/// not to hold.
///
/// # Errors
///
/// Reports whatever writing this installation's configuration refused.
#[tauri::command(async)]
pub fn schedule_remember<R: Runtime>(
    app: AppHandle<R>,
    guard: State<'_, ScheduleFile>,
    project: String,
    extensions: Vec<String>,
) -> Result<(), ProjectError> {
    let _held = guard.0.lock().map_err(|_| poisoned())?;
    let mut store = Store::read(&app);
    store.remember(&project, extensions);
    store.write(&app)
}

/// Which extensions' clocks are switched off in this project.
///
/// The exceptions, not the rule, which is what the page needs: everything a
/// project declares runs unless it is in this list. Answering with the whole
/// state instead would hand the window the last-run stamps as well, and the
/// window has decided it does not show them — data nobody reads is data that
/// starts being kept true for nothing.
///
/// # Errors
///
/// Never in practice: an unreadable file reads as nothing switched off, which
/// is the state a fresh installation is in.
#[tauri::command(async)]
pub fn schedule_switched_off<R: Runtime>(
    app: AppHandle<R>,
    guard: State<'_, ScheduleFile>,
    project: String,
) -> Result<Vec<String>, ProjectError> {
    let _held = guard.0.lock().map_err(|_| poisoned())?;
    Ok(Store::read(&app)
        .state
        .get(&project)
        .map(|state| state.off.clone())
        .unwrap_or_default())
}

/// Stop, or restart, one extension's clock in one project.
///
/// **A switch, not a second consent.** Installing an
/// extension that declares a schedule was the agreement; this is how somebody
/// takes it back for one project without removing the package, and turning it
/// on again writes nothing but the removal of an exception.
///
/// # Errors
///
/// Reports whatever writing this installation's configuration refused. It is
/// reported rather than swallowed because a person is in front of this one: a
/// switch that appears to move and does not is the failure the shell is least
/// allowed to have.
#[tauri::command(async)]
pub fn schedule_switch<R: Runtime>(
    app: AppHandle<R>,
    guard: State<'_, ScheduleFile>,
    project: String,
    id: String,
    on: bool,
) -> Result<(), ProjectError> {
    let _held = guard.0.lock().map_err(|_| poisoned())?;
    let mut store = Store::read(&app);
    store.switch(&project, &id, on);
    store.write(&app)
}

fn poisoned() -> ProjectError {
    ProjectError::new(
        "configuration_failed",
        "the schedule file's lock is poisoned",
    )
}

/// Stop answering for a project at all. Called where a project is forgotten.
pub(crate) fn forget<R: Runtime>(app: &AppHandle<R>, guard: &ScheduleFile, project: &str) {
    let Ok(_held) = guard.0.lock() else { return };
    let mut store = Store::read(app);
    store.forget(project);
    // Deliberately quiet. Forgetting a project is a gesture that has already
    // taken it out of the menu and the registry, and a failure to tidy this
    // file is not a reason to tell somebody their project was not forgotten.
    let _ = store.write(app);
}

// ---------------------------------------------------------------------------
// The clock.
// ---------------------------------------------------------------------------

/// Start the clock, for as long as this application runs.
///
/// **A thread of its own rather than a task.** A tick reads files, evaluates
/// JavaScript in an isolate and may reach the engine — every part of it blocks,
/// and blocking work on the async runtime is a mistake this repository has
/// already made: dropping a nested runtime inside an async context panicked tokio on
/// every project open, and the window stayed up while the work quietly died. A
/// thread that sleeps a minute at a time costs a stack and cannot starve
/// anything.
///
/// Nothing stops it. It ends with the process, which is the same answer the
/// engine gets, and for the same reason: there is no state in
/// which Sync is running and its clock deliberately is not.
///
/// It runs in a debug build too, unlike [`crate::updates`]. An update installed
/// over `tauri dev` would replace a bundle that is not one; a handler running on
/// a clock is exactly what somebody working on this needs to be able to see.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            // Sleep first. A launch is already opening windows, starting the
            // engine and unpacking what the build ships with, and a tick has
            // waited an interval by definition — one more minute of it is
            // nothing to anybody, and the alternative is a handler competing
            // with the first frame.
            std::thread::sleep(TICK);
            tick(&app);
        }
    });
}

/// Now, as the stamps are written.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Which of a package's scheduled handlers are due for this project.
///
/// Pure, and the whole of what the clock decides — everything around it is
/// resolving files and reporting. A `None` state is a project nobody has
/// switched anything off in and nothing has run for, which is the ordinary case
/// on a first tick.
fn due<'a>(
    state: Option<&ProjectState>,
    id: &str,
    schedule: &'a [Scheduled],
    now: u64,
) -> Vec<&'a Scheduled> {
    // The switch, and it is read before anything else is worked out: a person
    // who turned this off is owed no further arithmetic about it.
    if state.is_some_and(|state| state.off.iter().any(|off| off == id)) {
        return Vec::new();
    }
    schedule
        .iter()
        .filter(|scheduled| {
            // A manifest whose `every` does not parse is refused at parse, so
            // this is a package that changed under us rather than a state to
            // report. Skipping it is what leaves the rest of the package running.
            let Some(interval) = scheduled.interval() else {
                return false;
            };
            let key = format!("{id}/{}", scheduled.handler);
            match state.and_then(|state| state.last_run_ms.get(&key)) {
                // Never run is overdue, not new. It is the same case as a
                // machine that was asleep: the interval says how often, and
                // nothing has happened for longer than that. It is also what
                // makes an extension somebody has just installed prove itself
                // within the minute rather than at this time tomorrow.
                None => true,
                // A stamp in the future is a clock that moved backwards — an
                // NTP correction, a machine carried across the world, a
                // battery. Waiting for wall-clock time to catch up would stop
                // a handler for as long as the jump, silently.
                Some(&last) if last > now => true,
                Some(&last) => {
                    now.saturating_sub(last)
                        >= u64::try_from(interval.as_millis()).unwrap_or(u64::MAX)
                }
            }
        })
        .collect()
}

/// One pass over every project that ticks.
///
/// **One handler at a time, for the whole machine.** That is the ceiling on
/// concurrent calls `docs/background.md` §5 asks for, and it is the shape rather
/// than a number: there is one loop, so a handler cannot be re-entered while it
/// is running and two packages cannot compete for the engine at three in the
/// morning. Each call is capped in wall-clock time by `handlers.rs`, so a pass
/// cannot run away however many are due.
fn tick<R: Runtime>(app: &AppHandle<R>) {
    let Some(guard) = app.try_state::<ScheduleFile>() else {
        return;
    };
    let store = {
        // Read under the lock. `write_configuration` truncates in place, so a
        // read racing a window's open can see half a file — which parses as
        // nothing and would cost a tick.
        let Ok(_held) = guard.0.lock() else { return };
        Store::read(app)
    };
    if store.declared.is_empty() {
        return;
    }
    let Ok(packages) = crate::extensions::store(app) else {
        return;
    };

    let now = now_ms();
    for (project, ids) in &store.declared {
        // A path that is not there is a folder somebody moved, deleted, or
        // keeps on a volume that is not mounted. It is left in the file: the
        // third of those comes back, and forgetting a project is a gesture a
        // person makes.
        if !std::path::Path::new(project).exists() {
            continue;
        }
        for id in ids {
            // Not installed here any more, or a store that cannot be read.
            // Neither is news: the window says what a project declares and this
            // machine cannot satisfy, and it says it where somebody is looking.
            let Ok(Some(installed)) = packages.resolve(id) else {
                continue;
            };
            for scheduled in due(
                store.state.get(project),
                id,
                &installed.manifest.schedule,
                now,
            ) {
                // Stamped before it runs, so what is recorded is the attempt.
                // A handler that failed failed (`docs/background.md` §9) and
                // waits its interval like any other; stamping the success
                // instead would make the clock retry a broken handler every
                // minute, which is a retry policy arrived at by accident.
                stamp(
                    app,
                    &guard,
                    project,
                    &format!("{id}/{}", scheduled.handler),
                    now,
                );
                let payload = serde_json::json!({
                    "project": { "path": project },
                    "version": installed.manifest.version,
                    "every": scheduled.every,
                });
                if let Err(error) =
                    crate::handlers::run(app, &installed, project, &scheduled.handler, &payload)
                {
                    // The only place a scheduled failure goes. Nobody is in
                    // front of it by definition, so this process's error stream
                    // is where a developer looks and where a packaged build's
                    // console shows it.
                    eprintln!("the clock: {error}");
                }
            }
        }
    }
}

/// Record that a handler was called just now.
fn stamp<R: Runtime>(
    app: &AppHandle<R>,
    guard: &ScheduleFile,
    project: &str,
    handler: &str,
    at: u64,
) {
    let Ok(_held) = guard.0.lock() else { return };
    let mut store = Store::read(app);
    store
        .state
        .entry(project.to_owned())
        .or_default()
        .last_run_ms
        .insert(handler.to_owned(), at);
    // Quiet, and the cost of failing is one handler running again next tick
    // rather than at its interval. Telling somebody would mean telling them
    // about a file they have never heard of, about a handler they did not ask
    // for, at an hour they did not choose.
    let _ = store.write(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(off: &[&str], last: &[(&str, u64)]) -> ProjectState {
        ProjectState {
            off: off.iter().map(|id| (*id).to_owned()).collect(),
            last_run_ms: last
                .iter()
                .map(|(handler, at)| ((*handler).to_owned(), *at))
                .collect(),
        }
    }

    /// The whole point of two sections. A project is opened, its declaration is
    /// rewritten from what the manifest says now, and the switch somebody
    /// turned off last week is still off.
    #[test]
    fn remembering_a_declaration_leaves_what_was_decided_alone() {
        let mut store = Store::default();
        store
            .state
            .insert("/a".to_owned(), state(&["issues"], &[("issues/poll", 7)]));
        store.remember("/a", vec!["issues".to_owned(), "chat".to_owned()]);

        assert_eq!(
            store.declared["/a"],
            vec!["issues".to_owned(), "chat".to_owned()]
        );
        assert_eq!(
            store.state["/a"],
            state(&["issues"], &[("issues/poll", 7)]),
            "a switch a person turned off is not the open flow's to turn back on"
        );
    }

    #[test]
    fn a_declaration_is_replaced_rather_than_added_to() {
        let mut store = Store::default();
        store.remember("/a", vec!["issues".to_owned(), "chat".to_owned()]);
        store.remember("/a", vec!["chat".to_owned()]);
        assert_eq!(
            store.declared["/a"],
            vec!["chat".to_owned()],
            "an extension removed from a project stops being one the clock looks at"
        );
    }

    #[test]
    fn a_project_that_declares_nothing_is_not_an_entry() {
        let mut store = Store::default();
        store.remember("/a", vec!["issues".to_owned()]);
        store.remember("/a", Vec::new());
        assert!(
            !store.declared.contains_key("/a"),
            "an entry meaning nothing is one every tick walks past"
        );
    }

    #[test]
    fn forgetting_a_project_takes_both_halves() {
        let mut store = Store::default();
        store.remember("/a", vec!["issues".to_owned()]);
        store.state.insert("/a".to_owned(), state(&["issues"], &[]));
        store.remember("/b", vec!["chat".to_owned()]);

        store.forget("/a");

        assert!(!store.declared.contains_key("/a"));
        assert!(
            !store.state.contains_key("/a"),
            "a switch left behind comes back into force the day the folder is opened again"
        );
        assert!(store.declared.contains_key("/b"), "and nobody else's");
    }

    /// A file written by a build that had one section and read by one that has
    /// two, and the other way round. Both are ordinary during this work.
    #[test]
    fn a_file_missing_a_section_reads_as_an_empty_one() {
        let store: Store =
            serde_json::from_str(r#"{"declared":{"/a":["issues"]}}"#).expect("reads");
        assert_eq!(store.declared["/a"], vec!["issues".to_owned()]);
        assert!(store.state.is_empty());

        let store: Store = serde_json::from_str("{}").expect("reads");
        assert!(store.declared.is_empty() && store.state.is_empty());
    }

    fn every(handler: &str, every: &str) -> Scheduled {
        Scheduled {
            handler: handler.to_owned(),
            description: "Looks for new issues".to_owned(),
            every: every.to_owned(),
        }
    }

    /// One hour, in milliseconds, as the stamps count.
    const HOUR: u64 = 60 * 60 * 1000;

    /// The case every first tick is in, and the reason it is `true` rather than
    /// `false`: an extension installed a minute ago proves itself within the
    /// minute instead of at this time tomorrow.
    #[test]
    fn a_handler_that_has_never_run_is_overdue() {
        let schedule = vec![every("issues.poll", "1h")];
        assert_eq!(due(None, "issues", &schedule, 5 * HOUR).len(), 1);
        assert_eq!(
            due(
                Some(&ProjectState::default()),
                "issues",
                &schedule,
                5 * HOUR
            )
            .len(),
            1,
            "a project somebody has switched something else off in is no different"
        );
    }

    #[test]
    fn a_handler_waits_its_interval_and_then_does_not() {
        let schedule = vec![every("issues.poll", "1h")];
        let ran = state(&[], &[("issues/issues.poll", 5 * HOUR)]);

        assert!(
            due(Some(&ran), "issues", &schedule, 5 * HOUR + HOUR - 1).is_empty(),
            "a minute short of an hour is not an hour"
        );
        assert_eq!(
            due(Some(&ran), "issues", &schedule, 6 * HOUR).len(),
            1,
            "and an hour is"
        );
    }

    /// Lateness is not made up for: a machine asleep for six hours runs the
    /// handler once when it wakes, not six times. That is one pass of the
    /// clock answering `1`, not `6`.
    #[test]
    fn six_hours_asleep_is_one_run_and_not_six() {
        let schedule = vec![every("issues.poll", "1h")];
        let ran = state(&[], &[("issues/issues.poll", 0)]);
        assert_eq!(due(Some(&ran), "issues", &schedule, 6 * HOUR).len(), 1);
    }

    /// A clock corrected backwards — NTP, a flat battery, a machine carried
    /// across the world — leaves a stamp in the future. Waiting for wall-clock
    /// time to catch up would stop a handler for as long as the jump, and say
    /// nothing about it.
    #[test]
    fn a_stamp_from_the_future_does_not_stop_the_clock() {
        let schedule = vec![every("issues.poll", "1h")];
        let ran = state(&[], &[("issues/issues.poll", 10 * HOUR)]);
        assert_eq!(due(Some(&ran), "issues", &schedule, 2 * HOUR).len(), 1);
    }

    #[test]
    fn a_switch_turned_off_stops_everything_that_package_asked_for() {
        let schedule = vec![every("issues.poll", "1h"), every("issues.file", "1d")];
        let off = state(&["issues"], &[]);
        assert!(due(Some(&off), "issues", &schedule, 9 * HOUR).is_empty());
        assert_eq!(
            due(Some(&off), "chat", &schedule, 9 * HOUR).len(),
            2,
            "and nobody else's"
        );
    }

    /// Two handlers of one package are two questions. The one whose hour has
    /// come runs; the one on a day does not, and neither delays the other.
    #[test]
    fn handlers_of_one_package_are_due_separately() {
        let schedule = vec![every("issues.poll", "1h"), every("issues.file", "1d")];
        let ran = state(
            &[],
            &[("issues/issues.poll", 0), ("issues/issues.file", 20 * HOUR)],
        );
        let now = 22 * HOUR;
        assert_eq!(
            due(Some(&ran), "issues", &schedule, now)
                .iter()
                .map(|scheduled| scheduled.handler.as_str())
                .collect::<Vec<_>>(),
            vec!["issues.poll"]
        );
    }

    /// A manifest whose `every` does not parse is refused at parse, so this is
    /// a package that changed under the clock. The rest of it goes on running.
    #[test]
    fn an_interval_that_does_not_parse_is_skipped_and_not_fatal() {
        let schedule = vec![
            every("issues.poll", "sometimes"),
            every("issues.file", "1h"),
        ];
        assert_eq!(
            due(None, "issues", &schedule, HOUR)
                .iter()
                .map(|scheduled| scheduled.handler.as_str())
                .collect::<Vec<_>>(),
            vec!["issues.file"]
        );
    }

    #[test]
    fn turning_a_clock_off_and_on_again_leaves_no_trace() {
        let mut store = Store::default();
        store.switch("/a", "issues", false);
        assert_eq!(store.state["/a"].off, vec!["issues".to_owned()]);

        store.switch("/a", "issues", false);
        assert_eq!(
            store.state["/a"].off,
            vec!["issues".to_owned()],
            "asking twice for the same thing is one exception, not two"
        );

        store.switch("/a", "issues", true);
        assert!(
            !store.state.contains_key("/a"),
            "a decision taken and untaken leaves the file as it was"
        );
    }

    /// The stamps are the clock's own arithmetic and are nobody's decision, so
    /// switching something on must not take them with it.
    #[test]
    fn turning_a_clock_on_keeps_what_the_clock_recorded() {
        let mut store = Store::default();
        store.state.insert(
            "/a".to_owned(),
            state(&["issues", "chat"], &[("issues/issues.poll", 7)]),
        );
        store.switch("/a", "issues", true);

        assert_eq!(store.state["/a"].off, vec!["chat".to_owned()]);
        assert_eq!(store.state["/a"].last_run_ms["issues/issues.poll"], 7);
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let mut store = Store::default();
        store.remember("/a", vec!["issues".to_owned()]);
        store
            .state
            .insert("/a".to_owned(), state(&["chat"], &[("issues/poll", 42)]));

        let text = serde_json::to_string(&store).expect("writes");
        assert_eq!(serde_json::from_str::<Store>(&text).expect("reads"), store);
    }
}
