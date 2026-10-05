//! An address followed from outside this application, and where it lands.
//!
//! A record has a url — `sync://<kind>/<key>` — and until now it meant
//! something only inside the window that wrote it: `src/lib/record-link.ts`
//! parses one out of a body and opens the record beside it. This is the other
//! half. The scheme is registered with the system, so the same address in a
//! terminal, a chat client or somebody's notes is a link, and following one
//! brings the project it names to the front with the record open.
//!
//! # Two spellings, and why the second exists
//!
//! - `sync://<kind>/<key>` names a record and leaves the project to be
//!   understood. It is the spelling every body in every corpus already holds,
//!   and inside a window it is exactly right: the window has a project open, and
//!   the link was written in one of its records.
//! - `sync://<project>/<kind>/<key>` names the project as well, by the
//!   identifier it answers to here — what `sync_projects` lists. This one exists
//!   because the first cannot survive leaving the window: this machine answers
//!   for every project a person has opened, and an address that does not say
//!   which one is an address pointing at all of them at once.
//!
//! The price of the second spelling is that the two are told apart by how many
//! segments they have, so a key with a raw `/` in it would read as a project.
//! Nothing Sync writes has one — `link.rs` in `sync-mcp` escapes every segment,
//! which is the same round trip the window's parser decodes.
//!
//! # Which window, and in what order
//!
//! [`crate::windows::reveal`] decides, and it is the same decision a banner's
//! click gets — the window holding the project, a window holding nothing, the
//! window a launch is still making, a new one. That is not a coincidence worth
//! removing: *put this project in front of somebody* has one answer, and a
//! second implementation of it would be a second answer to the same question.
//!
//! What is left to this module is the address itself: what it names, which
//! working tree that is on this machine, and the shelf a window collects it
//! from. The shelf is here rather than beside the banner's because the two carry
//! different things — a banner is about a conversation and this is not — and the
//! mechanism it shares with that one is a handful of lines, while a shelf
//! holding either would be a shelf whose readers have to ask which kind arrived.
//!
//! # What an address cannot do
//!
//! Choose a project nobody has opened here. The identifier is resolved against
//! the registry this installation keeps, so an address written on another
//! machine, or one naming a project that has been forgotten, reaches nothing and
//! says so on stdout. Guessing — the same key in another project, a search
//! across every corpus on the machine — would open somebody's window on a record
//! that is not the one the link was written about.
//!
//! # It only works installed
//!
//! macOS hears of the scheme from `CFBundleURLTypes`, which the bundler writes
//! from `plugins > deep-link > desktop` in `tauri.conf.json`, and Launch
//! Services reads that from an installed bundle. A build from source has no
//! bundle, so `pnpm tauri dev` never receives an address at all — like the CSP
//! and like a banner that can be clicked, this is verified against `pnpm tauri
//! build` and an application in `/Applications`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use percent_encoding::percent_decode_str;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, Runtime};
use tauri_plugin_deep_link::DeepLinkExt as _;

/// The scheme this application answers for.
///
/// `SCHEME` in `sync-mcp`'s `link.rs` and `RECORD_SCHEME` in
/// `src/lib/record-link.ts` are the same constant, written and read.
pub const SCHEME: &str = "sync";

/// What an address names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// The project, by the identifier it answers to on this machine — or `None`
    /// for the two-segment spelling, which leaves it to be understood.
    pub project: Option<String>,
    pub kind: String,
    pub key: String,
}

/// Read an address, or `None` for a url this application does not own.
///
/// Parsed rather than handed to `Url`, for the reason the window's parser is:
/// what the segments mean here is decided by *this* grammar, and a url type's
/// idea of a host is a place for a normalisation nobody asked for to happen to
/// somebody's project identifier or kind. The segments are unescaped exactly as
/// far as `link.rs` escaped them.
#[must_use]
pub fn named(url: &str) -> Option<Named> {
    let (scheme, rest) = url.trim().split_once("://")?;
    // A scheme is case-insensitive everywhere, and an address that came back
    // through a chat client that capitalises the start of a line is the same
    // address.
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }

    // A query and a fragment are dropped: nothing in this grammar reads either,
    // and an address that arrived with a tracking parameter on it still names
    // the record it names.
    let path = rest.split(['?', '#']).next()?;
    // One trailing slash is forgiven — it is what a client that thinks it is
    // looking at a web address adds — and a second one is not, because by then
    // the thing being read is not an address.
    let path = path.strip_suffix('/').unwrap_or(path);

    let mut segments = Vec::new();
    for raw in path.split('/') {
        if raw.is_empty() {
            return None;
        }
        segments.push(percent_decode_str(raw).decode_utf8().ok()?.into_owned());
    }

    let mut segments = segments.into_iter();
    match (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) {
        (Some(kind), Some(key), None, None) => Some(Named {
            project: None,
            kind,
            key,
        }),
        (Some(project), Some(kind), Some(key), None) => Some(Named {
            project: Some(project),
            kind,
            key,
        }),
        _ => None,
    }
}

/// The record a window is told to come and open.
///
/// The project is a working tree rather than the identifier the address carried:
/// resolving it is this side's job, and a window opens a project by its path.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Followed {
    /// The project's working tree, which is what a window has open.
    pub project: PathBuf,
    pub key: String,
    pub kind: String,
}

/// The event sent to the window an address landed in.
///
/// It carries nothing, like the banner's: the address is fetched with
/// [`link_followed`], which is also how a window that has only just been made
/// gets it. One way in means an address cannot be answered twice.
pub const FOLLOWED: &str = "link://followed";

/// The address a window has not come to collect yet, keyed by label.
///
/// It exists because following a link is very often what *starts* Sync: the
/// address is delivered while the window that will answer it is still building
/// its webview, and an event sent into that gap is not delayed, it is gone.
#[derive(Default)]
pub struct Waiting(Mutex<HashMap<String, Followed>>);

/// What a link sent this window to, if it was sent anywhere.
///
/// Read once and gone, for the reason a banner's address is: a window asks when
/// it starts and again when it is told there is something, and an address left
/// on the shelf after being answered would be answered again by the next window
/// to start.
///
/// # Errors
///
/// When the shelf cannot be reached, which is a lock poisoned by a panic
/// elsewhere.
#[tauri::command]
pub fn link_followed<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    state: tauri::State<'_, Waiting>,
) -> Result<Option<Followed>, String> {
    Ok(state
        .0
        .lock()
        .map_err(|_| "what a link pointed at could not be read".to_owned())?
        .remove(window.label()))
}

/// Follow an address from inside the window.
///
/// The window follows a link to a record of the project it has open itself —
/// nothing leaves the webview for that, and nothing should. This is for the
/// other case: a body or a message naming a record of *another* project, which
/// is one window's link and another window's record, and choosing between
/// windows is not something a webview can do.
///
/// # Errors
///
/// When the address names nothing this machine can open. The window has drawn
/// the link as followable by then, so the refusal is answered rather than
/// swallowed: what it says is what there is to tell somebody.
#[tauri::command]
pub fn link_follow<R: Runtime>(app: AppHandle<R>, url: String) -> Result<(), String> {
    let Some(named) = named(&url) else {
        return Err(format!("`{url}` is not an address this application owns"));
    };
    let Some(project) = tree(&app, &named) else {
        return Err(match &named.project {
            Some(identifier) => format!("`{identifier}` is not a project on this machine"),
            None => "no window has a project open, so there is nowhere to show this".to_owned(),
        });
    };
    land(&app, project, named);
    Ok(())
}

/// Listen for an address for as long as this application runs.
///
/// Installed at launch rather than beside the first window, for the reason the
/// banner's listener is: following a link is what launches Sync as often as not,
/// and something installed later is installed after the click it was for.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            let app = handle.clone();
            let url = url.to_string();
            // Onto the main thread, because answering an address makes and shows
            // windows and that is a main-thread errand on every platform. The
            // handler itself runs wherever the event was emitted.
            if let Err(error) = handle.run_on_main_thread(move || follow(&app, &url)) {
                eprintln!("an address could not be answered: {error}");
            }
        }
    });

    // The launch that *was* the click, off the Mac. There an address arrives as
    // a command line argument and the plugin reads it while it is initialising —
    // which is before this listener exists, so the one address that starts the
    // application would be the one address nobody hears. On macOS it arrives as
    // an event instead and this is empty every time.
    match app.deep_link().get_current() {
        Ok(Some(urls)) => {
            for url in urls {
                follow(app, url.as_str());
            }
        }
        Ok(None) => {}
        Err(error) => eprintln!("the address this launch carried could not be read: {error}"),
    }
}

/// What following an address does.
///
/// Nothing here is reported to a person, and that is the difference between this
/// and [`link_follow`]: an address arriving from another application arrives with
/// nobody looking at Sync, and there is no window to answer into. What it does
/// instead is say so where the log is read and leave the screen alone — a window
/// brought to the front showing the wrong project would be worse than a click
/// that did nothing.
fn follow<R: Runtime>(app: &AppHandle<R>, url: &str) {
    let Some(named) = named(url) else {
        eprintln!("`{url}` is not an address this application owns");
        return;
    };
    let Some(project) = tree(app, &named) else {
        return;
    };
    land(app, project, named);
}

/// Put the window in front and leave the record where it will find it.
///
/// Two halves, and the second is why the first cannot be the whole of it: which
/// project a window has open is the window's own state, so nothing here can
/// reach in and change it. The window is chosen and revealed, and the record is
/// left on the shelf for it to collect.
fn land<R: Runtime>(app: &AppHandle<R>, project: PathBuf, named: Named) {
    let Some(window) = crate::windows::reveal(app, &project) else {
        // Reported where it happened. A window that would not open has already
        // told the person as much as it can.
        return;
    };

    let followed = Followed {
        project,
        key: named.key,
        kind: named.kind,
    };
    let shelf = app.state::<Waiting>();
    let Ok(mut waiting) = shelf.0.lock() else {
        return;
    };
    waiting.insert(window.label().to_owned(), followed);
    drop(waiting);

    // To that window by name, never broadcast: every other window would go and
    // collect an address meant for one of them. A window that has not started
    // yet hears nothing, which is what the shelf is for.
    if let Err(error) = app.emit_to(window.label(), FOLLOWED, ()) {
        eprintln!("the window could not be told what was followed: {error}");
    }
}

/// Which working tree on this machine an address is about.
///
/// The identifier is resolved against the registry, which is the file that says
/// what this installation answers for — the same answer an agent naming a
/// project in a call gets, because it is the same list.
///
/// An address that names no project falls to the project somebody is looking at.
/// That is a guess, and it is the only honest one available: the two-segment
/// spelling is every link written before this existed, and the window a person
/// is reading in is the context they wrote it from.
fn tree<R: Runtime>(app: &AppHandle<R>, named: &Named) -> Option<PathBuf> {
    let Some(identifier) = named.project.as_deref() else {
        return crate::windows::foremost(app);
    };

    let found = crate::project::projects_registered(app.clone())
        .into_iter()
        .find(|registered| registered.identifier == identifier)
        .map(|registered| PathBuf::from(registered.path));

    if found.is_none() {
        eprintln!(
            "`{identifier}` is not a project this installation answers for, \
             so an address naming it reaches nothing here"
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spelling the window writes and the one it reads, stated in full
    /// rather than derived: `href` in `sync-mcp`'s `link.rs` writes these, and
    /// an address the two sides disagree about is a link that opens nothing.
    #[test]
    fn both_spellings_are_the_ones_that_are_written() {
        assert_eq!(
            named("sync://decision/d-one"),
            Some(Named {
                project: None,
                kind: "decision".to_owned(),
                key: "d-one".to_owned(),
            })
        );
        assert_eq!(
            named("sync://SYNC/tasks.task/task-a92e5b"),
            Some(Named {
                project: Some("SYNC".to_owned()),
                kind: "tasks.task".to_owned(),
                key: "task-a92e5b".to_owned(),
            })
        );
    }

    /// An identifier is upper case, and a kind may be spelled with a capital.
    /// Nothing here may quietly fold either: a kind decides which section opens
    /// the record, and an identifier decides which project does.
    #[test]
    fn the_case_of_a_segment_survives() {
        let named = named("sync://SYNC/Decision/D-One").expect("an address");
        assert_eq!(named.project.as_deref(), Some("SYNC"));
        assert_eq!(named.kind, "Decision");
        assert_eq!(named.key, "D-One");
    }

    /// Only the scheme is case-insensitive, because that is what a scheme is.
    #[test]
    fn the_scheme_is_the_one_thing_read_either_way() {
        assert!(named("Sync://decision/d-one").is_some());
        assert!(named("SYNC://decision/d-one").is_some());
    }

    /// Every segment is escaped on the way out, so every one is unescaped on the
    /// way in — and a kind with a space in it is a kind, not a truncated url.
    #[test]
    fn a_segment_is_unescaped_as_far_as_it_was_escaped() {
        let named = named("sync://SYNC/a%20kind/d%281%29").expect("an address");
        assert_eq!(named.kind, "a kind");
        assert_eq!(named.key, "d(1)");
    }

    /// What arrives from another application is whatever somebody typed, so an
    /// address that is not one is answered as not one rather than by opening
    /// something approximate.
    #[test]
    fn what_is_not_an_address_is_not_read_as_one() {
        assert!(named("https://example.com/decision/d-one").is_none());
        assert!(named("sync://decision").is_none());
        assert!(named("sync://SYNC/tasks.task/task-1/extra").is_none());
        assert!(named("sync://").is_none());
        assert!(named("sync:///d-one").is_none());
        assert!(named("sync://decision//d-one").is_none());
        assert!(named("nonsense").is_none());
    }

    /// A query is somebody's client adding to the end of a url, and the record
    /// it names is the record it named before that happened.
    #[test]
    fn a_query_or_a_fragment_does_not_change_which_record_it_is() {
        let named = named("sync://SYNC/tasks.task/task-1?from=chat#top").expect("an address");
        assert_eq!(named.key, "task-1");
        assert_eq!(named.project.as_deref(), Some("SYNC"));
    }

    /// One trailing slash is what a client that thinks it has a web address
    /// adds. Two is not that, and is answered as nothing.
    #[test]
    fn one_trailing_slash_is_forgiven_and_two_are_not() {
        assert_eq!(
            named("sync://decision/d-one/").map(|named| named.key),
            Some("d-one".to_owned())
        );
        assert!(named("sync://decision/d-one//").is_none());
    }
}
