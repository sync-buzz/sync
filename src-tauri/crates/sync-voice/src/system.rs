//! The system's own synthesiser, which on this platform is `AVSpeechSynthesizer`.
//!
//! It is the engine that costs nothing and is here already: macOS ships a voice
//! for every language it supports and downloads its Enhanced and Premium ones
//! through System Settings, without Sync being involved in any of it. The
//! system also *plays* the sound, so there is no audio device to open, no
//! sample format to agree on and no playback library in the dependency tree.
//!
//! # One thread, for the life of the process
//!
//! `speakUtterance` queues and returns; the speaking happens afterwards. So the
//! synthesiser must outlive the call, and a `Retained` is not `Send` — it
//! cannot be handed between the three threads that ask for a sentence (the
//! window's, the clock's, and a handler's blocking one).
//!
//! The answer is a thread that owns it and a channel that reaches the thread.
//! The object is made there and never leaves, so nothing here depends on
//! whether `AVFoundation` is thread-safe: it is only ever touched from one
//! thread. The thread is started once, on first use, and is never stopped —
//! there is nothing to stop it *for*, and a synthesiser torn down between two
//! sentences is a sentence cut in half.

use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};

use objc2::rc::Retained;
use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesisVoiceQuality, AVSpeechSynthesizer,
    AVSpeechUtterance, AVSpeechUtteranceDefaultSpeechRate, AVSpeechUtteranceMaximumSpeechRate,
    AVSpeechUtteranceMinimumSpeechRate,
};
use objc2_foundation::NSString;

use crate::{Engine, Quality, SYSTEM, Utterance, Voice, VoiceError};

/// The engine, made once and shared.
///
/// A `OnceLock` rather than a value the application holds, because what it
/// guards is a thread: two of them would be two synthesisers, and two
/// synthesisers speak over each other.
pub(crate) fn engine() -> &'static dyn Engine {
    static ENGINE: OnceLock<SystemEngine> = OnceLock::new();
    ENGINE.get_or_init(SystemEngine::start)
}

/// What the thread is asked to do.
///
/// Each carries the channel its answer goes back on, except `Stop`, which has
/// no answer worth waiting for: stopping is what a person does when they have
/// heard enough, and reporting on it afterwards helps nobody.
enum Order {
    Voices(Sender<Vec<Voice>>),
    Speak(Utterance, Sender<Result<(), VoiceError>>),
    Stop,
}

struct SystemEngine {
    orders: Sender<Order>,
}

impl SystemEngine {
    fn start() -> Self {
        let (orders, taken) = channel();
        // Named, because a stuck thread in a sample is a line in a report that
        // has to be identifiable without a stack trace.
        let started = std::thread::Builder::new()
            .name("sync-voice".to_owned())
            .spawn(move || serve(&taken));
        if let Err(error) = &started {
            // Nothing here can recover, and the caller will hear it as
            // `VoiceError::Gone` on the first sentence. Said once, where the
            // reason still exists.
            eprintln!("[voice] the speech thread would not start: {error}");
        }
        Self { orders }
    }

    /// Send an order and wait for the answer it carries.
    ///
    /// Both halves fail the same way and mean the same thing: the thread is not
    /// there. `Gone` is that sentence, and it is the only failure this function
    /// invents.
    fn ask<T>(&self, order: impl FnOnce(Sender<T>) -> Order) -> Result<T, VoiceError> {
        let (answer, heard) = channel();
        self.orders
            .send(order(answer))
            .map_err(|_| VoiceError::Gone)?;
        heard.recv().map_err(|_| VoiceError::Gone)
    }
}

impl Engine for SystemEngine {
    fn id(&self) -> &'static str {
        SYSTEM
    }

    fn voices(&self) -> Result<Vec<Voice>, VoiceError> {
        self.ask(Order::Voices)
    }

    fn speak(&self, utterance: &Utterance) -> Result<(), VoiceError> {
        if utterance.text.trim().is_empty() {
            return Err(VoiceError::Nothing);
        }
        self.ask(|answer| Order::Speak(utterance.clone(), answer))?
    }

    fn stop(&self) {
        // A closed channel means the thread is gone, which means nothing is
        // being said, which is what was asked for.
        let _ = self.orders.send(Order::Stop);
    }
}

/// The thread: it owns the synthesiser and answers until nobody can ask again.
///
/// The loop ends when every sender has been dropped, which for a `OnceLock`
/// engine means the process is ending.
fn serve(orders: &Receiver<Order>) {
    // SAFETY: `AVSpeechSynthesizer` is not a main-thread-only class — the
    // bindings would mark it `MainThreadOnly` — and this instance is created
    // here and never leaves this thread.
    let synthesiser = unsafe { AVSpeechSynthesizer::new() };

    while let Ok(order) = orders.recv() {
        match order {
            Order::Voices(answer) => {
                let _ = answer.send(installed_voices());
            }
            Order::Speak(utterance, answer) => {
                let _ = answer.send(say(&synthesiser, &utterance));
            }
            Order::Stop => silence(&synthesiser),
        }
    }
}

/// Every voice this Mac has, including the ones it downloaded.
fn installed_voices() -> Vec<Voice> {
    // SAFETY: a class method that reads the system's voice registry.
    let listed = unsafe { AVSpeechSynthesisVoice::speechVoices() };
    // `to_vec` rather than an iterator: iterating an `NSArray` is behind the
    // `NSEnumerator` feature, and this asks the array for its objects once and
    // is done with it.
    listed
        .to_vec()
        .into_iter()
        .map(|voice| {
            // SAFETY: property reads on a voice the system just handed over.
            unsafe {
                Voice {
                    id: voice.identifier().to_string(),
                    name: voice.name().to_string(),
                    language: voice.language().to_string(),
                    quality: graded(voice.quality()),
                }
            }
        })
        .collect()
}

fn graded(quality: AVSpeechSynthesisVoiceQuality) -> Quality {
    match quality {
        AVSpeechSynthesisVoiceQuality::Premium => Quality::Premium,
        AVSpeechSynthesisVoiceQuality::Enhanced => Quality::Enhanced,
        // The platform's own name for this one is `Default`, which in a list
        // beside Enhanced and Premium reads as "the one you get if you do not
        // choose" rather than as a grade. `Standard` is the same fact said as
        // a grade.
        _ => Quality::Standard,
    }
}

fn say(
    synthesiser: &Retained<AVSpeechSynthesizer>,
    utterance: &Utterance,
) -> Result<(), VoiceError> {
    let voice = match &utterance.voice {
        Some(id) => {
            let named = NSString::from_str(id);
            // SAFETY: a class method over a string that lives across the call.
            // It answers `None` both for a voice that never existed and for one
            // that has not finished downloading, which is why the refusal below
            // says both.
            let found = unsafe { AVSpeechSynthesisVoice::voiceWithIdentifier(&named) };
            Some(found.ok_or_else(|| VoiceError::UnknownVoice(id.clone()))?)
        }
        // No voice named: the system speaks in whatever it is set to, which is
        // the right answer for a caller that has no opinion.
        None => None,
    };

    let text = NSString::from_str(&utterance.text);
    // SAFETY: every call below is a property write or a queue on objects owned
    // by this thread, made before the synthesiser is asked to speak.
    unsafe {
        if utterance.interrupt {
            silence(synthesiser);
        }
        let spoken = AVSpeechUtterance::speechUtteranceWithString(&text);
        spoken.setVoice(voice.as_deref());
        spoken.setRate(rate_of(utterance.rate));
        synthesiser.speakUtterance(&spoken);
    }
    Ok(())
}

/// A multiplier over normal speech, as the platform's own scale.
///
/// The platform's rate is a number between a minimum and a maximum with no
/// meaning of its own — `0.5` is normal on macOS and nothing says so. A
/// multiplier does say so, and it survives the day the second engine has a
/// different scale, because `1.0` means *normal* in any of them.
fn rate_of(multiplier: f32) -> f32 {
    // SAFETY: reading three constants the framework defines.
    unsafe {
        let normal = AVSpeechUtteranceDefaultSpeechRate;
        (normal * multiplier).clamp(
            AVSpeechUtteranceMinimumSpeechRate,
            AVSpeechUtteranceMaximumSpeechRate,
        )
    }
}

fn silence(synthesiser: &Retained<AVSpeechSynthesizer>) {
    // SAFETY: a method on an object owned by this thread. `Immediate` rather
    // than `Word`: this is what somebody presses when they have heard enough,
    // and finishing the current word first is not what "stop" means.
    unsafe {
        synthesiser.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one claim that matters about the system engine: this machine has
    /// voices, and they are described well enough to choose one from.
    #[test]
    fn the_system_has_voices_and_each_is_described() {
        let voices = engine().voices().expect("the system engine answers");
        assert!(
            !voices.is_empty(),
            "a Mac ships with voices; none was listed"
        );
        for voice in &voices {
            assert!(
                !voice.id.is_empty(),
                "a voice with no identifier: {voice:?}"
            );
            assert!(!voice.name.is_empty(), "a voice with no name: {voice:?}");
            assert!(
                !voice.language.is_empty(),
                "a voice with no language: {voice:?}"
            );
        }
    }

    /// A voice that is not here is refused by name rather than replaced with
    /// whatever the system would have used — a caller that asked for Milena and
    /// silently got Alex has no way to find that out.
    #[test]
    fn a_voice_that_is_not_here_is_refused_by_name() {
        let asked = Utterance::saying("Тишина").in_voice(Some("com.example.nobody".to_owned()));
        let refusal = engine().speak(&asked).expect_err("no such voice");
        assert!(
            matches!(refusal, VoiceError::UnknownVoice(ref id) if id == "com.example.nobody"),
            "the refusal names the voice: {refusal}"
        );
    }

    #[test]
    fn there_is_nothing_to_say_is_a_refusal_rather_than_a_silence() {
        let refusal = engine()
            .speak(&Utterance::saying("   "))
            .expect_err("whitespace is not a sentence");
        assert!(matches!(refusal, VoiceError::Nothing), "{refusal}");
    }

    /// Normal is the platform's own default, and the ends of the range map to
    /// the ends of the platform's.
    #[test]
    fn normal_is_the_platform_s_normal() {
        // SAFETY: reading a constant the framework defines.
        let normal = unsafe { AVSpeechUtteranceDefaultSpeechRate };
        assert!((rate_of(1.0) - normal).abs() < f32::EPSILON);
        assert!(rate_of(crate::FASTEST) > normal);
        assert!(rate_of(crate::SLOWEST) < normal);
    }

    /// Run with `cargo test -p sync-voice -- --ignored --nocapture` and listen.
    /// It is ignored because a test suite that talks is a test suite nobody
    /// runs twice — and because there is no assertion a machine can make about
    /// a sound coming out of a speaker.
    #[test]
    #[ignore = "speaks out loud"]
    fn it_says_something() {
        let voices = engine().voices().expect("voices");
        let russian = voices.iter().find(|voice| voice.language.starts_with("ru"));
        let said = Utterance::saying("Синхронизация закончена. Голос работает.")
            .in_voice(russian.map(|voice| voice.id.clone()));
        engine().speak(&said).expect("it speaks");
        // The sentence is queued, not spoken, so the thread has to be given
        // time before the process ends underneath it.
        std::thread::sleep(std::time::Duration::from_secs(4));
    }
}
