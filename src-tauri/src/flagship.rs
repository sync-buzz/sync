//! The agent this installation works through.
//!
//! Everywhere else an agent is raised, somebody asked for it in a conversation
//! and is watching what it does. This one is raised on the window's behalf, for
//! work nobody is looking at, so which agent it is has to be a choice rather
//! than a guess: a machine may have five of them installed, they answer to
//! different accounts, and every turn costs the person money.
//!
//! It belongs to the installation like the rest of the settings window, and for
//! a firmer reason than most of them: what the choice reaches is a file in the
//! person's home directory, the same file whatever project is open. A choice
//! per project would be several answers to a question asked once.
//!
//! Kept in a file rather than in the window's storage for the reason the voice
//! preference is: it has to be readable with no window open at all.
//!
//! # Why an agent may not be chosen
//!
//! Two conditions, and neither is this module's opinion — both are somebody
//! else's measurement, read here.
//!
//! It has to have completed a full turn. `acp_client::registry` records how far
//! each row was ever proven, and an agent that has never answered a prompt
//! cannot be asked to do this one.
//!
//! And Sync has to know where that program keeps its own servers, because that
//! list is the whole of what this choice is for. The pairing lives in
//! [`crate::connect`], beside the files, because the two catalogues describe one
//! program from opposite sides and neither id can be derived from the other.
//!
//! # What the choice is spent on
//!
//! Two things, both here. [`flagship_servers`] reads that list of servers off
//! the file, without a model and without starting anything. [`flagship_call`]
//! is the other direction: one tool of one of those servers, called through a
//! turn of that agent, answering with the JSON the tool itself returned.
//!
//! They belong together because they are one bargain. Sync speaks no MCP and
//! holds no key of anybody's; what it has instead is a program the person has
//! already set up and authorised, and both of these are that program being
//! asked something it already knows.
//!
//! [`extension_tool_call`] is the second of those two with a name on it. An
//! installed package may ask as well, and what it asks is checked twice before
//! an agent is raised: against its own manifest, and against the servers the
//! person agreed that package may call — [`crate::consent`] holds the second. The
//! package cannot say who it is in either check, because the door it holds was
//! built with its id already in it.

use acp_client::{Consent, McpToolName, errand, launch, registry};
use serde::{Deserialize, Serialize};
use sync_extensions::manifest::TOOLS_CAPABILITY;
use tauri::{AppHandle, Runtime};

use crate::project::{ProjectError, configuration_file, write_configuration};

/// Where the choice is kept, beside this installation's other files.
const FILE: &str = "flagship.json";

/// What is written down, and the whole of it.
///
/// The agent this installation works through, raised on the window's behalf
/// for work nobody is looking at. A machine may have several of them
/// installed, they answer to different accounts, and every turn costs the
/// person money, so which one is a choice rather than a guess. Absent and
/// unchosen are the same state deliberately: a machine that has never been
/// asked and one that was asked and never answered are the same to everything
/// downstream.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Preference {
    /// The `registry` id of the agent chosen for this installation's own work,
    /// or `None` while nobody has chosen.
    agent: Option<String>,
}

/// The whole of one decision, as the settings window reads it: which agent
/// this installation works through.
///
/// The whole of one decision, as the settings window reads it: which agent
/// this installation works through.
///
/// One answer rather than several commands: the list of agents this build can
/// raise and the one chosen, so a window assembles its state out of a single
/// reply rather than three that could disagree.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoiceStatus {
    /// Every agent this build can raise, the unchoosable ones carrying their
    /// reason rather than being filtered out — see [`candidates`].
    pub cloud: Vec<Candidate>,
    /// Which of them is chosen, or `None` while nobody has picked. A stored id
    /// the list no longer offers reads as `None`, so a choice this build
    /// stopped offering is not drawn as current.
    pub cloud_chosen: Option<String>,
}

/// The whole decision, read off the file the choice is kept in.
///
/// # Errors
///
/// Never. A settings section that refused to draw would leave somebody unable
/// to change the thing that failed — including changing it back.
#[tauri::command(async)]
pub async fn model_choice_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ModelChoiceStatus, String> {
    Ok(reported(preference(&app)).await)
}

/// Write the decision down, and answer with what the section should now show.
///
/// Every member travels together because the window holds them together: the
/// card, the provider under each card, the address and the model. Both
/// providers are stored whichever card is showing, so switching between them
/// costs nobody the answer they gave the other one.
///
/// Write the decision down, and answer with what the section should now show.
///
/// The agent named travels alone: which one this installation works through is
/// the whole of the choice, and the model it reaches is the agent's own —
/// configured in its own file, never here.
///
/// # Errors
///
/// When an agent named is not one this build offers, and when the file cannot
/// be written.
#[tauri::command(async)]
pub async fn model_choice_set<R: Runtime>(
    app: AppHandle<R>,
    cloud_agent: Option<String>,
) -> Result<ModelChoiceStatus, ProjectError> {
    offered(cloud_agent.as_deref(), &candidates())?;

    let preference = Preference { agent: cloud_agent };
    let path = configuration_file(&app, FILE)?;
    write_configuration(&path, &preference)?;
    Ok(reported(preference).await)
}

/// An agent is stored only if this build would raise it.
///
/// Refused rather than written: a preference naming an agent that cannot run is
/// one that fails later, somewhere nobody is watching.
///
/// # Errors
///
/// [`ProjectError`] when the id is not on this build's list, and when it is
/// there carrying a reason it cannot be chosen.
fn offered(agent: Option<&str>, list: &[Candidate]) -> Result<(), ProjectError> {
    let Some(agent) = agent else {
        return Ok(());
    };
    let candidate = list
        .iter()
        .find(|candidate| candidate.id == agent)
        .ok_or_else(|| {
            ProjectError::new(
                "unknown_agent",
                format!("`{agent}` is not an agent Sync knows how to raise here."),
            )
        })?;
    match &candidate.refusal {
        None => Ok(()),
        Some(refusal) => Err(ProjectError::new(
            "agent_unavailable",
            format!("{} cannot be worked through. {refusal}", candidate.name),
        )),
    }
}

/// One preference, drawn.
async fn reported(preference: Preference) -> ModelChoiceStatus {
    let cloud = candidates();
    ModelChoiceStatus {
        cloud_chosen: still_offered(preference.agent.clone(), &cloud),
        cloud,
    }
}

/// A stored agent is reported as chosen only while this build still offers it.
///
/// The list an agent is chosen from moves between releases: a row can stop
/// being verified, and one card's list is narrower than the other's. Reporting
/// a stored id the list no longer offers would draw a chosen provider with
/// nothing marked beside it.
fn still_offered(chosen: Option<String>, list: &[Candidate]) -> Option<String> {
    chosen.filter(|id| {
        list.iter()
            .any(|candidate| candidate.eligible && &candidate.id == id)
    })
}

/// One agent, as the settings window reads it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The `registry` id, and what a choice stores.
    pub id: String,
    /// What a person reads.
    pub name: String,
    /// Whether this one can be chosen at all.
    pub eligible: bool,
    /// The file its servers would be read from, when there is one.
    ///
    /// Shown rather than kept private because the row below it in the same
    /// section shows the same thing, and because it is the honest answer to
    /// *where does this list come from* — a person who has servers in a file
    /// Sync does not name can see immediately that they picked the wrong agent.
    pub configuration: Option<String>,
    /// Why it cannot be chosen, in a sentence, or `None` when it can.
    pub refusal: Option<String>,
}

/// Every agent, with the reason beside the ones that cannot be chosen.
///
/// The unchoosable are listed rather than filtered out. A person who installed
/// an agent and cannot find it in this list learns nothing from its absence;
/// the sentence beside it is what tells them whether to install something else
/// or to stop looking.
fn candidates() -> Vec<Candidate> {
    registry::ALL
        .iter()
        .map(|spec| {
            let configuration = crate::connect::configuration_shown(spec.id);
            let refusal = refusal(spec.is_verified(), configuration.is_some());
            Candidate {
                id: spec.id.to_owned(),
                name: spec.display_name.trim_matches('`').to_owned(),
                eligible: refusal.is_none(),
                configuration,
                refusal,
            }
        })
        .collect()
}

/// The sentence a person reads instead of a choice.
///
/// Written for somebody deciding what to do next rather than for somebody
/// diagnosing this build: neither sentence names a protocol method or a version
/// of anything, because neither is a thing the reader can act on.
fn refusal(verified: bool, has_configuration: bool) -> Option<String> {
    if !verified {
        return Some("It has never finished a turn through Sync on this build.".to_owned());
    }
    if !has_configuration {
        return Some(
            "Sync cannot tell which tools it has: it does not know where this one keeps them."
                .to_owned(),
        );
    }
    None
}

/// Read the choice, or the state of never having made one.
///
/// A file that cannot be read is the same as no file. This is a preference,
/// not a record: refusing to open the settings window because a JSON file has
/// a stray comma in it would be the window holding a person's own machine
/// against them, and the next choice they make writes it clean.
fn preference<R: Runtime>(app: &AppHandle<R>) -> Preference {
    configuration_file(app, FILE)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The agent this installation works through.
///
/// One function because everything downstream — the servers a screen lists,
/// the turn a package's ask is carried by — asks the same question: *who does
/// Sync raise for its own work*.
pub(crate) fn working_agent<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let preference = preference(app);
    still_offered(preference.agent, &candidates())
}

/// The servers the chosen agent has, as its own configuration lists them.
///
/// The choice is read through [`status`] rather than straight out of the file,
/// so an agent this build stopped offering answers the same way an unmade
/// choice does. Anything else would read a list out of a program that cannot be
/// asked to use it.
///
/// # Errors
///
/// When no agent is chosen, and when the chosen one's configuration cannot be
/// read — including its not being there. A list is the one thing this must not
/// invent: an empty one is an answer about the person rather than about the
/// file.
#[tauri::command(async)]
pub fn flagship_servers<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<crate::connect::ServerRow>, ProjectError> {
    let chosen = working_agent(&app).ok_or_else(|| {
        ProjectError::new(
            "no_flagship",
            "No agent is chosen to work through yet.".to_owned(),
        )
    })?;
    crate::connect::servers_of(&app, &chosen)
}

/// What a screen asks the flagship's tools for.
///
/// Three members and nothing about how the asking is done: an MCP server, a
/// tool it publishes, and the arguments that tool takes.
///
/// The shape is the durable part of this. What carries the ask today is a turn
/// of the chosen agent — the only way of reaching those tools that needs no key
/// of anybody's and opens no second connection — and it costs a turn's worth of
/// tokens and latency every time. That price is worth paying and it is not
/// worth being locked into: a request spelled in the protocol underneath, with
/// a session and a prompt and a stop reason in it, would make replacing that
/// carrier a change to every caller that ever asked. This one is spelled in the
/// vocabulary of the far end, which is the same whatever gets it there.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    /// The MCP server, as the flagship's own configuration keys it.
    pub server: String,
    /// The tool, as that server publishes it. Not the spelling any particular
    /// agent uses for it — that is rendered further down, from this.
    pub tool: String,
    /// The arguments, passed through untouched. What a tool takes is the tool's
    /// business, and a copy of its schema kept here would be an older one.
    ///
    /// A caller that says nothing means a tool that takes nothing, which MCP
    /// spells `{}`. Left as JSON `null` it would reach the agent as the word
    /// *null* written where its arguments should be, and what a model does with
    /// that is anybody's guess.
    #[serde(default = "no_arguments")]
    pub arguments: serde_json::Value,
}

/// What a tool that takes nothing is called with.
fn no_arguments() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Call one tool of one of the flagship's servers, and answer with what the
/// tool returned.
///
/// # Where this turn does *not* appear
///
/// Nowhere a person looks for their conversations. It touches neither the
/// registry of live sessions nor the file of resumable ones, so no row joins
/// the list, no pointer is written, and closing the project has nothing of this
/// to end. That is by construction rather than by a filter: a turn that was
/// registered and then hidden would be one screen's rule, and the next screen
/// to read the registry would show it.
///
/// The agent is raised for this call and stopped when it answers. Holding one
/// open between calls would be cheaper and is a decision about cost against
/// staleness that nothing has taken yet.
///
/// # Errors
///
/// [`ProjectError`] when no agent is chosen, when the chosen one has no such
/// server in its configuration, when it cannot be found or started, and when
/// the turn ran without the tool answering — the last of these carries
/// [`errand::Refusal`]'s own name for what happened, because *the agent talked
/// instead of calling it* and *the tool was called and said nothing* send
/// somebody to different places.
#[tauri::command(async)]
pub async fn flagship_call<R: Runtime>(
    app: AppHandle<R>,
    // Where the turn runs. The agent resolves its own configuration against it,
    // and a tool that reads a repository is looking at this one.
    project: String,
    ask: Ask,
) -> Result<serde_json::Value, ProjectError> {
    // Nobody is asking on a package's behalf, so there is no agreement to
    // find: an agreement is something a person made about a package, in a card
    // naming that package and those tools, and the window is not a package and
    // has no card. A turn started here answers a permission exactly as it did
    // before agreements existed — which is to refuse it and say so.
    asked(&app, None, project, ask).await
}

/// The same call, made by a package rather than by the window.
///
/// One door with two ways in rather than two doors: what the far end is asked,
/// how the ask is carried and every one of the seven refusals are the same
/// question whoever is asking, and a second copy of them would answer it
/// differently within a release.
///
/// What the second way adds is the only thing that differs — **who is asking**.
/// The id is resolved against what is installed on this machine and its own
/// manifest is read there, so a package that did not ask for the capability is
/// refused before an agent is raised. It cannot be passed by whoever called:
/// the window builds this door per package with the id closed over, which is
/// the same shape the network and the keychain doors have and for the same
/// reason.
///
/// The refusal is a [`ProjectError`] rather than a string, unlike the vault's
/// and the network's, because everything else this door can answer with is one:
/// a panel that had to tell `no_flagship` from a sentence by reading the
/// sentence would be parsing prose to decide what to draw.
///
/// # Two permissions, and they are asked of different people
///
/// The manifest is the first: *this build spends turns of your agent*, which a
/// card says before anything is installed and which is the same for everybody
/// who installs it. The agreement in [`crate::consent`] is the second: *and it
/// may spend one on this server*, which is one person's decision about their
/// own account and is taken in the card that installs the package.
///
/// The second is read before an agent is raised rather than left to the turn to
/// discover, and that is not only about cost: an agent configured to run its
/// own tools without asking never puts the question at all, so a check that
/// lived only in the answer to that question would be no check whatever on the
/// machines where it is easiest to lose.
///
/// It is read after the questions about this installation, though — see
/// [`asked`] — because *no agent is chosen* is the larger fact, and a person
/// sent to agree to a server would find nothing to agree with until they had
/// answered that one.
///
/// # Errors
///
/// Everything [`flagship_call`] returns, `extension_refused` for a package this
/// machine does not serve or whose manifest did not ask for the capability, and
/// `tool_permission_needed` for a server nobody has agreed this package may
/// call.
/// The last of those is spelled by [`errand::Refusal`] rather than written out,
/// because a screen branching on it must not have to tell an agreement that was
/// never given from one the agent refused to act on: to whoever asked, they are
/// one situation and one thing to do about it.
#[tauri::command(async)]
pub async fn extension_tool_call<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    project: String,
    ask: Ask,
) -> Result<serde_json::Value, ProjectError> {
    // Reading the artefact off the disk, which is why it goes to the blocking
    // pool — the same road `extension_fetch` and `terminal_open` take to the
    // same function.
    let checking = app.clone();
    let asking = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::extensions::permitted(&checking, &asking, TOOLS_CAPABILITY)
    })
    .await
    .map_err(|error| ProjectError::new("extension_refused", error.to_string()))?
    .map_err(|refusal| ProjectError::new("extension_refused", refusal))?;

    asked(&app, Some(&id), project, ask).await
}

/// One tool of one of the flagship's servers, asked and answered.
///
/// Not a command. It is the whole of what both commands above do once the
/// question of who may ask has been settled, and it is a function rather than a
/// third command so that settling that question cannot be skipped by calling
/// the wrong one.
async fn asked<R: Runtime>(
    app: &AppHandle<R>,
    // Which package this is for, where it is for one. `None` is the window
    // itself asking, which nobody agreed to on any card and which therefore
    // gives a turn nothing to answer a permission with.
    asking: Option<&str>,
    // Where the turn runs. The agent resolves its own configuration against it,
    // and a tool that reads a repository is looking at this one.
    project: String,
    ask: Ask,
) -> Result<serde_json::Value, ProjectError> {
    let app = app.clone();
    let chosen = working_agent(&app).ok_or_else(|| {
        ProjectError::new(
            "no_flagship",
            "No agent is chosen to work through yet.".to_owned(),
        )
    })?;

    // Read out of the file rather than found out by asking: a server the
    // flagship does not have is a turn that would cost a launch and come back
    // saying the tool was never called, which reads as the agent misbehaving.
    let servers = crate::connect::servers_of(&app, &chosen)?;
    if !servers.iter().any(|server| server.name == ask.server) {
        return Err(ProjectError::new(
            "unknown_server",
            format!("The chosen agent has no server called `{}`.", ask.server),
        ));
    }

    // What the person agreed to, read here and not cached, so that withdrawing
    // an agreement takes effect on the next call rather than on the next
    // launch. Refused before the agent is raised: an agent that runs its own
    // tools without asking would never put the question, and an agreement
    // enforced only inside the answer to a question would be enforced only
    // where it was already going to be.
    let consent = match asking {
        None => Consent::Withheld,
        Some(id) if crate::consent::allows(&app, id, &ask.server) => Consent::Given,
        Some(id) => {
            return Err(ProjectError::new(
                // Spelled by the refusal rather than written out here. To
                // whoever asked, *nobody ever agreed to this* and *the agent
                // would not run it without somebody agreeing* are one
                // situation with one thing to do about it, and a panel that
                // had to tell them apart would be branching on where the
                // sentence came from.
                errand::Refusal::PermissionNeeded.code(),
                format!("Nobody has agreed that \"{id}\" may call `{}`.", ask.server),
            ));
        }
    };

    let cwd = std::path::PathBuf::from(project);
    let (spec, options) = raised(&app, &chosen, &cwd)?;

    let mut errand =
        errand::Errand::new(cwd, McpToolName::new(ask.server, ask.tool), ask.arguments);
    errand.consent = consent;
    errand::run(spec, &options, &errand).await.map_err(|error| {
        match error {
            errand::ErrandError::Refused(refusal) => {
                ProjectError::new(refusal.code(), refusal.to_string())
            }
            // The agent's own sentence when it gave one, because a refusal from
            // the program is worth more than this side's account of a pipe.
            errand::ErrandError::Connection(error) => ProjectError::new(
                "agent_turn",
                error.detail().unwrap_or_else(|| error.to_string()),
            ),
            // The enum is `#[non_exhaustive]`: a way of failing this build has
            // not read about is reported as one, never mapped to a name that
            // would send somebody after the wrong thing.
            other => ProjectError::new("agent_turn", other.to_string()),
        }
    })
}

/// What it takes to start the chosen agent in one directory.
///
/// Its own function rather than lines inside the errand below, because every
/// one of them is a fact about *this installation* rather than about the turn:
/// which row raises that program, where the program is, and what
/// [`crate::sessions::launched`] already decided about a launch of it. An
/// unwatched turn that answered any of those for itself would be a second
/// answer, drifting quietly while both kept compiling.
///
/// # Errors
///
/// [`ProjectError`] when the chosen id is not a row this build can raise, when
/// the program is not on the machine, and whatever
/// [`crate::sessions::launched`] refuses for.
fn raised<R: Runtime>(
    app: &AppHandle<R>,
    chosen: &str,
    cwd: &std::path::Path,
) -> Result<(&'static acp_client::AgentLaunchSpec, launch::SpawnOptions), ProjectError> {
    let spec = crate::sessions::catalog::spec(chosen)
        .ok_or_else(|| ProjectError::new("agent_unknown", format!("no agent called {chosen}")))?;
    let program = crate::sessions::catalog::resolve(spec.program).ok_or_else(|| {
        ProjectError::new(
            "agent_missing",
            format!("`{}` was not found on this machine", spec.program),
        )
    })?;
    let options = crate::sessions::launched(app, spec, program, None, cwd)?;
    Ok((spec, options))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool that takes nothing is asked with nothing, and `{}` is what MCP
    /// spells that. The window omits the member entirely in that case, so what
    /// this pins is the shape the far end is handed when it does.
    #[test]
    fn an_ask_with_no_arguments_is_an_ask_with_empty_ones() {
        let ask: Ask = serde_json::from_value(serde_json::json!({
            "server": "sync",
            "tool": "sync_status",
        }))
        .expect("an ask may leave its arguments out");
        assert_eq!(ask.arguments, serde_json::json!({}));
    }

    /// A file written before any field but `agent` existed still reads, and a
    /// file carrying fields this version no longer keeps reads just the same:
    /// they are ignored rather than refused, so a preference left behind by an
    /// older installation never blocks the settings window.
    #[test]
    fn an_older_file_still_reads() {
        let stored: Preference =
            serde_json::from_str(r#"{"agent":"opencode"}"#).expect("an older file still reads");
        assert_eq!(stored.agent.as_deref(), Some("opencode"));

        let with_legacy_fields: Preference = serde_json::from_str(
            r#"{"agent":"opencode","modelSource":"localServer","modelServer":"http://127.0.0.1:1234/v1","model":"a-model"}"#,
        )
        .expect("unknown fields are ignored, not refused");
        assert_eq!(with_legacy_fields.agent.as_deref(), Some("opencode"));
    }

    #[test]
    fn an_agent_that_never_finished_a_turn_is_not_offered() {
        let refusal = refusal(false, true).expect("an unverified agent is refused");
        assert!(refusal.contains("finished a turn"), "{refusal}");
    }

    #[test]
    fn an_agent_whose_servers_cannot_be_read_is_not_offered() {
        let refusal = refusal(true, false).expect("an agent with no known file is refused");
        assert!(refusal.contains("does not know where"), "{refusal}");
    }

    #[test]
    fn an_agent_proven_and_readable_is_offered() {
        assert!(refusal(true, true).is_none());
    }

    /// The pairing in `connect` is what makes anything choosable at all, and it
    /// is two catalogues agreeing about a program neither of them owns. A
    /// rename on either side leaves both files compiling and this list empty.
    #[test]
    fn at_least_one_agent_can_be_chosen() {
        let offered = candidates();
        assert!(
            offered.iter().any(|candidate| candidate.eligible),
            "no agent is eligible: {offered:?}"
        );
    }

    #[test]
    fn an_eligible_agent_says_which_file_its_tools_come_from() {
        for candidate in candidates().iter().filter(|candidate| candidate.eligible) {
            assert!(
                candidate.configuration.is_some(),
                "{} is offered without a file",
                candidate.name
            );
        }
    }

    #[test]
    fn a_choice_this_build_no_longer_offers_is_reported_as_none() {
        assert_eq!(
            still_offered(Some("an-agent-that-left".to_owned()), &candidates()),
            None
        );
    }

    #[test]
    fn a_choice_this_build_offers_is_reported_back() {
        let chosen = candidates()
            .into_iter()
            .find(|candidate| candidate.eligible)
            .expect("a build with no eligible agent is caught by another test");
        assert_eq!(
            still_offered(Some(chosen.id.clone()), &candidates()),
            Some(chosen.id)
        );
    }

    /// The display name of a row may be spelled for a document rather than for
    /// a window, and what reaches a person has to read as the product's name.
    #[test]
    fn no_name_reaches_the_window_in_backticks() {
        for candidate in candidates() {
            assert!(!candidate.name.contains('`'), "{}", candidate.name);
        }
    }
}
