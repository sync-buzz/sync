//! `initialize` read back off the wire, not off the documentation.
//!
//! Every assertion here is against a frame that a real CLI actually sent
//! (`tests/fixtures/initialize-frames.json`). The point is not that the happy
//! path parses — it is that five agents which disagree about almost every
//! optional field all parse, and that the ways they disagree survive the parse
//! instead of being flattened.
// In a test, an `expect` on a fixture is the failure report: if the captured
// frames stop being readable, the panic names which one and why.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use acp_client::{AgentProfile, ProtocolVersion};

/// The captured frames, keyed by agent.
fn frames() -> serde_json::Value {
    let raw = include_str!("fixtures/initialize-frames.json");
    serde_json::from_str(raw).expect("the captured frames are valid JSON")
}

/// The `result` of one captured `initialize` response, parsed as this client
/// would parse it in production.
fn profile(agent: &str) -> AgentProfile {
    let frames = frames();
    let result = frames
        .get(agent)
        .and_then(|entry| entry.get("frame"))
        .and_then(|frame| frame.get("result"))
        .unwrap_or_else(|| panic!("no captured initialize frame for {agent}"))
        .clone();

    AgentProfile::new(
        serde_json::from_value(result)
            .unwrap_or_else(|e| panic!("{agent}'s real initialize frame must parse: {e}")),
    )
}

const AGENTS: [&str; 5] = ["claude", "codex", "opencode", "grok", "gemini"];

#[test]
fn every_captured_frame_parses() {
    for agent in AGENTS {
        let _ = profile(agent);
    }
}

#[test]
fn all_five_settled_on_the_protocol_version_this_client_speaks() {
    for agent in AGENTS {
        let profile = profile(agent);
        assert_eq!(
            profile.protocol_version(),
            ProtocolVersion::V1,
            "{agent} answered a version this client does not speak"
        );
        assert!(profile.speaks_our_protocol_version(), "{agent}");
    }
}

#[test]
fn agents_that_named_themselves_are_read_back_exactly() {
    // Grok is the one that names nothing at the top level — it puts its
    // version and ids in `_meta` instead. That absence is a fact about Grok,
    // so it is asserted, not tolerated.
    let expected: [(&str, Option<(&str, &str)>); 5] = [
        (
            "claude",
            Some(("@agentclientprotocol/claude-agent-acp", "0.66.0")),
        ),
        ("codex", Some(("codex-acp", "0.16.0"))),
        ("opencode", Some(("OpenCode", "1.17.20"))),
        ("grok", None),
        ("gemini", Some(("gemini-cli", "0.45.2"))),
    ];

    for (agent, want) in expected {
        let profile = profile(agent);
        let got = profile.agent_name().zip(profile.agent_version());
        assert_eq!(got, want, "agentInfo of {agent}");
    }
}

#[test]
fn load_session_is_advertised_by_all_five() {
    for agent in AGENTS {
        assert!(
            profile(agent).supports_load_session(),
            "{agent} used to advertise loadSession"
        );
    }
}

#[test]
fn the_auth_methods_each_agent_actually_offered() {
    // The spread is the finding: Claude offers nothing because it is already
    // authenticated, Gemini offers four, and a client that assumed either
    // extreme would be wrong about the other.
    let expected: [(&str, &[&str]); 5] = [
        ("claude", &[]),
        ("codex", &["chatgpt", "codex-api-key", "openai-api-key"]),
        ("opencode", &["opencode-login"]),
        ("grok", &["cached_token", "grok.com"]),
        (
            "gemini",
            &["oauth-personal", "gemini-api-key", "vertex-ai", "gateway"],
        ),
    ];

    for (agent, want) in expected {
        let profile = profile(agent);
        assert_eq!(profile.auth_method_ids(), want, "authMethods of {agent}");
        assert_eq!(profile.offers_authentication(), !want.is_empty(), "{agent}");
    }
}

#[test]
fn codexs_typed_auth_methods_still_arrive_as_methods() {
    // Two of Codex's three carry `"type": "env_var"`, which this client's
    // feature set does not model. They must still be offered as methods with
    // their ids — silently dropping them would make the list a lie.
    let profile = profile("codex");
    assert_eq!(profile.auth_method_ids().len(), 3);
}

#[test]
fn an_agents_own_meta_extensions_are_not_dropped() {
    // Grok hangs its model list, working directory and agent ids off
    // `initialize._meta`. Nothing else does. It has to survive the parse: it
    // is the only place that information exists for that agent.
    let grok = profile("grok");
    let meta = grok
        .response()
        .meta
        .as_ref()
        .expect("Grok's initialize frame carries _meta");
    let meta = serde_json::to_value(meta).expect("meta is JSON");

    assert_eq!(meta["agentVersion"], serde_json::json!("1.0.0"));
    assert_eq!(
        meta["modelState"]["currentModelId"],
        serde_json::json!("grok-4.5")
    );
    assert!(
        meta["availableCommands"]
            .as_array()
            .is_some_and(|c| !c.is_empty()),
        "Grok announces its commands inside initialize._meta"
    );
}

#[test]
fn agent_capabilities_the_types_do_not_model_do_not_break_the_parse() {
    // `sessionCapabilities` differs fourfold across these five, and Grok puts
    // an `x.ai/hooks` object inside `agentCapabilities._meta`. Neither is
    // modelled by the compiled types; both must pass through harmlessly.
    let grok = profile("grok");
    assert!(grok.supports_load_session());

    let gemini = profile("gemini");
    // Gemini has no `sessionCapabilities` key at all — the opposite shape.
    assert!(gemini.supports_load_session());
}
