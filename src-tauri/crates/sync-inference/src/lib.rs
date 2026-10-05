//! Local model inference: the runtimes this machine can load, and the shape a
//! model's manifest takes.
//!
//! Tauri-free like `sync-voice` and `sync-vault` beside it: this crate knows
//! nothing about a window, a project or a command. It takes a manifest and an
//! input, and answers with an output. What may ask for one, and on whose
//! behalf, is decided in `src-tauri/src/models.rs`, where the installation is
//! known — the same division `sync-handlers` and `handlers.rs` make, and for
//! the same reason: a crate that cannot see the application cannot widen its
//! own reach.
//!
//! # Why the surface is pass-through
//!
//! A model's request and response shape is the model's, not the shell's, and it
//! moves between versions of the model without the shell's surface moving with
//! it. So [`InferenceRuntime::run`] takes a task string and an opaque JSON
//! value, and answers an opaque JSON value: the task is stable (it is what the
//! shell dispatches on), the shape is declared in the manifest (it is what the
//! shell validates against), and neither is encoded in the trait's signature.
//! This is the `ToolAsk` precedent — `arguments: unknown`, because the schema
//! is the tool's — applied to a model.
//!
//! # Why no runtime is linked here yet
//!
//! The trait is the contract; the implementations that link `llama.cpp`, `mlx`
//! or `onnx` arrive when the first real model does. [`Stub`] is here so the
//! whole pipeline — manifest, store, dispatch, surface — can be proven and
//! tested before a heavy native dependency is taken. A runtime that links
//! nothing is the lazy first rung: the spine compiles and answers, and the
//! muscle is added without the spine changing.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use serde::{Deserialize, Serialize};

pub mod capability;
pub mod ipc;

/// One model's manifest: what a download carries, and what the store reads.
///
/// The shape a model is installed by, and the whole of what the shell needs to
/// decide whether it can run here. It is not a `.syncext` archive manifest and
/// is read by a different path — a model is downloaded as bytes, not unpacked
/// from a signed archive, and its integrity is the sha256 of the bytes rather
/// than a signature over the archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    /// The model's permanent id. What a project would name, what `refs/` keys.
    pub id: String,
    /// A semver version, for the catalogue and for update checks.
    pub version: String,
    /// What a person reads.
    pub name: String,
    /// What kind of input the model takes. Decides which task vocabulary it may
    /// serve — a `text` model serves `text.*`, an `audio` model serves
    /// `audio.*`.
    pub modality: Modality,
    /// Which runtime loads it. The shell picks the implementation from this.
    pub runtime: Runtime,
    /// The tasks this model answers, spelled as the shell's vocabulary does:
    /// `text.classify`, `audio.tts`. The shell dispatches on the first
    /// installed model that declares the task asked for.
    pub purposes: Vec<String>,
    /// What kind of model this is — a higher-level category than the individual
    /// `purposes`. A `router` picks tools or actions; a `classifier` returns
    /// typed decisions; a `completer` generates text. The catalogue groups by
    /// this, and a handler may ask for a category rather than a task.
    #[serde(default)]
    pub category: Option<String>,
    /// The size of the weights in bytes. Shown before a download, and used by
    /// the hardware filter's memory check.
    pub size: u64,
    /// A floor on the machine's free memory, in bytes. Zero means unspecified,
    /// and the filter does not refuse on it.
    #[serde(default)]
    pub min_memory: u64,
    /// What hardware the model runs on. The filter reads this against
    /// [`capability::Capabilities`] and answers a refusal rather than a silent
    /// hide, so a person may override.
    #[serde(default)]
    pub requires: Requires,
    /// Where the bytes are and how to verify them.
    pub weights: Weights,
    /// The model's own inference binary, when it ships one. A GGUF model
    /// leaves this absent — the shell's `llama-cpp-2` loads it. A model like
    /// Needle 3 ships its own executable per platform, and this is where the
    /// one for this build is fetched from.
    #[serde(default)]
    pub engine: Option<Engine>,
    /// Who published it, for the catalogue card. Free text.
    #[serde(default)]
    pub source: Option<String>,
    /// The licence, named for the catalogue card. Not enforced here.
    #[serde(default)]
    pub license: Option<String>,
    /// Per-task input and output JSON Schemas. Keyed by task string. The shell
    /// validates an ask against the input schema and returns the output whole;
    /// a task with no entry is served without validation, which is the model's
    /// to be honest about.
    #[serde(default)]
    pub schemas: std::collections::BTreeMap<String, TaskSchema>,
}

/// What a model takes in and gives back for one task.
///
/// Two schemas rather than one, because the two are read at different moments:
/// the input is checked before the model is asked, and the output is returned
/// whole. Neither is compiled into the shell's surface — they live in the
/// manifest, so they move with the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSchema {
    /// A JSON Schema for the task's input, or the unit value if the task takes
    /// nothing the shell should check.
    #[serde(default = "empty_object")]
    pub input: serde_json::Value,
    /// A JSON Schema for the task's output. Returned to a caller that asks
    /// `model.serving`, so an author can write against the shape a model
    /// declares without reading the manifest file.
    #[serde(default = "empty_object")]
    pub output: serde_json::Value,
}

/// What `{}` looks like as a default, so a missing schema is an empty object
/// rather than `null` — which would be a different answer to a caller reading
/// it back.
fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// The kind of input a model works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Audio,
    Vision,
}

/// Which native runtime loads the weights.
///
/// The four the catalogue curates for. A model declares one, and the shell
/// raises the matching [`InferenceRuntime`] implementation. `mlx` is
/// Apple-silicon only; `needle` is a model that ships its own binary; the
/// others are cross-platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Gguf,
    Mlx,
    Onnx,
    /// A model that ships its own inference binary — Needle 3, for example,
    /// loads a `.cact` file through a `needle` executable per platform. The
    /// binary is downloaded alongside the weights; the server spawns it
    /// rather than linking a library.
    Needle,
    /// A Python-based model — Laya, for example, loads through `torch` +
    /// `transformers`. The server spawns a Python script as a persistent
    /// subprocess and forwards requests to it. Requires Python on the
    /// machine.
    Laya,
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gguf => "gguf",
            Self::Mlx => "mlx",
            Self::Onnx => "onnx",
            Self::Needle => "needle",
            Self::Laya => "laya",
        }
        .fmt(f)
    }
}

/// What a model needs from the machine it runs on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default = "Requires::any")]
pub struct Requires {
    /// CPU architectures the runtime builds for. Empty means any.
    pub arch: Vec<String>,
    /// `"any"`, `"metal"`, `"cuda"` or `"none"`. `any` is the default and the
    /// loosest; `none` is a model that runs on the CPU and needs no GPU.
    pub gpu: String,
}

impl Requires {
    /// The loosest requirement: any architecture, any GPU.
    fn any() -> Self {
        Self {
            arch: Vec::new(),
            gpu: "any".to_owned(),
        }
    }
}

impl Default for Requires {
    fn default() -> Self {
        Self::any()
    }
}

/// Where the weights are fetched from, and how they are verified.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Weights {
    /// The URL the bytes are downloaded from.
    pub url: String,
    /// The sha256 of the bytes, hex. The integrity anchor — there is no
    /// signature over a model, because the hash is the whole of what a
    /// content-addressed store needs.
    pub sha256: String,
    /// The file format, when it is not implied by [`Runtime`]. A `gguf` model
    /// is a single `.gguf`; an `onnx` model may be a directory, and `mlx` is a
    /// directory. Left to the runtime when absent.
    #[serde(default)]
    pub format: Option<String>,
}

/// A model's own inference binary, downloaded alongside the weights.
///
/// A `needle` model ships a per-platform executable that loads its `.cact`
/// file; the server spawns it rather than linking a library. The platform is
/// matched at install time — a manifest for macOS ARM64 carries the
/// `macos-arm64` binary, and a build for another platform would carry a
/// different one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    /// The URL the binary is downloaded from.
    pub url: String,
    /// The sha256 of the binary, hex.
    pub sha256: String,
    /// The platform this binary is for: `macos-arm64`, `linux-x86_64`, etc.
    /// The install process checks this against the build's platform.
    pub platform: String,
}

/// Why a runtime did not answer.
///
/// A refusal rather than a panic: a model that fails its call is something a
/// handler can catch and degrade from, and a string the handler's author can
/// act on reaches JavaScript as the message of an `Error`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct InferenceError {
    /// A short code a caller can branch on: `no_model`, `bad_input`,
    /// `runtime_failed`.
    pub code: String,
    /// A sentence a person or an author can act on.
    pub message: String,
}

impl InferenceError {
    /// One error, in the shape the rest of the crate builds them.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

/// What every runtime implements.
///
/// One method, deliberately: the task and the input cross whole, and the
/// output comes back whole. A method per task (`classify`, `complete`, `noul`)
/// would encode the model's shape in the trait, which is exactly what the
/// pass-through design refuses to do — the shape moves with the model, and the
/// trait is what stays still.
pub trait InferenceRuntime: Send + Sync {
    /// Answer one task. The shell has already validated `input` against the
    /// manifest's schema for `task` when one was declared.
    ///
    /// # Errors
    ///
    /// [`InferenceError`] when the model cannot answer: a load that failed, an
    /// input it could not read, or a runtime that misbehaved. The code reaches
    /// the caller; the message reaches the handler.
    fn run(
        &self,
        task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError>;
}

/// A runtime that answers without a model, for testing the pipeline.
///
/// Returns the input back under a `stub` key, so a caller can see the whole
/// path — manifest, store, dispatch, surface — answered end to end without a
/// native dependency linked. The day a real `gguf` implementation arrives, this
/// stays for the tests that do not need one.
#[derive(Debug, Default)]
pub struct Stub;

impl InferenceRuntime for Stub {
    fn run(
        &self,
        task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({
            "stub": true,
            "task": task,
            "echo": input,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest round-trips through JSON, which is the whole of what the
    /// store and the catalogue do with one. A field that fails to cross is a
    /// field a model cannot declare.
    #[test]
    fn a_manifest_round_trips() {
        let manifest = ModelManifest {
            id: "needle-3".to_owned(),
            version: "1.0.0".to_owned(),
            name: "Needle 3".to_owned(),
            modality: Modality::Text,
            runtime: Runtime::Gguf,
            purposes: vec!["text.classify".to_owned()],
            category: None,
            size: 35_000_000,
            min_memory: 0,
            requires: Requires::default(),
            weights: Weights {
                url: "https://example.invalid/needle-3.gguf".to_owned(),
                sha256: "abc".to_owned(),
                format: Some("gguf".to_owned()),
            },
            engine: None,
            source: Some("cactus".to_owned()),
            license: None,
            schemas: std::collections::BTreeMap::new(),
        };
        let json = serde_json::to_string(&manifest).expect("serialises");
        let back: ModelManifest =
            serde_json::from_str(&json).expect("deserialises to the same shape");
        assert_eq!(manifest, back);
    }

    /// A manifest written without the optional fields still reads, because a
    /// model published before they existed must not be refused by a newer
    /// build.
    #[test]
    fn a_minimal_manifest_reads() {
        let json = r#"{
            "id": "x", "version": "0", "name": "X",
            "modality": "text", "runtime": "gguf",
            "purposes": ["text.classify"], "size": 1,
            "weights": { "url": "u", "sha256": "h" }
        }"#;
        let manifest: ModelManifest = serde_json::from_str(json).expect("optional fields absent");
        assert!(manifest.requires.arch.is_empty());
        assert_eq!(manifest.requires.gpu, "any");
        assert!(manifest.schemas.is_empty());
    }

    /// The stub echoes, which is all a test of the pipeline needs: the dispatch
    /// reached a runtime and the runtime answered.
    #[test]
    fn the_stub_answers() {
        let answer = Stub
            .run("text.classify", &serde_json::json!({ "state": "hi" }))
            .expect("the stub never fails");
        assert_eq!(answer["task"], "text.classify");
        assert_eq!(answer["echo"]["state"], "hi");
    }
}
