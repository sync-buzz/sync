//! Every `session/update` variant the live spike saw, and what happens to one
//! it never saw.
//!
//! Two sources, kept apart on purpose:
//!
//! * frames the probe stored raw (`tests/fixtures/session-update-frames.json`)
//!   are used as captured — shortened where a command list was somebody's home
//!   directory, and `tests/fixtures/README.md` says exactly what was dropped;
//! * variants the spike observed by name but did not keep raw are built here,
//!   from the shapes the probe's findings quote.
//!
//! Which is which is stated at each test, because a fixture that claims to be
//! a captured frame and is not would make every conclusion drawn from it
//! worthless.
// In a test, an `expect` on a fixture is the failure report: if the captured
// frames stop being readable, the panic names which one and why.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use acp_client::update::decode_session_update;
use acp_client::{schema, SessionUpdatePayload};
use serde_json::json;

/// Decodes a `session/update` body, failing the test if the envelope is bad.
fn decode(update: &serde_json::Value) -> SessionUpdatePayload {
    decode_session_update(json!({ "sessionId": "s-1", "update": update }))
        .expect("the envelope carries a sessionId")
        .payload
}

/// The typed variant, or a panic naming what came through instead.
fn known(update: &serde_json::Value) -> schema::SessionUpdate {
    match decode(update) {
        SessionUpdatePayload::Known(known) => *known,
        SessionUpdatePayload::Unrecognized(raw) => {
            panic!(
                "expected a typed variant, got raw: {} — {}",
                raw.raw, raw.reason
            )
        }
    }
}

// --- Captured frames ---------------------------------------------------------

/// The raw frames the probe kept, keyed by agent.
fn captured() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/session-update-frames.json"))
        .expect("the captured frames are valid JSON")
}

#[test]
fn every_captured_frame_decodes_into_a_typed_variant() {
    let captured = captured();
    let agents = captured.as_object().expect("keyed by agent");
    assert_eq!(agents.len(), 4, "four agents completed a live cycle");

    let mut decoded = 0_usize;
    for (agent, frames) in agents {
        for frame in frames.as_array().expect("a list of frames") {
            let event = decode_session_update(frame.clone())
                .unwrap_or_else(|e| panic!("{agent}'s captured frame lost its envelope: {e}"));
            match event.payload {
                SessionUpdatePayload::Known(_) => decoded += 1,
                SessionUpdatePayload::Unrecognized(raw) => panic!(
                    "{agent}'s real {:?} frame did not decode: {}",
                    raw.session_update, raw.reason
                ),
            }
        }
    }
    assert!(decoded >= 8, "every captured frame must have been decoded");
}

#[test]
fn the_captured_session_ids_survive_the_decode() {
    // Routing depends on this: one agent process can hold several sessions,
    // and an update that loses its id cannot be delivered to the right one.
    let captured = captured();
    for (agent, frames) in captured.as_object().expect("keyed by agent") {
        for frame in frames.as_array().expect("a list of frames") {
            let want = frame["sessionId"].as_str().expect("a captured sessionId");
            let event = decode_session_update(frame.clone()).expect("decodes");
            assert_eq!(event.session_id.0.as_ref(), want, "{agent}");
        }
    }
}

#[test]
fn codexs_captured_usage_update_reads_its_numbers() {
    let captured = captured();
    let frame = captured["codex"]
        .as_array()
        .expect("codex frames")
        .iter()
        .find(|frame| frame["update"]["sessionUpdate"] == "usage_update")
        .expect("codex sent usage_update");

    let schema::SessionUpdate::UsageUpdate(usage) = known(&frame["update"]) else {
        panic!("usage_update must decode as UsageUpdate");
    };
    assert_eq!(usage.used, 41_643);
    assert_eq!(usage.size, 258_400);
    assert!(usage.cost.is_none(), "only OpenCode was seen sending cost");
}

#[test]
fn codexs_captured_command_list_reads_its_six_commands() {
    // Codex advertises 6 built-ins where the probe saw the others advertise 82
    // to 111, and that gap is the reason a composer cannot rely on this list
    // alone. Codex's six are the one list short enough to keep whole, so they
    // are named here; the others were shortened, per `fixtures/README.md`.
    let captured = captured();
    let frame = captured["codex"]
        .as_array()
        .expect("codex frames")
        .iter()
        .find(|frame| frame["update"]["sessionUpdate"] == "available_commands_update")
        .expect("codex sent available_commands_update");

    let schema::SessionUpdate::AvailableCommandsUpdate(update) = known(&frame["update"]) else {
        panic!("available_commands_update must decode as AvailableCommandsUpdate");
    };
    let names: Vec<&str> = update
        .available_commands
        .iter()
        .map(|command| command.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "review",
            "review-branch",
            "review-commit",
            "init",
            "compact",
            "logout"
        ]
    );
}

// --- Variants observed by name, rebuilt here --------------------------------

#[test]
fn the_message_and_thought_chunk_variants_decode() {
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "PONG" },
        })),
        schema::SessionUpdate::AgentMessageChunk(_)
    ));
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": { "type": "text", "text": "thinking" },
        })),
        schema::SessionUpdate::AgentThoughtChunk(_)
    ));
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": "hello" },
        })),
        schema::SessionUpdate::UserMessageChunk(_)
    ));
}

#[test]
fn the_tool_call_variants_decode() {
    // Shaped after the `toolCalls` the permission probe recorded: Codex and
    // Grok report `execute`, Claude reports `edit` for the same action.
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-1",
            "title": "printf 'OK' > /tmp/probe.txt",
            "kind": "execute",
            "status": "pending",
        })),
        schema::SessionUpdate::ToolCall(_)
    ));
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "completed",
        })),
        schema::SessionUpdate::ToolCallUpdate(_)
    ));
}

#[test]
fn the_plan_variant_decodes() {
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "plan",
            "entries": [
                { "content": "read the module", "priority": "high", "status": "pending" },
            ],
        })),
        schema::SessionUpdate::Plan(_)
    ));
}

#[test]
fn the_current_mode_variant_decodes() {
    // Only Codex and Claude have modes at all; the other two never send this.
    let schema::SessionUpdate::CurrentModeUpdate(update) = known(&json!({
        "sessionUpdate": "current_mode_update",
        "currentModeId": "read-only",
    })) else {
        panic!("current_mode_update must decode as CurrentModeUpdate");
    };
    assert_eq!(update.current_mode_id.0.as_ref(), "read-only");
}

#[test]
fn grokks_session_info_update_decodes_verbatim_from_the_findings() {
    // Quoted verbatim from the probe's findings: Grok sends this in
    // place of the `usage_update` it never sends.
    let schema::SessionUpdate::SessionInfoUpdate(info) = known(&json!({
        "sessionUpdate": "session_info_update",
        "title": "User Demands Exact One-Word PONG Reply",
    })) else {
        panic!("session_info_update must decode as SessionInfoUpdate");
    };
    assert_eq!(
        info.title.as_opt_deref(),
        Some(Some("User Demands Exact One-Word PONG Reply"))
    );
}

#[test]
fn opencodes_usage_update_with_cost_decodes_verbatim_from_the_findings() {
    // Quoted verbatim from the probe's findings. `cost` is OpenCode's
    // alone — Claude and Codex send the same frame without it.
    let schema::SessionUpdate::UsageUpdate(usage) = known(&json!({
        "sessionUpdate": "usage_update",
        "used": 68430,
        "size": 1_048_576,
        "cost": { "amount": 0.004_790_98, "currency": "USD" },
    })) else {
        panic!("usage_update must decode as UsageUpdate");
    };
    assert_eq!(usage.used, 68_430);
    let cost = usage.cost.expect("OpenCode sends cost");
    assert!((cost.amount - 0.004_790_98).abs() < f64::EPSILON);
    assert_eq!(cost.currency, "USD");
}

#[test]
fn the_config_option_variant_decodes() {
    // Claude was seen sending this one; Codex exposes its modes the same way.
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "config_option_update",
            "configOptions": [],
        })),
        schema::SessionUpdate::ConfigOptionUpdate(_)
    ));
}

// --- Divergence that has not happened yet ------------------------------------

#[test]
fn a_variant_this_client_has_never_seen_is_carried_through_whole() {
    // The agents diverged fourfold on the first frame of the protocol. They
    // will invent variants too, and when they do the session must keep
    // running with the payload intact rather than dying on it.
    let SessionUpdatePayload::Unrecognized(raw) = decode(&json!({
        "sessionUpdate": "x.ai/telepathy_update",
        "confidence": 0.7,
    })) else {
        panic!("an unknown variant must not be forced into a typed one");
    };

    assert_eq!(raw.session_update.as_deref(), Some("x.ai/telepathy_update"));
    assert_eq!(raw.raw["confidence"], json!(0.7));
}

#[test]
fn an_unknown_field_on_a_known_variant_is_not_an_unknown_variant() {
    // Grok already does this with `_meta`; the others are free to start.
    assert!(matches!(
        known(&json!({
            "sessionUpdate": "usage_update",
            "used": 1,
            "size": 2,
            "_meta": { "x.ai/anything": true },
            "fieldFromAProtocolRevisionWeDoNotCompileAgainst": [1, 2, 3],
        })),
        schema::SessionUpdate::UsageUpdate(_)
    ));
}
