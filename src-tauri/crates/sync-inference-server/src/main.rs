//! The inference sidecar: a process that loads a local model and answers
//! tasks, isolated from the window.
//!
//! The window spawns this binary, sends JSON-RPC requests over stdin, and
//! reads responses from stdout — the same pattern the memory sidecar
//! (`sync-mcp`) uses, for the same reason: a crash in native inference is a
//! reconnect rather than a lost window.
//!
//! The server holds one loaded model at a time. A `run` request that names a
//! different model path swaps the loaded one. This is the simplest arrangement
//! that serves a handler's call, and the one a machine with one auxiliary
//! model installed needs; loading several at once is a later concern.

use std::io::{self, BufRead, Write};

use sync_inference::ipc::{
    CHANNEL_VERSION, HandshakeParams, HandshakeResult, Request, Response, RunParams,
};

mod gguf;
mod laya;
mod needle;
mod onnx;

/// The server's version, sent in the handshake.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    if let Err(error) = run() {
        eprintln!("fatal: {error}");
        std::process::exit(1);
    }
}

/// One loaded model, whichever runtime it uses.
enum LoadedModel {
    Gguf(gguf::GgufModel),
    Needle(needle::NeedleModel),
    Laya(laya::LayaModel),
    Onnx(Box<onnx::OnnxModel>),
}

impl LoadedModel {
    fn path(&self) -> &str {
        match self {
            Self::Gguf(m) => &m.path,
            Self::Needle(m) => &m.path,
            Self::Laya(m) => &m.path,
            Self::Onnx(m) => &m.path,
        }
    }

    fn run(
        &mut self,
        task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, sync_inference::InferenceError> {
        match self {
            Self::Gguf(m) => m.run(task, input),
            Self::Needle(m) => m.run(task, input),
            Self::Laya(m) => m.run(task, input),
            Self::Onnx(m) => m.run(task, input),
        }
    }
}

/// Read lines from stdin, dispatch each, write the answer to stdout.
fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let mut loaded: Option<LoadedModel> = None;

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("could not read stdin: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let answer = Response::err("bad_request", format!("not readable JSON: {error}"));
                writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&answer).unwrap_or_default()
                )
                .map_err(|e| e.to_string())?;
                stdout.flush().map_err(|e| e.to_string())?;
                continue;
            }
        };
        let answer = dispatch(&request, &mut loaded);
        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&answer).unwrap_or_default()
        )
        .map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Answer one request.
fn dispatch(request: &Request, loaded: &mut Option<LoadedModel>) -> Response {
    match request.method.as_str() {
        "handshake" => handshake(request.params.clone()),
        "run" => run_task(request.params.clone(), loaded),
        other => Response::err(
            "unsupported",
            format!("`{other}` is not a method this server answers"),
        ),
    }
}

/// The handshake: confirm the channel version and describe the server.
fn handshake(params: serde_json::Value) -> Response {
    let params: HandshakeParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(error) => return Response::err("bad_params", error.to_string()),
    };
    if params.channel != CHANNEL_VERSION {
        return Response::err(
            "channel_mismatch",
            format!(
                "the window speaks channel {} and this server speaks {}",
                params.channel, CHANNEL_VERSION
            ),
        );
    }
    Response::ok(
        serde_json::to_value(HandshakeResult {
            channel: CHANNEL_VERSION,
            version: VERSION.to_owned(),
            runtimes: vec![
                "gguf".to_owned(),
                "needle".to_owned(),
                "laya".to_owned(),
                "onnx".to_owned(),
            ],
        })
        .unwrap_or_default(),
    )
}

/// Load a model (if needed) and run one task.
fn run_task(params: serde_json::Value, loaded: &mut Option<LoadedModel>) -> Response {
    let params: RunParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(error) => return Response::err("bad_params", error.to_string()),
    };

    // Swap the loaded model if the path changed. A model that is already
    // loaded is reused — loading is the expensive part for GGUF, and a
    // handler that asks the same model twice should not pay for it twice.
    let needs_load = loaded
        .as_ref()
        .is_none_or(|m| m.path() != params.model_path);
    if needs_load {
        *loaded = match params.runtime.as_str() {
            "gguf" => match gguf::GgufModel::load(&params.model_path) {
                Ok(model) => Some(LoadedModel::Gguf(model)),
                Err(error) => return Response::err("load_failed", error),
            },
            "needle" => {
                let Some(engine_path) = &params.engine_path else {
                    return Response::err("load_failed", "a needle model needs an engine path");
                };
                Some(LoadedModel::Needle(needle::NeedleModel::new(
                    params.model_path.clone(),
                    engine_path.clone(),
                )))
            }
            "laya" => {
                let Some(script_path) = &params.engine_path else {
                    return Response::err("load_failed", "a laya model needs a script path");
                };
                match laya::LayaModel::start(&params.model_path, script_path) {
                    Ok(model) => Some(LoadedModel::Laya(model)),
                    Err(error) => return Response::err("load_failed", error),
                }
            }
            "onnx" => match onnx::OnnxModel::load(&params.model_path) {
                Ok(model) => Some(LoadedModel::Onnx(Box::new(model))),
                Err(error) => return Response::err("load_failed", error),
            },
            other => {
                return Response::err(
                    "unsupported",
                    format!("runtime `{other}` is not supported by this server"),
                );
            }
        };
    }

    let Some(model) = loaded.as_mut() else {
        return Response::err("load_failed", "the model was not loaded");
    };

    match model.run(&params.task, &params.input) {
        Ok(value) => Response::ok(value),
        Err(error) => Response::err(&error.code, error.message),
    }
}
