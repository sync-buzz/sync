//! What this machine can say, and what it says it with.
//!
//! An engine turns text into sound. There are two kinds in the design and one
//! here: the system's synthesiser, which is the platform's and needs nothing
//! downloaded, and — later — a model on this disk (`docs/voice.md` §3.2). They
//! meet at [`Engine`], and the crate answers [`engines`] with what this build
//! and this platform can actually offer, so a caller never has to ask what
//! operating system it is on.
//!
//! # Tauri-free, like its neighbours
//!
//! Nothing here knows about a window, a project or a command. It takes a
//! sentence and says it. What may ask for one, and on whose behalf, is decided
//! in `src-tauri/src/voice.rs`, where the application is known — the same
//! division `sync-handlers` and `handlers.rs` make, and for the same reason: a
//! crate that cannot see the application cannot widen its own reach.
//!
//! # Why an engine owns a thread
//!
//! `speakUtterance` queues a sentence and returns immediately — the speaking
//! happens afterwards, in the system. So the synthesiser has to outlive the
//! call that asked for the sentence, or the speech stops half way through when
//! the object is released. A `static` would not do it either: the object is not
//! `Send`, and the callers are three different threads (the window's, the
//! clock's, and a handler's `spawn_blocking` one).
//!
//! So the engine is a thread that owns the synthesiser for the life of the
//! process, and every caller reaches it through a channel. The Objective-C
//! object is created on that thread and never leaves it, which is what makes
//! this safe without asserting anything about `AVFoundation`'s own thread
//! safety.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod system;

/// The engine every platform Sync ships on has, and the only one today.
///
/// A name rather than an enum variant, because the second engine is a model
/// somebody downloaded and models arrive after the build does. What identifies
/// an engine is a string for the same reason a `kind` is: the set grows without
/// this file changing.
pub const SYSTEM: &str = "system";

/// One voice, as a person picking one needs to see it.
///
/// `id` is the platform's own identifier and is what gets stored; `name` and
/// `language` are what a person reads. Both are kept because neither answers
/// the other's question — `com.apple.voice.compact.ru-RU.Milena` is not a name
/// anybody chooses from, and "Milena" is not something a preference can be
/// written down as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// A BCP-47 tag — `ru-RU`, `en-GB`. The window groups by it.
    pub language: String,
    pub quality: Quality,
}

/// How good a voice sounds, as the platform grades it.
///
/// Carried because it is the difference a person hears and cannot see: macOS
/// ships a compact voice for every language and downloads a far better one on
/// request, and both appear in the list under the same name. A list that did
/// not say which is which would look like a duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Standard,
    Enhanced,
    Premium,
}

/// One request to say one piece of text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Utterance {
    pub text: String,
    /// Which voice, or the caller has no opinion and the person's choice is
    /// used. A package should not have to know what is installed on this Mac.
    #[serde(default)]
    pub voice: Option<String>,
    /// A multiplier over the platform's normal speaking rate, where `1.0` is
    /// normal. Clamped rather than refused: a rate outside the range is a
    /// caller being enthusiastic, not a caller being wrong.
    #[serde(default = "normal_rate")]
    pub rate: f32,
    /// What a second sentence means while one is still being said: `false`
    /// waits its turn, `true` clears the queue and stops what is speaking.
    #[serde(default)]
    pub interrupt: bool,
}

fn normal_rate() -> f32 {
    1.0
}

/// The slowest and fastest this build will ask an engine for.
///
/// The bounds are the window's as much as the engine's: a control that offers a
/// rate nothing can distinguish from silence-per-word is a control that wastes
/// its own range.
pub const SLOWEST: f32 = 0.5;
pub const FASTEST: f32 = 2.0;

impl Utterance {
    /// A sentence in the person's chosen voice, at their chosen rate.
    #[must_use]
    pub fn saying(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            voice: None,
            rate: normal_rate(),
            interrupt: false,
        }
    }

    #[must_use]
    pub fn in_voice(mut self, voice: Option<String>) -> Self {
        self.voice = voice;
        self
    }

    #[must_use]
    pub fn at_rate(mut self, rate: f32) -> Self {
        self.rate = rate.clamp(SLOWEST, FASTEST);
        self
    }

    #[must_use]
    pub fn interrupting(mut self, interrupt: bool) -> Self {
        self.interrupt = interrupt;
        self
    }
}

/// What an engine is, from the outside.
///
/// Three questions and no state: what voices are there, say this, stop. Where
/// an engine keeps a thread, a model or a device is its own business — which is
/// what lets the model engine of `docs/voice.md` §3.2 arrive as another
/// implementation rather than as a second shape.
pub trait Engine: Send + Sync {
    /// Which engine this is, as a preference writes it down.
    fn id(&self) -> &'static str;

    /// Every voice it can speak in, in no particular order.
    ///
    /// # Errors
    ///
    /// When the engine cannot be reached at all.
    fn voices(&self) -> Result<Vec<Voice>, VoiceError>;

    /// Queue a sentence, and answer as soon as it is queued.
    ///
    /// It deliberately does not wait for the speaking to finish: a sentence
    /// takes seconds, and every caller here — a command, a handler, the clock —
    /// is something that has to answer in milliseconds.
    ///
    /// # Errors
    ///
    /// When there is nothing to say, when the named voice is not on this
    /// machine, or when the engine cannot be reached.
    fn speak(&self, utterance: &Utterance) -> Result<(), VoiceError>;

    /// Stop what is being said and drop what is waiting.
    fn stop(&self);
}

/// An engine this build knows about, and whether it is here.
///
/// Both halves are needed by the same screen: a build that lists only what is
/// present cannot explain an absence, and *why* an engine is missing is the one
/// thing somebody can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub id: String,
    /// What the section calls it.
    pub label: String,
    /// Why it cannot be used here, or `None` when it can.
    pub absent: Option<String>,
}

/// What could speak on this machine, present or not.
#[must_use]
pub fn engines() -> Vec<Available> {
    vec![Available {
        id: SYSTEM.to_owned(),
        label: "System".to_owned(),
        absent: system_absence(),
    }]
}

#[cfg(target_os = "macos")]
fn system_absence() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
fn system_absence() -> Option<String> {
    Some("Sync speaks through the system's own synthesiser, and this platform has none it can reach.".to_owned())
}

/// The engine a name refers to, if this machine has it.
///
/// # Errors
///
/// When no engine goes by that name here — which covers both a name from a
/// newer build and one from a platform this is not.
pub fn engine(id: &str) -> Result<&'static dyn Engine, VoiceError> {
    #[cfg(target_os = "macos")]
    if id == SYSTEM {
        return Ok(system::engine());
    }

    Err(VoiceError::NoEngine(id.to_owned()))
}

/// What goes wrong, in the words the person who hears it needs.
///
/// Speech that quietly does not happen is indistinguishable from speech nobody
/// heard, so every failure here is a sentence rather than a silence — including
/// the ones only a developer will read.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("there is no voice engine called `{0}` on this machine")]
    NoEngine(String),

    #[error("there is nothing to say")]
    Nothing,

    #[error(
        "the voice `{0}` is not on this machine — it may have been removed, or it may not have finished downloading"
    )]
    UnknownVoice(String),

    #[error("the voice engine stopped answering")]
    Gone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_outside_the_range_is_brought_into_it() {
        assert!((Utterance::saying("x").at_rate(9.0).rate - FASTEST).abs() < f32::EPSILON);
        assert!((Utterance::saying("x").at_rate(0.0).rate - SLOWEST).abs() < f32::EPSILON);
    }

    #[test]
    fn an_utterance_defaults_to_the_person_s_own_choice() {
        let utterance = Utterance::saying("Hello");
        assert_eq!(utterance.voice, None, "no voice means the person's own");
        assert!(!utterance.interrupt, "a sentence waits its turn by default");
    }

    /// A name from a newer build, or from another platform, is refused in words
    /// rather than by falling back to whatever this build happens to have.
    #[test]
    fn an_engine_nobody_has_is_refused_by_name() {
        let Err(refusal) = engine("kokoro") else {
            panic!("this build has no model engine, and answered as though it had one");
        };
        assert!(
            refusal.to_string().contains("kokoro"),
            "the refusal names what was asked for: {refusal}"
        );
    }
}

/// What a person decided about the voice this machine speaks in.
///
/// # Why this is the crate's and not the application's
///
/// It was the application's for exactly as long as the window was the only
/// thing that spoke. It is here because there are now two readers — the window
/// and the MCP sidecar an agent is connected to — and two readings of one file
/// is how two of them come to disagree about which voice was chosen.
///
/// **Read here, written only by the window.** The sidecar never writes it: a
/// preference is something a person set on a page, and a process with no window
/// has nobody to have set it. That asymmetry is what keeps the file free of
/// locking — one writer, many readers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preference {
    /// Which engine says it.
    pub engine: String,
    /// Which voice, or none chosen and the engine speaks in its own default.
    #[serde(default)]
    pub voice: Option<String>,
    /// A multiplier over normal speech, where `1.0` is normal.
    #[serde(default = "normal_rate")]
    pub rate: f32,
    /// Whether an agent connected to Sync may speak.
    ///
    /// **Off until somebody says otherwise, and it is the one switch here that
    /// starts off.** Everything else on the page describes *how* Sync speaks
    /// when something asks it to; this one decides whether a language model
    /// gets to make the machine talk. A package that could speak was installed
    /// from a card that said so — that card is the consent, by the rule
    /// `docs/background.md` §4.1 settled for the clock. An agent was connected
    /// from a settings page that said nothing of the kind, so there is no
    /// earlier moment where this was agreed to, and it has to be agreed to
    /// here.
    #[serde(default)]
    pub agents: bool,
}

impl Default for Preference {
    fn default() -> Self {
        Self {
            engine: SYSTEM.to_owned(),
            voice: None,
            rate: normal_rate(),
            agents: false,
        }
    }
}

/// What the preference is written in, inside this installation's configuration
/// directory — beside `mcp-server.json` and `recent-projects.json`.
pub const FILE: &str = "voice.json";

/// Read the preference out of a configuration directory.
///
/// An unreadable or absent file is the default rather than a failure. What is
/// lost is three choices; refusing to speak because a file was hand-edited
/// would be a worse answer than speaking in the ordinary voice — and the fourth
/// choice, [`Preference::agents`], fails *closed*, which is the direction that
/// matters.
#[must_use]
pub fn preference(directory: &std::path::Path) -> Preference {
    std::fs::read_to_string(directory.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Say something in the voice a preference names.
///
/// The one place an utterance is built from a preference, so that no caller can
/// speak in a voice the person did not choose or at a rate they did not set.
///
/// # Errors
///
/// When the engine is not on this machine, when the chosen voice is not, or
/// when there is nothing to say.
pub fn say(preference: &Preference, text: &str, interrupt: bool) -> Result<(), VoiceError> {
    let engine = engine(&preference.engine)?;
    let utterance = Utterance::saying(text)
        .in_voice(preference.voice.clone())
        .at_rate(preference.rate)
        .interrupting(interrupt);
    engine.speak(&utterance)
}
