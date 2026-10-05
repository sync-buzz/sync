//! The protocol the inference sidecar speaks over stdio.
//!
//! Line-delimited JSON-RPC, the same shape the memory sidecar speaks: one
//! request per line on stdin, one response per line on stdout. The window
//! spawns `sync-inference-server`, sends [`Request`]s, reads [`Response`]s,
//! and reconnects when the process dies — the same lifecycle the memory
//! sidecar has, for the same reason: a crash in native inference is a
//! reconnect rather than a lost window.
//!
//! The types live here — in the library, not the server binary — so both
//! halves of the channel compile against one spelling of them. A drift between
//! the two would be a protocol that succeeds in the test and fails in the
//! window.

use serde::{Deserialize, Serialize};

/// The protocol's channel version, sent in the handshake and checked on
/// connect. Bumped when the wire format changes incompatibly.
pub const CHANNEL_VERSION: u32 = 1;

/// One line the window sends to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The method: `"handshake"` or `"run"`.
    pub method: String,
    /// The method's parameters, or an empty object.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// One line the server sends back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// `null` on success, or `{"code": "...", "message": "..."}` on failure.
    #[serde(default)]
    pub error: Option<ResponseError>,
    /// The answer, or `null` on failure.
    #[serde(default)]
    pub result: serde_json::Value,
}

/// Why a call failed, in the shape the window reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseError {
    pub code: String,
    pub message: String,
}

impl Response {
    /// A successful answer.
    #[must_use]
    pub fn ok(value: serde_json::Value) -> Self {
        Self {
            error: None,
            result: value,
        }
    }

    /// A failure.
    #[must_use]
    pub fn err(code: &str, message: impl Into<String>) -> Self {
        Self {
            error: Some(ResponseError {
                code: code.to_owned(),
                message: message.into(),
            }),
            result: serde_json::Value::Null,
        }
    }
}

/// The handshake the window sends on connect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    pub channel: u32,
}

/// The handshake the server answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResult {
    pub channel: u32,
    /// The server binary's version.
    pub version: String,
    /// Which runtimes this server can load: `["gguf"]`, or `["gguf", "mlx"]`.
    pub runtimes: Vec<String>,
}

/// A `run` request: load a model (if not loaded) and answer one task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunParams {
    /// The path to the model's weights file on disk. The server loads this on
    /// first use and holds it; a different path swaps the loaded model.
    pub model_path: String,
    /// The runtime to load with: `"gguf"`, `"mlx"`, `"onnx"`, `"needle"`.
    pub runtime: String,
    /// The task to run, as the model's manifest declares it.
    pub task: String,
    /// The opaque input, validated by the server against the model's schema
    /// when one was declared.
    #[serde(default)]
    pub input: serde_json::Value,
    /// The path to the model's own engine binary, when the runtime is
    /// `needle`. Absent for `gguf`/`mlx`/`onnx`, where the server links the
    /// runtime itself.
    #[serde(default)]
    pub engine_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A response round-trips through JSON, which is the whole of the wire
    /// format. A field that fails to cross is a protocol that drifts.
    #[test]
    fn a_response_round_trips() {
        let response = Response::ok(serde_json::json!({"choice": 0}));
        let json = serde_json::to_string(&response).expect("serialises");
        let back: Response = serde_json::from_str(&json).expect("deserialises");
        assert!(back.error.is_none());
        assert_eq!(back.result["choice"], 0);
    }

    /// An error response round-trips with its code intact, because the code is
    /// what the caller branches on.
    #[test]
    fn an_error_response_round_trips() {
        let response = Response::err("no_model", "nothing installed");
        let json = serde_json::to_string(&response).expect("serialises");
        let back: Response = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back.error.as_ref().expect("present").code, "no_model");
        assert!(back.result.is_null());
    }

    /// A `run` request round-trips with its input opaque, which is the
    /// pass-through contract: the server does not interpret the input's shape.
    #[test]
    fn a_run_request_round_trips() {
        let params = RunParams {
            model_path: "/models/abc/weights".to_owned(),
            runtime: "gguf".to_owned(),
            task: "text.classify".to_owned(),
            input: serde_json::json!({"state": "hi", "choices": ["a", "b"]}),
            engine_path: None,
        };
        let json = serde_json::to_string(&params).expect("serialises");
        let back: RunParams = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back.task, "text.classify");
        assert_eq!(back.input["choices"][1], "b");
    }
}
