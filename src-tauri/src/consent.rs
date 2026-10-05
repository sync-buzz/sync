//! Which of a person's own servers a package may call through the flagship,
//! agreed to once and kept.
//!
//! A package that draws a screen over somebody else's tools cannot call one
//! because its manifest asked to: the manifest says *this build spends turns of
//! your agent*, which is a fact about the build, and this says *and it may
//! spend one on this server*, which is a decision about a person's own account.
//! Both are needed, and the second is here.
//!
//! # Why it is not asked for during the turn
//!
//! Because there is nothing to ask it in. The turn that carries an ask has no
//! window of its own by design — it runs for a panel that is redrawing, or for
//! a clock, with nobody looking at it — so a question raised inside one waits
//! for somebody who is not there until the turn's patience runs out. The
//! agreement is taken where a person is already reading and deciding: the
//! package's own page, beside what it brings and what else it reaches, before
//! its first turn ever runs.
//!
//! # How wide one agreement is, and why it is not narrower
//!
//! One package and one server: every tool that server publishes, including the
//! ones that delete things. That is wider than anybody would design on a blank
//! page, and it is the honest width of what can be asked for today.
//!
//! **Sync cannot enumerate a server's tools.** It speaks no MCP, and the
//! flagship's own configuration holds a server's key and how to reach it and
//! nothing else. Two parties know the names: the person, who would have to
//! recall them from memory in the middle of installing something, and the agent
//! that would be asked, whose answer comes back to the window only after a turn
//! has been spent. So the row a person presses says *this server, every tool of
//! it*, and says it before the first turn rather than after.
//!
//! **What is stored is the pair with no tool named.** A row says *this package,
//! this server*; the day a narrower answer can be asked for, a named tool is a
//! member added to that row, and every row without one goes on meaning the
//! whole server. So narrowing later is a refinement of this file rather than a
//! migration of it — which is also how the rows written before this width was
//! settled are read: a row that names a tool is an agreement about the server
//! it names, and the name is dropped the next time anything is written.
//!
//! # Where it is kept
//!
//! Beside the flagship choice, in this installation's own configuration
//! directory, for the same two reasons: a package is installed on this machine
//! rather than in a project, and the turns it spends are spent through one
//! person's agent whatever project is open. A second copy of the answer in a
//! repository would travel to a colleague as *you agreed to this*, which they
//! did not.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::project::{ProjectError, configuration_file, write_configuration};

/// Where the agreements are kept, beside this installation's other files.
const FILE: &str = "tool-consent.json";

/// One agreement: one package, one of the person's servers.
///
/// A row carrying a member this does not declare is read without it, which is
/// what makes a row written under a narrower rule readable here: `serde` drops
/// what it cannot place, the row keeps naming the package and the server, and
/// the agreement it stands for is the one this build knows how to honour.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Agreement {
    /// The package's id, as this machine serves it.
    extension: String,
    /// The MCP server, as the flagship's own configuration keys it.
    server: String,
}

/// The file, and the whole of it.
///
/// A record around the list rather than a bare array, for the reason the
/// flagship's file is a record around one member: the day this has to say when
/// an agreement was given, or which build of the package was on screen when it
/// was, an array would have to be migrated to hold it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Agreed {
    #[serde(default)]
    granted: Vec<Agreement>,
}

/// A server name that matches every server, for a package a person has agreed
/// may call any of their MCP servers rather than one.
///
/// The narrow rule this file keeps is one package and one server, and a row
/// that names a server is still that. A wildcard is a second kind of row, given
/// deliberately and read back as *this package, any server* — the width a
/// drawing surface needs when the set of servers it reaches is decided in the
/// project (a node names `playwright.navigate` today and `stripe.get_account`
/// tomorrow) rather than fixed in the package. It is wider than the per-server
/// row, and it is the width the person asked for, on the package's own page.
pub(crate) const WILDCARD_SERVER: &str = "*";

/// One package, with every server it was agreed it may call.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentedExtension {
    /// The id every agreement is stored against.
    pub id: String,
    /// What a person reads: the package's own name where this machine still
    /// serves it, and the id where it does not.
    pub name: String,
    /// Whether this machine still serves that id.
    ///
    /// An agreement outlives the package it was given to — nothing goes
    /// looking through this file when something is uninstalled — and a screen
    /// that hid those would leave a person unable to withdraw an agreement
    /// they can see they gave. Reinstalling the same id would then find it
    /// still standing, which is the surprise this member exists to prevent.
    pub installed: bool,
    /// The servers, keyed as the flagship's configuration keys them.
    pub servers: Vec<String>,
}

/// Read the agreements, or the state of having made none.
///
/// A file that cannot be read is no agreements, which refuses calls rather than
/// allowing them. Unreadable is *not* treated as an error the window has to
/// show: the safe reading and the honest one are the same here, and a settings
/// section that refused to draw would leave somebody unable to fix the file it
/// was complaining about.
///
/// Repeated pairs are collapsed on the way in. Nothing this build writes can
/// make one; a file written when an agreement named a tool has one row per tool
/// and every row says the same thing now, and a person reading a server listed
/// three times would be reading about the file rather than about their machine.
fn agreed<R: Runtime>(app: &AppHandle<R>) -> Agreed {
    let mut agreed: Agreed = configuration_file(app, FILE)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    collapse(&mut agreed);
    agreed
}

/// One row per thing agreed to, keeping the order they were agreed in.
fn collapse(agreed: &mut Agreed) {
    let mut kept: Vec<Agreement> = Vec::new();
    agreed.granted.retain(|agreement| {
        if kept.contains(agreement) {
            return false;
        }
        kept.push(agreement.clone());
        true
    });
}

fn keep<R: Runtime>(app: &AppHandle<R>, agreed: &Agreed) -> Result<(), ProjectError> {
    let path = configuration_file(app, FILE)?;
    write_configuration(&path, agreed)
}

/// Whether a package may call this server.
///
/// The one question the door asks, and it is asked on every call rather than
/// remembered: withdrawing an agreement has to take effect on the next call,
/// and anything cached would keep answering *yes* for as long as the window
/// stayed open.
///
/// Which tool of it is deliberately not an argument. There is no answer this
/// could give about one tool that it does not give about every other, and a
/// parameter that is accepted and ignored is a caller's belief that something
/// was checked.
pub(crate) fn allows<R: Runtime>(app: &AppHandle<R>, extension: &str, server: &str) -> bool {
    allowed(&agreed(app), extension, server)
}

fn allowed(agreed: &Agreed, extension: &str, server: &str) -> bool {
    agreed.granted.iter().any(|agreement| {
        agreement.extension == extension
            && (agreement.server == server || agreement.server == WILDCARD_SERVER)
    })
}

/// What a person agreed to for one package, beside whatever it held already.
///
/// Adding rather than replacing, and that is the decision in this function. An
/// agreement is given one row at a time — one server, on the page of the
/// package that would call it — and taken back the same way, so replacing would
/// make agreeing to a second server a silent withdrawal of the first. Nothing
/// on that page says so, and the person would find out by a panel going quiet.
///
/// Agreeing to the same pair twice is one agreement, because two rows saying
/// the same thing are a fact about the file: the settings screen would list it
/// twice and a person reading their own machine would be counting them.
fn grant(agreed: &mut Agreed, extension: &str, server: &str) {
    if allowed(agreed, extension, server) {
        return;
    }
    agreed.granted.push(Agreement {
        extension: extension.to_owned(),
        server: server.to_owned(),
    });
}

/// Take one agreement back. The others of that package stand.
fn revoke(agreed: &mut Agreed, extension: &str, server: &str) {
    agreed
        .granted
        .retain(|agreement| !(agreement.extension == extension && agreement.server == server));
}

/// The agreements grouped by package, in the order they were given.
///
/// The name is looked up here rather than in the window, because it is the only
/// place both facts are in hand: the file says which ids were agreed to, and
/// the artefact directory says what those ids are called. A window that joined
/// them would be asking two commands a question that has one answer.
fn listed<R: Runtime>(app: &AppHandle<R>, agreed: &Agreed) -> Vec<ConsentedExtension> {
    // An artefact directory that cannot be read costs names, not agreements:
    // every row then reads as its id, and every one of them can still be
    // withdrawn.
    let store = crate::extensions::store(app).ok();
    let mut listed: Vec<ConsentedExtension> = Vec::new();
    for agreement in &agreed.granted {
        if let Some(held) = listed
            .iter_mut()
            .find(|held| held.id == agreement.extension)
        {
            held.servers.push(agreement.server.clone());
            continue;
        }
        let installed = store
            .as_ref()
            .and_then(|store| store.resolve(&agreement.extension).ok().flatten());
        listed.push(ConsentedExtension {
            id: agreement.extension.clone(),
            name: installed.as_ref().map_or_else(
                || agreement.extension.clone(),
                |installed| installed.manifest.name.clone(),
            ),
            installed: installed.is_some(),
            servers: vec![agreement.server.clone()],
        });
    }
    listed
}

/// Everything this installation has agreed a package may call.
///
/// Answers with a list and never with a failure, for the reason
/// [`crate::flagship::flagship_status`] does not fail either: the screen this
/// draws is the one somebody opens to take an agreement back, and a screen that
/// refused to draw would take that away exactly when they wanted it.
#[tauri::command]
pub fn tool_consent_status<R: Runtime>(app: AppHandle<R>) -> Vec<ConsentedExtension> {
    let agreed = agreed(&app);
    listed(&app, &agreed)
}

/// Agree that one package may call one server.
///
/// **This is what the package's own page calls**, where a person is reading
/// what that package brings and what it reaches, one server at a time. Nothing
/// else in the application may write an agreement, and no package can write its
/// own — the command takes an id, and a package cannot state one, because the
/// door it holds is built with its id already closed over.
///
/// The package need not be installed yet, and the check is deliberately not
/// added: requiring the artefact first would make the order of agreeing and
/// installing a rule nobody wrote down, and an agreement naming an id this
/// machine never comes to serve is dead text that the settings section still
/// lets somebody delete.
///
/// # Errors
///
/// When the package or the server is not named — an agreement that names
/// nothing is not a narrower agreement, it is a row that can never match and
/// never be understood — and when the file cannot be written.
#[tauri::command]
pub fn tool_consent_grant<R: Runtime>(
    app: AppHandle<R>,
    extension: String,
    server: String,
) -> Result<Vec<ConsentedExtension>, ProjectError> {
    if extension.trim().is_empty() || server.trim().is_empty() {
        return Err(ProjectError::new(
            "nothing_named",
            "An agreement names a package and a server.".to_owned(),
        ));
    }

    let mut agreed = agreed(&app);
    grant(&mut agreed, &extension, &server);
    keep(&app, &agreed)?;
    Ok(listed(&app, &agreed))
}

/// Take one agreement back, and answer with what the section should now show.
///
/// The next call to that server is refused, by the same name it would have been
/// refused under before anybody agreed. Nothing is stopped mid-flight: a turn
/// already running was allowed when it started, and killing it here would leave
/// a panel with half an answer and no account of why.
///
/// # Errors
///
/// When the file cannot be written. Withdrawing something that was not there is
/// not one: it leaves the person where they wanted to be.
#[tauri::command]
pub fn tool_consent_revoke<R: Runtime>(
    app: AppHandle<R>,
    extension: String,
    server: String,
) -> Result<Vec<ConsentedExtension>, ProjectError> {
    let mut agreed = agreed(&app);
    revoke(&mut agreed, &extension, &server);
    keep(&app, &agreed)?;
    Ok(listed(&app, &agreed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_allowed_until_somebody_agrees() {
        let agreed = Agreed::default();
        assert!(!allowed(&agreed, "panel", "sync"));
    }

    /// A wildcard row agrees to every server for that package and no server for
    /// any other. The width a drawing surface needs when the servers it reaches
    /// are decided in the project, not in the package.
    #[test]
    fn a_wildcard_agreement_allows_every_server_for_that_package() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "workflows", WILDCARD_SERVER);

        assert!(allowed(&agreed, "workflows", "playwright"));
        assert!(allowed(&agreed, "workflows", "stripe"));
        // Another package is not covered: a wildcard is per package.
        assert!(!allowed(&agreed, "other-panel", "playwright"));
    }

    /// A named server is still agreed to by its name, and a wildcard does not
    /// erase it. The two coexist, and withdrawing the wildcard leaves the named
    /// one standing — the same rule every other pair already follows.
    #[test]
    fn a_wildcard_does_not_replace_a_named_agreement() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "workflows", "playwright");
        grant(&mut agreed, "workflows", WILDCARD_SERVER);

        assert!(allowed(&agreed, "workflows", "playwright"));
        assert!(allowed(&agreed, "workflows", "stripe"));

        revoke(&mut agreed, "workflows", WILDCARD_SERVER);
        assert!(allowed(&agreed, "workflows", "playwright"));
        assert!(!allowed(&agreed, "workflows", "stripe"));
    }

    #[test]
    fn what_was_agreed_to_is_allowed_and_its_neighbours_are_not() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "panel", "sync");

        assert!(allowed(&agreed, "panel", "sync"));
        // Another server of the same person, which is another decision.
        assert!(!allowed(&agreed, "panel", "tracker"));
        // And another package, which is another decision entirely.
        assert!(!allowed(&agreed, "other-panel", "sync"));
    }

    /// Two servers for one package are two agreements, and the second does not
    /// take the first away. The page gives them one row at a time, and a row
    /// pressed here must not be a row withdrawn up there.
    #[test]
    fn agreeing_to_a_second_server_leaves_the_first_standing() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "panel", "sync");
        grant(&mut agreed, "panel", "tracker");

        assert!(allowed(&agreed, "panel", "tracker"));
        assert!(allowed(&agreed, "panel", "sync"));
    }

    /// And the same one twice is still one. A screen that asked again — a
    /// second window, a row pressed twice — would otherwise leave the person a
    /// list with a duplicate in it and two things to withdraw.
    #[test]
    fn agreeing_to_the_same_server_twice_is_one_agreement() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "panel", "sync");
        grant(&mut agreed, "panel", "sync");

        assert_eq!(agreed.granted.len(), 1);
    }

    #[test]
    fn agreeing_for_one_package_leaves_another_alone() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "panel", "sync");
        grant(&mut agreed, "other-panel", "tracker");
        grant(&mut agreed, "panel", "sync");

        assert!(allowed(&agreed, "other-panel", "tracker"));
        assert!(allowed(&agreed, "panel", "sync"));
    }

    #[test]
    fn withdrawing_one_agreement_leaves_the_rest_standing() {
        let mut agreed = Agreed::default();
        grant(&mut agreed, "panel", "sync");
        grant(&mut agreed, "other-panel", "sync");
        revoke(&mut agreed, "panel", "sync");

        assert!(!allowed(&agreed, "panel", "sync"));
        assert!(allowed(&agreed, "other-panel", "sync"));
    }

    /// The criterion that cannot be met by anything held in memory: the
    /// agreement is on the disk, and the read that follows is a different read.
    ///
    /// Written and read through the same two functions the commands use, so
    /// what this proves is the round trip rather than serde's opinion of a
    /// struct.
    #[test]
    fn an_agreement_survives_being_written_and_read_again() {
        let directory = tempfile::tempdir().expect("a directory to write in");
        let path = directory.path().join(FILE);

        let mut agreed = Agreed::default();
        grant(&mut agreed, "panel", "sync");
        write_configuration(&path, &agreed).expect("the agreements are written");

        let text = std::fs::read_to_string(&path).expect("the file is there");
        let read: Agreed = serde_json::from_str(&text).expect("and it reads back");
        assert!(allowed(&read, "panel", "sync"));
        assert!(!allowed(&read, "panel", "tracker"));
    }

    /// A file written when an agreement named a tool still says what it always
    /// said: this package, this server. The name of the tool goes, because
    /// there is nothing left that could honour it — and a person who agreed
    /// that a screen may read their server keeps the screen they had.
    ///
    /// Read rather than converted: nothing walks this file looking for old
    /// rows, and the next thing anybody agrees to or withdraws writes it out
    /// in the shape this build keeps.
    #[test]
    fn a_row_written_when_a_tool_was_named_is_an_agreement_about_its_server() {
        let read: Agreed = serde_json::from_str(
            r#"{ "granted": [
                 { "extension": "railway", "server": "railway", "tool": "list_projects" },
                 { "extension": "railway", "server": "railway", "tool": "list_services" }
               ] }"#,
        )
        .expect("a file written under the narrower rule still reads");

        assert!(allowed(&read, "railway", "railway"));
        assert!(!allowed(&read, "railway", "somewhere-else"));
    }

    /// And it is one agreement rather than one per tool it used to name. Two
    /// rows saying the same thing are a fact about the file, and a person
    /// reading their own machine would be counting them.
    #[test]
    fn rows_that_now_say_the_same_thing_are_one_agreement() {
        let read: Agreed = serde_json::from_str(
            r#"{ "granted": [
                 { "extension": "railway", "server": "railway", "tool": "list_projects" },
                 { "extension": "railway", "server": "railway", "tool": "list_services" }
               ] }"#,
        )
        .expect("a file written under the narrower rule still reads");

        let mut collapsed = read;
        collapse(&mut collapsed);
        assert_eq!(collapsed.granted.len(), 1);
    }

    /// A file somebody edited into nonsense allows nothing, rather than
    /// throwing the settings window away.
    #[test]
    fn a_file_that_is_not_agreements_is_no_agreements() {
        let read: Agreed = serde_json::from_str("{ not json").unwrap_or_default();
        assert!(!allowed(&read, "panel", "sync"));
    }
}
