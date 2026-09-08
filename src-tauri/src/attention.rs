//! What this application says when nobody is looking at it.
//!
//! Sync serves agents with every window closed (`crate::tray`), so the moment
//! an agent stops and waits for a person is the moment there is most likely
//! nothing on screen to say so. A banner is the only thing this application can
//! put in front of somebody who is in another one, and it is spent on exactly
//! that: an agent that cannot go on without an answer, an agent that has
//! finished, and an agent that fell over.
//!
//! # Why it is raised here and not in the window
//!
//! A notification asked for by the webview would exist only for as long as a
//! webview does, which is precisely when nobody needs one. So it is raised from
//! the session's own event, in the process that outlives every window, and the
//! absence of a window is one of the conditions rather than the thing that
//! stops it. None of the plugin's commands are in a capability for the same
//! reason the updater's are not.
//!
//! # Three events, and nothing else
//!
//! [`Status::Asking`], [`Status::Ready`] and [`Status::Failed`]. Each one is a
//! turn that has stopped moving without a person: an agent waiting on an
//! answer, one that has none left to give, and one that will not give any more.
//! Every other status is the agent working, which is the state a person left it
//! in on purpose.
//!
//! [`Status::Ended`] is deliberately not among them. A session ends because
//! somebody ended it, and telling a person what they have just done is the
//! definition of a banner nobody asked for.
//!
//! # What a banner does not carry
//!
//! The reason a session failed. A banner is *come and look*, and the window is
//! where a failure is explained — with the stderr under it, and the
//! conversation it happened in. A reason cut to fit a notification is a reason
//! somebody has to come and read properly anyway, after it has already
//! frightened them in a font they cannot copy from.
//!
//! # It says where it goes, because it goes there
//!
//! A banner is clicked by somebody deciding whether to leave what they are
//! doing, so the three lines are three answers to *where would I land*, coarse
//! to fine: the project, the conversation in it, and the agent with what has
//! happened to it. The system draws the application's own name above all three,
//! so the largest line this writes is the largest thing it has to say — which
//! is the project, because that is the window a click opens.
//!
//! It is also an [`Address`], carried out with the notification and handed back
//! when somebody clicks: `banner.rs` is what carries it, and [`opened`] is what
//! it means. A banner that only *said* where it went would be the worse half of
//! this — the same sentence, and still nowhere to go from.

#[cfg(target_os = "macos")]
mod banner;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, Manager as _, Runtime};
use tauri_plugin_notification::NotificationExt as _;

use crate::project::{configuration_file, write_configuration};
use crate::sessions::event::{SessionEvent, Status};
use crate::sessions::live::{Announce, Session};

/// What this installation's banners are called in its own configuration.
///
/// Beside `voice.json` and for the same reason: what the machine says out loud
/// and what it says on the screen are both decisions about these speakers and
/// this desk, and neither travels with a repository to somebody else's.
const FILE: &str = "notifications.json";

/// Which of the three a person wants to be interrupted for.
///
/// Three switches rather than one, because they are three different
/// interruptions: waiting for an answer is somebody blocked on you, finishing
/// is work you can now read, and falling over is something to fix. A person who
/// wants the first and not the second is not asking for a compromise.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preference {
    /// An agent is waiting for permission and the turn has stopped until it is
    /// answered.
    #[serde(default = "on")]
    pub asking: bool,
    /// A turn ended.
    #[serde(default = "on")]
    pub finished: bool,
    /// A session could not be raised, or it fell over.
    #[serde(default = "on")]
    pub failed: bool,
}

/// A switch nobody has touched is on.
///
/// Per field rather than `#[serde(default)]` on the struct, and that is the
/// whole of the difference: the derived default for a `bool` is `false`, so a
/// file written by a build that did not have one of these switches yet would
/// read as *turned off* — a preference somebody never expressed, in the
/// direction that goes quiet. The gate that keeps banners rare is the window
/// being in front, not a switch somebody has to find first.
fn on() -> bool {
    true
}

impl Default for Preference {
    fn default() -> Self {
        Self {
            asking: on(),
            finished: on(),
            failed: on(),
        }
    }
}

/// The three lines a banner is made of.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Banner {
    /// The project, by the name the window calls it — never the folder it is
    /// in and never the identifier agents type. A person reading a banner is
    /// reading it in the vocabulary they named things in.
    title: String,
    /// What the conversation is called, or nothing where it has no name yet: a
    /// conversation is named by the first thing said in it, and an agent
    /// waiting for permission may not have got that far.
    subtitle: String,
    /// Which agent, and what happened to it.
    body: String,
}

impl Banner {
    /// The title a platform with two lines gets.
    ///
    /// The conversation joins the project rather than being dropped — the two
    /// of them are the answer to *where would I land*, and a line naming only
    /// the project would send somebody to a window to find out which
    /// conversation it was about.
    fn two_line_title(&self) -> String {
        if self.subtitle.is_empty() {
            self.title.clone()
        } else {
            format!("{}: {}", self.title, self.subtitle)
        }
    }
}

/// Where a banner goes when somebody clicks it.
///
/// The window it lands in is the project's, so that is what the address is
/// built around; the conversation is what is opened once there.
///
/// It survives being written into a notification and read back out of one,
/// which is the whole reason it is spelled out here rather than being a
/// borrowed [`Session`]: a banner outlives the conversation it is about, and it
/// outlives this process — a person can click one hours later, after Sync has
/// been restarted, and what comes back is exactly these strings.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// The project's working tree, which is what a window has open.
    pub project: PathBuf,
    /// The conversation, by this application's own name for it.
    pub conversation: String,
    /// The record the conversation is being held under, where it is being held
    /// under one.
    pub record: Option<AboutRecord>,
}

/// A record, at the length that finding it again needs.
///
/// Two members where [`crate::sessions::live::About`] has three, and the third
/// is the one deliberately left behind: a title is what a heading says, and it
/// was already spent on the banner's own lines. What comes back from a click is
/// identity — which record, of what kind — because that is what opening one
/// takes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AboutRecord {
    pub key: String,
    pub kind: String,
}

/// This installation's choice, or the default nobody has changed yet.
pub(crate) fn preference<R: Runtime>(app: &AppHandle<R>) -> Preference {
    configuration_file(app, FILE)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// What the settings page draws.
#[tauri::command]
pub fn notifications_settings<R: Runtime>(app: AppHandle<R>) -> Preference {
    preference(&app)
}

/// Write the three switches down.
///
/// The whole preference rather than a switch, for the reason `voice_choose`
/// takes one: three controls changing one member each are three ways for the
/// file to end up describing a state nobody chose.
///
/// # Errors
///
/// When the configuration directory cannot be resolved or written.
#[tauri::command]
pub fn notifications_choose<R: Runtime>(
    app: AppHandle<R>,
    settings: Preference,
) -> Result<Preference, String> {
    let path = configuration_file(&app, FILE).map_err(|error| error.message)?;
    write_configuration(&path, &settings).map_err(|error| error.message)?;
    Ok(settings)
}

/// Something to hand a session so it can say what happened where a person will
/// see it with no window open.
///
/// One per session rather than one for the application, and it costs a cloned
/// handle: what a conversation announces to is named where the conversation is
/// made, so there is no moment when a session exists and this file has to go
/// looking for it.
pub fn announcer<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn Announce> {
    Arc::new(Announcer { app: app.clone() })
}

struct Announcer<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Announce for Announcer<R> {
    fn happened(&self, session: &Session, event: &SessionEvent) {
        let SessionEvent::Status { status, .. } = event else {
            return;
        };
        if in_front(&self.app) {
            return;
        }
        let Some(banner) = said(
            preference(&self.app),
            &session.agent_name,
            session.title().as_deref(),
            &named(&self.app, &session.project),
            *status,
        ) else {
            return;
        };

        raise(
            &self.app,
            &banner,
            &Address {
                project: session.project.clone(),
                conversation: session.key.clone(),
                record: session.about.as_ref().map(|about| AboutRecord {
                    key: about.key.clone(),
                    kind: about.kind.clone(),
                }),
            },
        );
    }
}

/// Put a banner on the screen.
///
/// Two ways of doing it, and the difference between them is what a click does.
/// The first carries the address and gives it back; the second is the plugin,
/// which shows a banner and forgets it — see `banner.rs` for why that is the
/// plugin rather than a choice, and why the process running from `tauri dev`
/// has only the second.
///
/// Nothing is done about a banner that could not be shown. The platform
/// refusing one is a person having turned Sync off in System Settings, which is
/// an answer rather than a fault — and the conversation it was about is in the
/// window either way.
fn raise<R: Runtime>(app: &AppHandle<R>, banner: &Banner, address: &Address) {
    #[cfg(target_os = "macos")]
    if banner::raise(banner, address) {
        return;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = address;

    let _ = app
        .notification()
        .builder()
        .title(banner.two_line_title())
        .body(banner.body.clone())
        .show();
}

/// Say what a click on a banner means, before there is one to click.
///
/// At launch rather than beside the first banner, and that is not tidiness:
/// clicking one raised before Sync was last quit *launches* Sync, and the
/// system hands that click to whatever is listening by the time the application
/// has finished starting. Something installed later is installed after the
/// click it was for.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        banner::attend(move |address| {
            let app = handle.clone();
            // Handed to the main thread rather than done here, for the reason
            // `crate::dock`'s menu item is: a window is made on the main
            // thread, and this is a callback from AppKit.
            let _ = handle.run_on_main_thread(move || opened(&app, &address));
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// What the window is told to come and read.
///
/// It carries nothing, and that is the point: the address is fetched with
/// [`notifications_addressed`], which is also how a window that has only just
/// been made gets it. One way in means a click cannot be answered twice — the
/// address is taken off the shelf by whoever reads it first — and it means the
/// hard case and the easy one are the same code.
pub const SHOWN: &str = "attention://shown";

/// The click a window has not come to collect yet.
///
/// Keyed by label, because it is one window's: two people's worth of clicks in
/// two windows are two separate things to go and look at.
///
/// It exists because a click can arrive at a window that does not exist yet. A
/// banner is shown when nothing of Sync is in front — often when Sync has no
/// window at all — so the ordinary answer to one is a window built to hold it,
/// and a window being built has nothing listening in it for several hundred
/// milliseconds. An event sent into that gap is not delayed, it is gone.
#[derive(Default)]
pub struct Addressed(std::sync::Mutex<std::collections::HashMap<String, Address>>);

/// What a banner sent this window to, if it was sent anywhere.
///
/// Read once and gone: a window asks when it starts and again when it is told
/// there is something, and a click that stayed on the shelf after being
/// answered would be answered again by the next window to start.
///
/// # Errors
///
/// When what the windows were sent to cannot be reached, which is a lock
/// poisoned by a panic elsewhere.
#[tauri::command]
pub fn notifications_addressed<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    state: tauri::State<'_, Addressed>,
) -> Result<Option<Address>, String> {
    Ok(state
        .0
        .lock()
        .map_err(|_| "what a banner addressed could not be read".to_owned())?
        .remove(window.label()))
}

/// What a click on a banner does.
///
/// Two halves, and the second is why the first cannot be the whole of it. A
/// window is put in front of the person — [`crate::windows::reveal`] decides
/// which — and then the address is left where that window will find it, because
/// which project a window has open is the window's own state and nothing in
/// Rust can reach in and change it.
///
/// A window that could not be found or made is not reported here. It is
/// reported where it happened, and there is nothing this could add: a banner
/// whose window would not open has already told the person as much as it can.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        dead_code,
        reason = "no banner off macOS carries an address to come back"
    )
)]
fn opened<R: Runtime>(app: &AppHandle<R>, address: &Address) {
    let Some(window) = crate::windows::reveal(app, &address.project) else {
        return;
    };
    let shelf = app.state::<Addressed>();
    let Ok(mut waiting) = shelf.0.lock() else {
        return;
    };
    waiting.insert(window.label().to_owned(), address.clone());
    drop(waiting);

    // To that window by name, never broadcast: every other window would go and
    // collect a click meant for one of them. A window that has not started yet
    // hears nothing, which is what it was left on the shelf for.
    if let Err(error) = app.emit_to(window.label(), SHOWN, ()) {
        eprintln!("the window could not be told what was clicked: {error}");
    }
}

/// Whether somebody is looking at this application right now.
///
/// Any window of it, the settings one included: a person with settings in front
/// of them is at this desk, and a banner over the window they are already in is
/// the interruption this whole file exists to avoid.
fn in_front<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.webview_windows()
        .values()
        .any(|window| window.is_focused().unwrap_or(false))
}

/// What a project is called in a sentence.
///
/// The name the window puts in its own title bar, which is the name a person
/// gave it — read from the registry this installation keeps, because that is a
/// file on this machine. The name in the project's own record would be the same
/// answer through the engine, and a banner is raised on the way past a session
/// event, in a path that must not wait on another process to find out what to
/// call something. The registry is written every time a project is opened, so
/// the two do not drift for longer than one open.
///
/// A project that is not registered is called after its directory, and a path
/// with no last component still leaves a sentence somebody can read.
fn named<R: Runtime>(app: &AppHandle<R>, project: &Path) -> String {
    crate::project::projects_registered(app.clone())
        .into_iter()
        .find(|registered| Path::new(&registered.path) == project)
        .map(|registered| registered.name)
        .unwrap_or_else(|| in_the_folder(project).to_owned())
}

/// The folder's own name, for a project nothing here has registered.
fn in_the_folder(project: &Path) -> &str {
    project
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("this project")
}

/// What a banner says, or nothing where this status is not one worth raising.
fn said(
    settings: Preference,
    agent: &str,
    conversation: Option<&str>,
    project: &str,
    status: Status,
) -> Option<Banner> {
    let what = match status {
        Status::Asking if settings.asking => "Waiting for an answer",
        Status::Ready if settings.finished => "Finished",
        Status::Failed if settings.failed => "Stopped",
        _ => return None,
    };

    Some(Banner {
        title: project.to_owned(),
        // Empty rather than the agent's name, which is on the line below
        // already. A banner with nothing to put here is a conversation nobody
        // has said anything in, and two lines is what that is worth.
        subtitle: conversation.unwrap_or_default().to_owned(),
        body: format!("{agent} · {what}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state somebody who has never opened the page is in. All three on:
    /// what keeps banners rare is the window being in front, and a person who
    /// never found the switches should still be told their agent is stuck.
    #[test]
    fn nobody_has_to_turn_this_on() {
        let settings = Preference::default();
        assert!(settings.asking);
        assert!(settings.finished);
        assert!(settings.failed);
    }

    /// A file from a build that had fewer switches must not read as three
    /// switches somebody turned off.
    #[test]
    fn a_preference_from_an_older_build_still_speaks() {
        let older: Preference =
            serde_json::from_str("{\"asking\":false}").expect("an older file is still readable");
        assert!(!older.asking, "what it does say is kept");
        assert!(older.finished, "what it does not say is not a refusal");
        assert!(older.failed);
    }

    /// A file that is nonsense is the default rather than silence.
    #[test]
    fn a_preference_that_cannot_be_read_is_the_default() {
        let read = serde_json::from_str::<Preference>("{\"asking\": 7}");
        assert!(read.is_err());
        assert_eq!(read.unwrap_or_default(), Preference::default());
    }

    #[test]
    fn the_three_events_are_the_only_ones() {
        let all = Preference::default();
        for quiet in [Status::Starting, Status::Working, Status::Ended] {
            assert_eq!(
                said(all, "Claude", None, "sync", quiet),
                None,
                "{quiet:?} is the agent doing what it was left doing"
            );
        }
        for loud in [Status::Asking, Status::Ready, Status::Failed] {
            assert!(said(all, "Claude", None, "sync", loud).is_some());
        }
    }

    /// Each switch silences its own event and no other.
    #[test]
    fn a_switch_turned_off_silences_one_event() {
        let settings = Preference {
            asking: false,
            ..Preference::default()
        };
        assert_eq!(said(settings, "Claude", None, "sync", Status::Asking), None);
        assert!(said(settings, "Claude", None, "sync", Status::Ready).is_some());
    }

    /// The lines go coarse to fine: the project, the conversation in it, and
    /// the agent with what happened. A conversation nobody has said anything in
    /// has no name yet, and its line is left empty rather than filled with the
    /// agent's name — which is on the line below already.
    #[test]
    fn a_banner_reads_project_then_conversation_then_what_happened() {
        let named = said(
            Preference::default(),
            "Claude",
            Some("Rename the badge column"),
            "Sync",
            Status::Ready,
        )
        .expect("finishing is raised");
        assert_eq!(named.title, "Sync");
        assert_eq!(named.subtitle, "Rename the badge column");
        assert_eq!(named.body, "Claude · Finished");

        let unnamed = said(
            Preference::default(),
            "Claude",
            None,
            "Sync",
            Status::Asking,
        )
        .expect("asking is raised");
        assert_eq!(unnamed.title, "Sync");
        assert_eq!(unnamed.subtitle, "");
        assert_eq!(unnamed.body, "Claude · Waiting for an answer");
    }

    /// A platform with two lines gets both halves of *where would I land*, and
    /// a conversation with no name yet leaves the project standing alone rather
    /// than trailing a colon.
    #[test]
    fn two_lines_keep_the_project_and_the_conversation() {
        let named = said(
            Preference::default(),
            "Claude",
            Some("Rename the badge column"),
            "Sync",
            Status::Asking,
        )
        .expect("asking is raised");
        assert_eq!(named.two_line_title(), "Sync: Rename the badge column");
        assert_eq!(named.body, "Claude · Waiting for an answer");

        let unnamed = said(
            Preference::default(),
            "Claude",
            None,
            "Sync",
            Status::Asking,
        )
        .expect("asking is raised");
        assert_eq!(unnamed.two_line_title(), "Sync");
    }

    /// A project this installation has never registered is called after its
    /// folder, and a path that has no last component still leaves a sentence
    /// somebody can read.
    #[test]
    fn an_unregistered_project_is_called_after_its_folder() {
        assert_eq!(
            in_the_folder(Path::new("/Users/someone/Projects/sync")),
            "sync"
        );
        assert_eq!(in_the_folder(Path::new("/")), "this project");
    }
}
