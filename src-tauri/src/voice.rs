//! What Sync says out loud, and in whose voice.
//!
//! `sync-voice` knows how to say a sentence and what was chosen to say it with;
//! this is where it is decided *who was allowed to ask*. The division is the
//! one `handlers.rs` makes with `sync-handlers`: the crate is Tauri-free and
//! cannot widen its own reach, and everything about this installation — the
//! commands the window calls, the directory the preference is read from — is
//! here.
//!
//! # Why this is the application's and not a package's
//!
//! `docs/voice.md` §1, in one line: a service module runs in an isolate with no
//! audio device, no filesystem and no thread that outlives the call, so nothing
//! about speaking can live in a package. What a package may do is *ask*, which
//! is a capability, and what a person decides is the settings window's.
//!
//! # The preference is the installation's, and this is not its only reader
//!
//! `voice.json` sits beside `mcp-server.json` and `recent-projects.json`, and
//! for the same reason all three are there rather than in a repository: a voice
//! belongs to these speakers. A colleague who clones the project has a
//! different set of voices installed, so a choice that travelled would name one
//! they do not have.
//!
//! It is not in the window's own storage either, where the appearance and the
//! typography live. Those are applied before the first frame a window paints
//! and are useless to anything else; this one has to be readable when there is
//! no window at all — and it *is* read elsewhere: `sync-mcp` reads the same
//! file to know whether an agent may speak. **This process is its only
//! writer**, which is what keeps one file free of locking.

use std::path::PathBuf;

use serde::Serialize;
use sync_voice::{Available, Engine, Preference, Voice};
use tauri::{AppHandle, Manager, Runtime};

use crate::project::{configuration_file, write_configuration};

/// Everything the settings page draws, in one answer.
///
/// One command rather than three, because the three are read together and
/// nothing on the page means anything without the others: a chosen voice with
/// no list to find it in is an identifier, and a list with no choice beside it
/// cannot show what is selected. `server_status` is the same shape for the same
/// reason.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    /// Every engine this build knows, present or not.
    pub engines: Vec<Available>,
    /// The voices of the *chosen* engine, or empty when it has none here.
    pub voices: Vec<Voice>,
    pub settings: Preference,
    /// Why there is nothing to choose from, when there is nothing.
    ///
    /// Beside the empty list rather than instead of it: a page that drew an
    /// error where its controls should be would leave somebody with no way to
    /// pick a different engine.
    pub failure: Option<String>,
}

/// Where this installation keeps what a person decided.
fn directory<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|error| format!("could not resolve the configuration directory: {error}"))
}

/// This installation's choice, or the default nobody has changed yet.
pub(crate) fn preference<R: Runtime>(app: &AppHandle<R>) -> Preference {
    directory(app)
        .map(|path| sync_voice::preference(&path))
        .unwrap_or_default()
}

/// What the machine can say, and what it has been told to say it with.
///
/// # Errors
///
/// Never for a missing engine or an empty list — those are `failure` on the
/// answer, so that the page can still offer a different engine.
#[tauri::command]
pub fn voice_status<R: Runtime>(app: AppHandle<R>) -> Result<VoiceStatus, String> {
    Ok(status_of(preference(&app)))
}

/// Write a choice down and answer with what the page should now show.
///
/// The whole preference rather than a field: four controls on one page change
/// one thing each, and a command per control would be four ways for the file to
/// end up describing a state nobody chose.
///
/// # Errors
///
/// When the engine is not on this machine, when the voice is not one that
/// engine has, or when the file cannot be written. Each is refused rather than
/// stored — a preference naming a voice that is not here is a preference that
/// fails later, in the dark, when something tried to speak.
#[tauri::command]
pub fn voice_choose<R: Runtime>(
    app: AppHandle<R>,
    settings: Preference,
) -> Result<VoiceStatus, String> {
    let engine = sync_voice::engine(&settings.engine).map_err(|error| error.to_string())?;

    if let Some(wanted) = &settings.voice {
        let voices = engine.voices().map_err(|error| error.to_string())?;
        if !voices.iter().any(|voice| &voice.id == wanted) {
            return Err(format!(
                "`{wanted}` is not a voice this machine has. It may have been removed in System Settings."
            ));
        }
    }

    let settings = Preference {
        rate: settings
            .rate
            .clamp(sync_voice::SLOWEST, sync_voice::FASTEST),
        ..settings
    };
    let path = configuration_file(&app, sync_voice::FILE).map_err(|error| error.message)?;
    write_configuration(&path, &settings).map_err(|error| error.message)?;
    Ok(status_of(settings))
}

/// Say something, in the voice this machine was told to use.
///
/// # Errors
///
/// When there is no engine, when the chosen voice is gone, or when there is
/// nothing to say.
#[tauri::command]
pub fn voice_speak<R: Runtime>(
    app: AppHandle<R>,
    text: String,
    interrupt: Option<bool>,
) -> Result<(), String> {
    say(&app, &text, interrupt.unwrap_or(false))
}

/// Stop what is being said and drop what is waiting.
///
/// It answers nothing, and cannot fail in a way worth reporting: an engine that
/// is not there is an engine that is not speaking, which is what was asked for.
#[tauri::command]
pub fn voice_stop<R: Runtime>(app: AppHandle<R>) {
    if let Ok(engine) = sync_voice::engine(&preference(&app).engine) {
        engine.stop();
    }
}

/// Say something on this installation's behalf.
///
/// # Errors
///
/// When there is no engine, when the chosen voice is not on this machine, or
/// when there is nothing to say.
pub(crate) fn say<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    interrupt: bool,
) -> Result<(), String> {
    sync_voice::say(&preference(app), text, interrupt).map_err(|error| error.to_string())
}

/// The page's whole answer, built from a preference already in hand.
fn status_of(settings: Preference) -> VoiceStatus {
    let engines = sync_voice::engines();
    match sync_voice::engine(&settings.engine).and_then(Engine::voices) {
        Ok(voices) => VoiceStatus {
            engines,
            voices,
            settings,
            failure: None,
        },
        Err(error) => VoiceStatus {
            engines,
            voices: Vec::new(),
            settings,
            failure: Some(error.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default is what somebody who has never opened the page gets, and it
    /// has to be a state that speaks — except for the one switch that must not.
    #[test]
    fn the_default_speaks_but_not_for_an_agent() {
        let settings = Preference::default();
        assert_eq!(settings.engine, sync_voice::SYSTEM);
        assert_eq!(settings.voice, None, "not choosing is a real answer");
        assert!((settings.rate - 1.0).abs() < f32::EPSILON);
        assert!(
            !settings.agents,
            "an agent has no card to have agreed on, so it starts unable to speak"
        );
    }

    /// A preference file written by an older build, or edited by hand, must not
    /// stop the machine speaking — and must not switch an agent's voice on.
    #[test]
    fn a_preference_that_cannot_be_read_is_the_default() {
        let read = serde_json::from_str::<Preference>("{\"engine\": 7}");
        assert!(read.is_err(), "the file is nonsense");
        assert_eq!(read.unwrap_or_default(), Preference::default());
    }

    /// A file from the build before agents could speak has no `agents` member.
    /// It reads as `false`, which is the direction a missing answer must fail
    /// in: nobody agreed to it, because there was nothing to agree to.
    #[test]
    fn a_preference_from_an_older_build_leaves_agents_silent() {
        let older: Preference = serde_json::from_str(
            "{\"engine\":\"system\",\"voice\":\"com.apple.voice.compact.ru-RU.Milena\",\"rate\":1.2}",
        )
        .expect("an older file is still readable");
        assert!(!older.agents);
        assert_eq!(
            older.voice.as_deref(),
            Some("com.apple.voice.compact.ru-RU.Milena")
        );
    }

    /// The status a page draws when its engine is missing still lists the
    /// engines, or there would be no way to pick a different one.
    #[test]
    fn a_missing_engine_leaves_the_engines_listed() {
        let status = status_of(Preference {
            engine: "kokoro".to_owned(),
            ..Preference::default()
        });
        assert!(status.voices.is_empty());
        assert!(!status.engines.is_empty(), "the choice is still offered");
        assert!(
            status.failure.is_some_and(|said| said.contains("kokoro")),
            "the page can say which engine is missing"
        );
    }
}
