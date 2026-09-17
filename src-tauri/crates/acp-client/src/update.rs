//! Tolerant decoding of the `session/update` notification.
//!
//! The protocol types model `sessionUpdate` as a tagged enum, so an agent that
//! invents a variant — or ships one from a newer protocol revision than the
//! types we compile against — would fail the whole notification. That is not
//! acceptable here: the live spike measured four agents that already disagree
//! about which variants they emit at all (`usage_update` never arrives from
//! Grok; `session_info_update` arrives only from Grok and Claude), and the
//! divergence is going to widen, not narrow.
//!
//! So decoding never fails. A payload the compiled types cannot read comes
//! through as [`SessionUpdatePayload::Unrecognized`] with the raw JSON intact,
//! and the connection keeps running.
//!
//! One thing is read back *out* of a decoded update here rather than by each
//! consumer separately: [`SessionUpdatePayload::tool_call`]. A tool's own
//! answer arrives spread across two of the protocol's variants, and two
//! consumers reassembling it apart is how the two copies come to disagree
//! about the same call.

use agent_client_protocol_schema::v1 as schema;
use serde::Deserialize;

/// One `session/update` notification, decoded as far as it can be.
#[derive(Debug, Clone)]
pub struct SessionUpdateEvent {
    /// The session the update belongs to.
    pub session_id: schema::SessionId,
    /// The update itself — typed when the compiled protocol types could read
    /// it, raw when they could not.
    pub payload: SessionUpdatePayload,
}

/// The update body of a [`SessionUpdateEvent`].
///
/// Deliberately closed: there are two states and there will only ever be two —
/// the compiled types read the payload, or they did not. A consumer that
/// handles both has handled everything, and should not have to write a
/// wildcard arm that could silently swallow a third case later.
#[derive(Debug, Clone)]
pub enum SessionUpdatePayload {
    /// The update deserialized into the protocol types.
    Known(Box<schema::SessionUpdate>),
    /// The update did not. Nothing is lost: the raw JSON is carried through so
    /// a consumer can log it, surface it, or grow support for it later.
    Unrecognized(UnrecognizedUpdate),
}

impl SessionUpdatePayload {
    /// The tool call this update reports, when it reports one.
    ///
    /// The protocol describes one call at two moments — `tool_call` announces
    /// it, `tool_call_update` amends it — and anything following a call to its
    /// result has to read both. One report over the pair is what keeps "the
    /// call finished, and here is what it returned" from being written twice,
    /// differently, by the second consumer that needs it.
    ///
    /// Returns `None` for every other variant, and for a payload the compiled
    /// types could not read at all. The second case is not the same as "no
    /// tool call was made": an [`UnrecognizedUpdate`] whose `session_update`
    /// says `tool_call` is a call whose result went past us, and a consumer
    /// that treats it as absence reports the wrong thing about the agent.
    #[must_use]
    pub fn tool_call(&self) -> Option<ToolCallReport<'_>> {
        let Self::Known(update) = self else {
            return None;
        };
        match &**update {
            schema::SessionUpdate::ToolCall(call) => Some(ToolCallReport {
                tool_call_id: &call.tool_call_id,
                title: Some(call.title.as_str()),
                // Present even when the frame omitted it: the protocol gives
                // an announcement a default status, and `pending` is it.
                status: Some(call.status),
                raw_output: call.raw_output.as_ref(),
            }),
            schema::SessionUpdate::ToolCallUpdate(update) => Some(ToolCallReport {
                tool_call_id: &update.tool_call_id,
                title: update.fields.title.as_deref(),
                status: update.fields.status,
                raw_output: update.fields.raw_output.as_ref(),
            }),
            _ => None,
        }
    }
}

/// What one `session/update` says about a tool call.
///
/// A view over the update rather than a copy of it: `raw_output` is arbitrary
/// JSON of a size nobody here chose, and deciding whether a call has finished
/// should not cost a clone of it.
///
/// Every member but the id is optional, because an amendment states only what
/// changed. Absent means *unsaid*, never *empty* — a `status` of `None` leaves
/// the call at whatever it already was, and reading it as `pending` would
/// report a finished call as one still waiting.
#[derive(Debug, Clone, Copy)]
pub struct ToolCallReport<'a> {
    /// Which call this is about. The one member every frame carries, and the
    /// only way to tell two calls in the same turn apart.
    pub tool_call_id: &'a schema::ToolCallId,
    /// The call's human-readable title, when this frame set one.
    ///
    /// Agents differ over where a tool's own name shows up on the wire, so
    /// this is handed through exactly as it arrived rather than interpreted
    /// here; [`crate::McpToolNaming`] is what reads one back into a name.
    pub title: Option<&'a str>,
    /// The call's status, when this frame stated one.
    pub status: Option<schema::ToolCallStatus>,
    /// What the tool itself returned, when this frame carried it.
    ///
    /// Not to be confused with the call's `content`, which is what the agent
    /// chose to *show* for the call and may be its own summary of the answer.
    /// Only this member is the tool's own word, which is why a consumer that
    /// needs the answer takes an absent `raw_output` for a refusal rather than
    /// falling back on the prose around it.
    ///
    /// An explicit JSON `null` arrives here as `None`, the same as an omitted
    /// member: the protocol gives no way to tell a tool that answered `null`
    /// from one that answered nothing at all.
    pub raw_output: Option<&'a serde_json::Value>,
}

/// A `session/update` body the compiled protocol types could not read.
#[derive(Debug, Clone)]
pub struct UnrecognizedUpdate {
    /// The `sessionUpdate` discriminator, when the payload carried a string
    /// one. `None` means the payload was not even shaped like an update.
    pub session_update: Option<String>,
    /// The update body exactly as it arrived.
    pub raw: serde_json::Value,
    /// Why the typed decode failed, for logs.
    pub reason: String,
}

/// The envelope of a `session/update` notification, decoded loosely: the
/// session id is typed (we cannot route without it) and the update body stays
/// raw so the tolerant pass below can own it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSessionNotification {
    session_id: schema::SessionId,
    update: serde_json::Value,
}

/// Decodes `session/update` params.
///
/// # Errors
///
/// Returns the serde failure only when the *envelope* is unusable — no
/// `sessionId`, so there is no session to attribute the update to. The update
/// body itself never fails; see [`SessionUpdatePayload::Unrecognized`].
pub fn decode_session_update(
    params: serde_json::Value,
) -> std::result::Result<SessionUpdateEvent, serde_json::Error> {
    let RawSessionNotification { session_id, update } = serde_json::from_value(params)?;

    let payload = match serde_json::from_value::<schema::SessionUpdate>(update.clone()) {
        Ok(known) => SessionUpdatePayload::Known(Box::new(known)),
        Err(reason) => SessionUpdatePayload::Unrecognized(UnrecognizedUpdate {
            session_update: update
                .get("sessionUpdate")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            raw: update,
            reason: reason.to_string(),
        }),
    };

    Ok(SessionUpdateEvent {
        session_id,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decode(update: &serde_json::Value) -> SessionUpdateEvent {
        decode_session_update(json!({ "sessionId": "s-1", "update": update }))
            .expect("envelope carries a sessionId")
    }

    #[test]
    fn known_variant_decodes_into_the_protocol_types() {
        let event = decode(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "PONG" },
        }));

        let SessionUpdatePayload::Known(update) = event.payload else {
            panic!("agent_message_chunk should decode into the typed variant");
        };
        assert!(matches!(
            *update,
            schema::SessionUpdate::AgentMessageChunk(_)
        ));
    }

    #[test]
    fn unknown_variant_is_carried_raw_instead_of_failing() {
        let event = decode(&json!({
            "sessionUpdate": "quantum_entanglement_update",
            "spookiness": 11,
        }));

        let SessionUpdatePayload::Unrecognized(raw) = event.payload else {
            panic!("an invented variant must not decode into a typed one");
        };
        assert_eq!(
            raw.session_update.as_deref(),
            Some("quantum_entanglement_update")
        );
        assert_eq!(raw.raw["spookiness"], json!(11));
        assert!(
            !raw.reason.is_empty(),
            "the decode failure must be reportable"
        );
    }

    #[test]
    fn unknown_field_on_a_known_variant_does_not_break_the_decode() {
        // Grok hangs its own extensions off `_meta`, and every agent measured
        // is free to add fields we have never seen. A known variant must
        // survive them.
        let event = decode(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "PONG" },
            "somethingNobodyShipsYet": { "nested": true },
            "_meta": { "x.ai/whatever": 1 },
        }));

        assert!(matches!(event.payload, SessionUpdatePayload::Known(_)));
    }

    #[test]
    fn envelope_without_a_session_id_is_an_error_not_a_guess() {
        let err = decode_session_update(json!({
            "update": { "sessionUpdate": "agent_message_chunk" },
        }));
        assert!(err.is_err(), "there is no session to attribute this to");
    }

    #[test]
    fn a_finished_tool_call_hands_over_what_the_tool_returned() {
        let event = decode(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "completed",
            "rawOutput": { "servers": [{ "name": "sync" }] },
        }));

        let report = event.payload.tool_call().expect("this is a tool call");
        assert_eq!(report.tool_call_id.0.as_ref(), "call-1");
        assert_eq!(report.status, Some(schema::ToolCallStatus::Completed));
        assert_eq!(
            report.raw_output,
            Some(&json!({ "servers": [{ "name": "sync" }] })),
        );
    }

    #[test]
    fn a_tool_call_without_a_result_is_still_a_tool_call() {
        // The announcement of a call carries no result yet, and most
        // amendments carry none either. Losing those frames would leave a
        // consumer with no id to follow and no status to wait on — so an
        // absent `rawOutput` is an empty member here, not a missing report
        // and not an unrecognized payload.
        let announced = decode(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-1",
            "title": "sync/sync_status",
            "kind": "fetch",
        }));
        assert!(
            matches!(announced.payload, SessionUpdatePayload::Known(_)),
            "a tool call without a result is a variant the types can read"
        );
        let report = announced.payload.tool_call().expect("this is a tool call");
        assert_eq!(report.title, Some("sync/sync_status"));
        assert_eq!(report.raw_output, None);
        assert_eq!(
            report.status,
            Some(schema::ToolCallStatus::Pending),
            "an announcement with no status is pending by the protocol's default"
        );

        let amended = decode(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "in_progress",
        }));
        let report = amended.payload.tool_call().expect("this is a tool call");
        assert_eq!(report.raw_output, None);
        assert_eq!(report.title, None, "this frame changed no title");

        // An explicit `null` reads the same as an omitted member, so a tool
        // whose whole answer is JSON `null` cannot be told from one that
        // answered nothing. That is the protocol's shape rather than a choice
        // made here, and it is pinned so that whoever meets it later finds it
        // named instead of guessing at a decoder bug.
        let nulled = decode(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "rawOutput": null,
        }));
        let report = nulled.payload.tool_call().expect("this is a tool call");
        assert_eq!(report.raw_output, None);
    }

    #[test]
    fn an_update_about_anything_else_reports_no_tool_call() {
        let event = decode(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "I called the tool for you" },
        }));
        assert!(
            event.payload.tool_call().is_none(),
            "prose about a tool call is not a tool call"
        );
    }

    #[test]
    fn a_tool_call_frame_the_types_cannot_read_is_still_named_as_one() {
        // `title` is required by the protocol, so a frame without one does not
        // decode. What matters is that the loss is loud: the payload still
        // says it was a `tool_call`, so a consumer waiting for a result can
        // tell "the answer went past me" from "no tool was called" — and the
        // second answer, given for the first situation, is a lie about the
        // agent.
        let event = decode(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-1",
            "rawOutput": { "servers": [] },
        }));

        let SessionUpdatePayload::Unrecognized(raw) = &event.payload else {
            panic!("a tool call with no title cannot decode into the typed variant");
        };
        assert_eq!(raw.session_update.as_deref(), Some("tool_call"));
        assert_eq!(raw.raw["rawOutput"], json!({ "servers": [] }));
        assert!(event.payload.tool_call().is_none());
    }

    #[test]
    fn body_that_is_not_an_object_is_unrecognized_not_a_panic() {
        let event = decode(&json!("just a string"));

        let SessionUpdatePayload::Unrecognized(raw) = event.payload else {
            panic!("a non-object body cannot be a typed variant");
        };
        assert_eq!(raw.session_update, None);
        assert_eq!(raw.raw, json!("just a string"));
    }
}
