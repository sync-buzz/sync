//! Reading the `initialize` answer.
//!
//! Every agent measured answers `protocolVersion: 1` and then diverges
//! immediately: `sessionCapabilities` is absent on Gemini, `list`/`resume`/`close`
//! on Codex and Grok, plus `fork` on `OpenCode`, plus `fork`/`delete`/`additionalDirectories`
//! on Claude. `agentInfo` is missing entirely on Grok, which puts its version
//! and identifiers in `_meta` instead. `authMethods` runs from empty (Claude,
//! already authenticated) to four entries (Gemini).
//!
//! [`AgentProfile`] is the narrow set of answers this client and the layers
//! above it actually act on. The whole response stays reachable through
//! [`AgentProfile::response`], because a summary that quietly drops a field is
//! worse than no summary.
//!
//! One deliberate loss to name: `agent-client-protocol-schema`'s typed
//! `env_var` and `terminal` auth methods sit behind its `unstable_auth_methods`
//! feature, which is not enabled here. Codex's two API-key methods therefore
//! read as ordinary agent-handled methods and their `vars` list is not
//! surfaced. That costs nothing: Sync never holds, types or proxies AI
//! credentials, so an API-key auth method is one this client will not choose.

use agent_client_protocol_schema::v1 as schema;
use agent_client_protocol_schema::ProtocolVersion;

/// The protocol version this client speaks.
///
/// Every agent measured live answers with exactly this.
pub const SUPPORTED_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1;

/// What an agent said about itself in `initialize`.
#[derive(Debug, Clone)]
pub struct AgentProfile {
    response: schema::InitializeResponse,
}

impl AgentProfile {
    /// Wraps an `initialize` response.
    #[must_use]
    pub fn new(response: schema::InitializeResponse) -> Self {
        Self { response }
    }

    /// The response as it arrived.
    #[must_use]
    pub fn response(&self) -> &schema::InitializeResponse {
        &self.response
    }

    /// The protocol version the agent settled on.
    #[must_use]
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.response.protocol_version
    }

    /// Whether that version is the one this client speaks.
    ///
    /// The protocol says a client that does not support the returned version
    /// should disconnect. This is where a caller finds that out — before it
    /// opens a session and discovers it the hard way.
    #[must_use]
    pub fn speaks_our_protocol_version(&self) -> bool {
        self.protocol_version() == SUPPORTED_PROTOCOL_VERSION
    }

    /// The agent's own name, when it gave one. Grok does not.
    #[must_use]
    pub fn agent_name(&self) -> Option<&str> {
        self.response
            .agent_info
            .as_ref()
            .map(|info| info.name.as_str())
    }

    /// The agent's own version string, when it gave one.
    #[must_use]
    pub fn agent_version(&self) -> Option<&str> {
        self.response
            .agent_info
            .as_ref()
            .map(|info| info.version.as_str())
    }

    /// Whether `session/load` is on offer. All four verified agents say yes,
    /// and all four were seen honouring it.
    #[must_use]
    pub fn supports_load_session(&self) -> bool {
        self.response.agent_capabilities.load_session
    }

    /// Whether the agent accepts images in a prompt.
    ///
    /// Measured: Claude, Codex, `OpenCode` and Gemini all answer `true`; Grok
    /// answers `false`. So this is a real distinction between installed agents
    /// rather than a defensive reading of the schema, and it is the difference
    /// between offering a person a gesture that works and one that does not.
    ///
    /// An agent that says nothing is taken at its word — `false`. The protocol
    /// makes the capability the permission, and sending an image to an agent
    /// that never claimed to read one is a prompt it may reject whole.
    #[must_use]
    pub fn accepts_images(&self) -> bool {
        self.response.agent_capabilities.prompt_capabilities.image
    }

    /// Whether the agent offers any way to authenticate.
    ///
    /// An empty list is not a failure: Claude answers with none because it is
    /// already authenticated. It means "there is nothing here for you to
    /// choose", not "you cannot get in".
    #[must_use]
    pub fn offers_authentication(&self) -> bool {
        !self.response.auth_methods.is_empty()
    }

    /// The ids of the offered authentication methods, in the agent's order.
    #[must_use]
    pub fn auth_method_ids(&self) -> Vec<&str> {
        self.response
            .auth_methods
            .iter()
            .map(|method| method.id().0.as_ref())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(json: serde_json::Value) -> AgentProfile {
        AgentProfile::new(serde_json::from_value(json).expect("initialize response"))
    }

    #[test]
    fn an_unknown_protocol_version_is_reported_not_assumed() {
        let profile = profile(serde_json::json!({ "protocolVersion": 7 }));
        assert_eq!(profile.protocol_version(), ProtocolVersion::from(7));
        assert!(!profile.speaks_our_protocol_version());
    }

    #[test]
    fn version_one_is_the_one_we_speak() {
        let profile = profile(serde_json::json!({ "protocolVersion": 1 }));
        assert!(profile.speaks_our_protocol_version());
    }

    #[test]
    fn an_agent_that_names_itself_is_read_back() {
        let profile = profile(serde_json::json!({
            "protocolVersion": 1,
            "agentInfo": { "name": "`OpenCode`", "version": "1.17.20" },
        }));
        assert_eq!(profile.agent_name(), Some("`OpenCode`"));
        assert_eq!(profile.agent_version(), Some("1.17.20"));
    }

    #[test]
    fn an_agent_that_names_nothing_is_not_invented_for() {
        let profile = profile(serde_json::json!({ "protocolVersion": 1 }));
        assert_eq!(profile.agent_name(), None);
        assert_eq!(profile.agent_version(), None);
    }

    #[test]
    fn an_agent_says_for_itself_whether_it_reads_pictures() {
        let claude = profile(serde_json::json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "promptCapabilities": { "image": true, "embeddedContext": true },
            },
        }));
        assert!(claude.accepts_images());

        let grok = profile(serde_json::json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "promptCapabilities": { "image": false, "audio": false },
            },
        }));
        assert!(!grok.accepts_images());

        let silent = profile(serde_json::json!({ "protocolVersion": 1 }));
        assert!(
            !silent.accepts_images(),
            "an agent that never claimed to read one is not assumed to",
        );
    }

    #[test]
    fn an_empty_auth_list_means_nothing_to_choose() {
        let profile = profile(serde_json::json!({
            "protocolVersion": 1,
            "authMethods": [],
        }));
        assert!(!profile.offers_authentication());
        assert!(profile.auth_method_ids().is_empty());
    }

    #[test]
    fn auth_method_ids_come_back_in_the_agents_order() {
        let profile = profile(serde_json::json!({
            "protocolVersion": 1,
            "authMethods": [
                { "id": "cached_token", "name": "cached_token", "description": "d" },
                { "id": "grok.com", "name": "Grok", "description": "d" },
            ],
        }));
        assert!(profile.offers_authentication());
        assert_eq!(profile.auth_method_ids(), ["cached_token", "grok.com"]);
    }
}
