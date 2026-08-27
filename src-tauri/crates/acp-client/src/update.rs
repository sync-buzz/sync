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
    fn body_that_is_not_an_object_is_unrecognized_not_a_panic() {
        let event = decode(&json!("just a string"));

        let SessionUpdatePayload::Unrecognized(raw) = event.payload else {
            panic!("a non-object body cannot be a typed variant");
        };
        assert_eq!(raw.session_update, None);
        assert_eq!(raw.raw, json!("just a string"));
    }
}
