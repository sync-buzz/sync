//! The Needle runtime: spawns the model's own `needle` binary and reads its
//! JSON answer.
//!
//! Needle 3 is not a GGUF model loaded through `llama-cpp-2`. It ships its own
//! inference binary per platform, and the server runs it as a subprocess —
//! `needle --model <path> --prompt <text>` — reading the JSON object it
//! prints to stdout. This is the shape the pass-through surface was designed
//! for: a model with its own API, reached through the same `run` request as a
//! GGUF model, the difference only in how the server executes it.

use std::process::Command;

use sync_inference::InferenceError;

/// One loaded Needle model: the path to the `.cact` file and the `needle`
/// binary that runs it. The binary is the runtime; the `.cact` is the weights.
pub struct NeedleModel {
    /// The path to the `.cact` file, so the server can tell whether a `run`
    /// request names the same model or a different one.
    pub path: String,
    /// The path to the `needle` binary.
    engine: String,
}

impl NeedleModel {
    /// Hold the paths. The files are not opened until [`run`](Self::run) is
    /// called — the binary is spawned per request, and there is no model to
    /// load into memory between calls.
    #[must_use]
    pub fn new(path: String, engine: String) -> Self {
        Self { path, engine }
    }

    /// Run one task: extract the prompt from `input`, spawn the `needle`
    /// binary, and return its JSON answer whole.
    ///
    /// # Errors
    ///
    /// [`InferenceError`] when the input has no prompt, the binary cannot be
    /// started, or it exits without answering.
    pub fn run(
        &self,
        _task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let prompt = input
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| InferenceError::new("bad_input", "the input has no `prompt` field"))?;

        let output = Command::new(&self.engine)
            .arg("--model")
            .arg(&self.path)
            .arg("--prompt")
            .arg(prompt)
            .output()
            .map_err(|e| {
                InferenceError::new("engine_failed", format!("could not run the engine: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InferenceError::new(
                "engine_failed",
                format!("the engine exited {}: {stderr}", output.status),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let answer: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|e| {
            InferenceError::new(
                "bad_output",
                format!("the engine's answer is not JSON: {e}"),
            )
        })?;

        Ok(answer)
    }
}
