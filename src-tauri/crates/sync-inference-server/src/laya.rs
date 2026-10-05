//! The Laya runtime: a persistent Python subprocess that loads the model and
//! answers requests, proxied through the same JSON-RPC the window speaks.
//!
//! Laya is a `PyTorch` model (`torch` + `transformers`), not a native binary.
//! The server spawns `python3` `laya_server.py` as a long-lived subprocess,
//! forwards `run` requests to it over stdio, and returns the answers. The
//! Python process loads the model once and holds it; the server holds the
//! Python process for as long as the model is in use.
//!
//! The script path arrives as `engine_path` in [`RunParams`](sync_inference::ipc::RunParams)
//! — the same field Needle 3 uses for its binary. The window resolves it out of
//! its own resource directory, which is `Contents/Resources` in a bundle and
//! `src-tauri` in a build from source, and passes it through.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sync_inference::InferenceError;
use sync_inference::ipc::{CHANNEL_VERSION, HandshakeParams, Request, Response, RunParams};

/// One loaded Laya model: a persistent Python subprocess and the repo it has
/// loaded. The subprocess stays alive across requests — loading a 421M model
/// takes seconds, and a handler that asks twice should not pay for it twice.
pub struct LayaModel {
    /// The `HuggingFace` repo ID the Python process has loaded, so the server
    /// can tell whether a `run` request names the same model or a different
    /// one.
    pub path: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LayaModel {
    /// Spawn the Python script, perform the handshake, and hold the process.
    ///
    /// # Errors
    ///
    /// When Python cannot be started, the script cannot be found, or the
    /// handshake fails.
    pub fn start(model_path: &str, script_path: &str) -> Result<Self, String> {
        let mut child = Command::new("python3")
            .arg(script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("could not start python3: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or("the python process opened no stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("the python process opened no stdout")?;

        let mut model = Self {
            path: model_path.to_owned(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        model.handshake()?;
        Ok(model)
    }

    /// Run one task: forward the request to the Python process and read the
    /// answer.
    ///
    /// # Errors
    ///
    /// [`InferenceError`] when the pipe breaks or the Python process answers
    /// an error.
    pub fn run(
        &mut self,
        task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = serde_json::to_value(RunParams {
            model_path: self.path.clone(),
            runtime: "laya".to_owned(),
            task: task.to_owned(),
            input: input.clone(),
            engine_path: None,
        })
        .map_err(|e| InferenceError::new("bad_input", e.to_string()))?;

        let response = self
            .send(&Request {
                method: "run".to_owned(),
                params,
            })
            .map_err(|e| InferenceError::new("pipe_failed", e))?;

        if let Some(error) = response.error {
            return Err(InferenceError::new(&error.code, error.message));
        }
        Ok(response.result)
    }

    /// Send the handshake and check the channel version.
    fn handshake(&mut self) -> Result<(), String> {
        let params = serde_json::to_value(HandshakeParams {
            channel: CHANNEL_VERSION,
        })
        .unwrap_or_default();
        let response = self.send(&Request {
            method: "handshake".to_owned(),
            params,
        })?;
        if let Some(error) = response.error {
            return Err(format!("handshake failed: {}", error.message));
        }
        Ok(())
    }

    /// Send one request and read one response.
    fn send(&mut self, request: &Request) -> Result<Response, String> {
        let line = serde_json::to_string(&request).map_err(|e| format!("could not encode: {e}"))?;
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("could not send to the python process: {e}"))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|e| format!("could not send to the python process: {e}"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("could not flush to the python process: {e}"))?;

        let mut answer = String::new();
        self.stdout
            .read_line(&mut answer)
            .map_err(|e| format!("could not read from the python process: {e}"))?;
        if answer.is_empty() {
            return Err("the python process closed without answering".to_owned());
        }
        serde_json::from_str(&answer)
            .map_err(|e| format!("the python process answered un-readable JSON: {e}"))
    }
}

impl Drop for LayaModel {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
