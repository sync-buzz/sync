//! Typed errors for the ACP client.
//!
//! The agent on the other end is an external program we do not control: it can
//! close mid-frame, answer with a JSON-RPC error object, or send a payload that
//! does not match the protocol at all. Every one of those is a value here, not
//! a panic — nothing in this crate unwraps agent-supplied data.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 error object, as it travels on the wire.
///
/// It is both what an agent may answer us with and what we answer an agent
/// with when a handler declines, so it is a first-class value rather than a
/// formatted string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    /// JSON-RPC error code. Codes in `-32768..=-32000` are protocol-level;
    /// everything else is agent-defined.
    pub code: i64,
    /// Short human-readable description.
    pub message: String,
    /// Agent-defined payload. Kept verbatim — agents put diagnostics here and
    /// we must not lose them on the way to a log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl RpcError {
    /// JSON-RPC's "method not found". What we answer when the agent calls a
    /// client method we have no handler for.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// JSON-RPC's "invalid params".
    pub const INVALID_PARAMS: i64 = -32602;
    /// JSON-RPC's "internal error". What a handler's failure becomes.
    pub const INTERNAL_ERROR: i64 = -32603;

    /// Builds an error with no `data` payload.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Builds a `-32601 method not found` for `method`.
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(Self::METHOD_NOT_FOUND, format!("no handler: {method}"))
    }

    /// Builds a `-32602 invalid params` carrying `detail`.
    #[must_use]
    pub fn invalid_params(detail: impl fmt::Display) -> Self {
        Self::new(Self::INVALID_PARAMS, detail.to_string())
    }

    /// Builds a `-32603 internal error` carrying `detail`.
    #[must_use]
    pub fn internal(detail: impl fmt::Display) -> Self {
        Self::new(Self::INTERNAL_ERROR, detail.to_string())
    }

    /// Attaches an agent-defined `data` payload.
    #[must_use]
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    /// The most specific sentence a PERSON can act on, dug out of `data`.
    ///
    /// [`Self::message`] is the JSON-RPC layer's own word for what happened, and
    /// it is routinely as useless as `"Internal error"`: what actually went wrong
    /// is in `data`, which is why that field is kept verbatim. Agents nest it
    /// differently and none of them promise a shape, so this digs rather than
    /// destructures — Codex answers a refused turn with `data.message` holding a
    /// whole JSON document AS A STRING, the only readable line two levels inside
    /// it:
    ///
    /// ```text
    /// {"message":"{\"type\":\"error\",\"status\":400,\"error\":{
    ///    \"type\":\"invalid_request_error\",
    ///    \"message\":\"The 'gpt-5.6-sol' model requires a newer version of Codex.\"}}",
    ///  "codex_error_info":"other"}
    /// ```
    ///
    /// `None` when `data` is absent or holds nothing readable — never a guess,
    /// and never `message` restated, so a caller can tell "the agent said more"
    /// from "the agent said only that".
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        let found = readable_line(self.data.as_ref()?, DETAIL_DEPTH)?;
        (found != self.message).then_some(found)
    }
}

/// How far [`readable_line`] descends before giving up.
///
/// The deepest real payload seen is three levels (`data` → its `message` string
/// → the document that string holds → that document's `error.message`); the
/// budget is what stops a self-referential or absurdly nested one, not a claim
/// about any agent's shape.
const DETAIL_DEPTH: u8 = 6;

/// Where agents put the sentence, in the order worth trying. `message` first
/// because it is the one every JSON-RPC-shaped payload has; the rest are the
/// wrappers seen around it.
const DETAIL_KEYS: [&str; 4] = ["message", "error", "detail", "description"];

/// The innermost human-readable line in an agent-supplied payload.
///
/// A string that PARSES as JSON is treated as a payload rather than as the
/// answer — that is exactly the case above, where stopping at the first string
/// would hand back a JSON document to show a person. A string that parses back
/// to a bare string is not that: it is a sentence that happens to look like a
/// literal, so it is taken as it stands and the descent ends.
fn readable_line(value: &serde_json::Value, depth: u8) -> Option<String> {
    if depth == 0 {
        return None;
    }
    match value {
        serde_json::Value::String(text) => {
            if let Ok(nested) = serde_json::from_str::<serde_json::Value>(text) {
                if !nested.is_string() {
                    if let Some(deeper) = readable_line(&nested, depth - 1) {
                        return Some(deeper);
                    }
                }
            }
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        // Every key is tried, not just the first one present: a payload carrying
        // an empty `message` beside a real `error` must not come back empty.
        serde_json::Value::Object(fields) => DETAIL_KEYS
            .iter()
            .filter_map(|key| fields.get(*key))
            .find_map(|nested| readable_line(nested, depth - 1)),
        _ => None,
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            // The code stays in front of the detail: this is what a log line and
            // an error chain are built from, and the detail is a sentence that
            // may run for a paragraph.
            Some(detail) => write!(f, "{} (code {}): {detail}", self.message, self.code),
            None => write!(f, "{} (code {})", self.message, self.code),
        }
    }
}

impl std::error::Error for RpcError {}

/// Everything that can go wrong on the client side of an ACP connection.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The agent answered the request with a JSON-RPC error object.
    #[error("agent refused {method}: {source}")]
    Rpc {
        /// The method we called.
        method: &'static str,
        /// The error object the agent sent back.
        #[source]
        source: RpcError,
    },

    /// The connection is gone: the agent's stdout reached EOF, the process
    /// died, or the connection was shut down while the request was in flight.
    /// A request that was waiting for an answer will never get one.
    #[error("connection to the agent is closed")]
    Closed,

    /// The agent took a control request and did not answer it inside the
    /// deadline. Distinct from [`Error::Closed`] on purpose: a process that is
    /// up and silent is a different report to the user — and a different thing
    /// to do about it — than one that died.
    ///
    /// The connection is given up on when this is raised, so every later
    /// request on it fails with this same variant rather than waiting again.
    #[error("agent did not answer {method} within {timeout:?}")]
    Timeout {
        /// The method we called.
        method: &'static str,
        /// The deadline it overran.
        timeout: Duration,
    },

    /// The agent's answer did not match the shape the method promises.
    /// Carries what came back so a log can show it verbatim.
    #[error("agent answered {method} with a payload this client cannot read: {source}")]
    MalformedResponse {
        /// The method we called.
        method: &'static str,
        /// The payload as it arrived.
        payload: serde_json::Value,
        /// Why it did not deserialize.
        #[source]
        source: serde_json::Error,
    },

    /// Serialising our own request failed. Ours to fix, never the agent's.
    #[error("could not encode {method}: {source}")]
    Encode {
        /// The method we were building.
        method: &'static str,
        /// The serde failure.
        #[source]
        source: serde_json::Error,
    },

    /// Raising the agent process failed before a single frame was exchanged.
    #[error("could not launch agent process `{program}`: {source}")]
    Spawn {
        /// The program we tried to run, as given to the OS.
        program: String,
        /// The OS error.
        #[source]
        source: std::io::Error,
    },

    /// Writing to the agent's stdin failed.
    #[error("write to the agent failed: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// What the AGENT said went wrong, in words fit to put in front of a person.
    ///
    /// Only a refusal has one: every other variant here is this side's own
    /// report about a pipe, a deadline or a payload, and its `Display` is
    /// already the whole of what is known. Callers showing a failure to somebody
    /// take this when it is there and fall back to `to_string()` when it is not
    /// — the difference is a sentence from the agent versus a sentence about the
    /// connection, and the first is worth showing alone.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Rpc { source, .. } => source.detail(),
            _ => None,
        }
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload that started this, byte for byte off the
    /// wire: the sentence is inside a JSON document that is itself the STRING
    /// value of `data.message`, and nothing shallower is worth showing.
    #[test]
    fn a_refusal_is_read_down_to_the_sentence_a_person_can_act_on() {
        let refusal = RpcError::internal("Internal error").with_data(serde_json::json!({
            "message": "{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-5.6-sol' model requires a newer version of Codex. Please upgrade to the latest app or CLI and try again.\"}}",
            "codex_error_info": "other",
        }));

        assert_eq!(
            refusal.detail().as_deref(),
            Some(
                "The 'gpt-5.6-sol' model requires a newer version of Codex. \
                 Please upgrade to the latest app or CLI and try again."
            ),
        );
        assert_eq!(
            refusal.to_string(),
            "Internal error (code -32603): The 'gpt-5.6-sol' model requires a newer version \
             of Codex. Please upgrade to the latest app or CLI and try again.",
        );
    }

    /// The plain shapes an agent may answer with, each read to the same place.
    #[test]
    fn a_detail_is_found_wherever_the_agent_put_it() {
        let cases = [
            (serde_json::json!("model is gone"), "model is gone"),
            (
                serde_json::json!({ "message": "model is gone" }),
                "model is gone",
            ),
            (
                serde_json::json!({ "error": { "message": "model is gone" } }),
                "model is gone",
            ),
            (
                serde_json::json!({ "detail": "  model is gone  " }),
                "model is gone",
            ),
            // An empty `message` beside a real one further in must not win by
            // being first in the key order.
            (
                serde_json::json!({ "message": "", "error": "model is gone" }),
                "model is gone",
            ),
        ];

        for (data, expected) in cases {
            let refused = RpcError::internal("Internal error").with_data(data.clone());
            assert_eq!(
                refused.detail().as_deref(),
                Some(expected),
                "payload {data} should read as {expected}",
            );
        }
    }

    /// Absent, empty, or nothing but a restatement of `message` — all "the agent
    /// said only that", and the text stays exactly what it has always been.
    #[test]
    fn nothing_readable_leaves_the_old_text_untouched() {
        let bare = RpcError::new(RpcError::METHOD_NOT_FOUND, "no handler: session/prompt");
        assert_eq!(bare.detail(), None);
        assert_eq!(bare.to_string(), "no handler: session/prompt (code -32601)",);

        for data in [
            serde_json::json!(null),
            serde_json::json!({ "codex_error_info": "other" }),
            serde_json::json!({ "message": "   " }),
            serde_json::json!({ "message": "Internal error" }),
        ] {
            let refused = RpcError::internal("Internal error").with_data(data.clone());
            assert_eq!(
                refused.detail(),
                None,
                "payload {data} should read as nothing"
            );
            assert_eq!(refused.to_string(), "Internal error (code -32603)");
        }
    }

    /// A sentence that happens to parse as a JSON literal is a sentence, not a
    /// payload to descend into — otherwise `"42"` would come back as `42` and a
    /// quoted line would lose its quotes.
    #[test]
    fn a_sentence_that_looks_like_a_literal_is_left_alone() {
        let refused =
            RpcError::internal("Internal error").with_data(serde_json::json!({ "message": "42" }));
        assert_eq!(refused.detail().as_deref(), Some("42"));

        let quoted = RpcError::internal("Internal error")
            .with_data(serde_json::json!({ "message": "\"model is gone\"" }));
        assert_eq!(quoted.detail().as_deref(), Some("\"model is gone\""));
    }

    /// Only a refusal carries the agent's own words; this side's reports about
    /// the connection do not, so a caller can tell the two apart.
    #[test]
    fn only_a_refusal_carries_a_detail() {
        let refused = Error::Rpc {
            method: "session/prompt",
            source: RpcError::internal("Internal error")
                .with_data(serde_json::json!({ "message": "model is gone" })),
        };
        assert_eq!(refused.detail().as_deref(), Some("model is gone"));

        assert_eq!(Error::Closed.detail(), None);
        assert_eq!(
            Error::Timeout {
                method: "session/prompt",
                timeout: Duration::from_secs(1),
            }
            .detail(),
            None,
        );
    }
}
