//! The door an extension's tools arrive through.
//!
//! **Nothing contributes yet, and that is the point.** This spec builds the
//! server with the seam in place and no wasm runtime behind it; the runtime is
//! a later stage, and it will contribute through this door rather than have one
//! cut for it then. A seam added afterwards is a seam shaped by whatever was
//! easiest to change, which is not the same as the shape the thing needs.
//!
//! What makes it a seam rather than a placeholder is that a contribution is
//! runnable. A registry that could list a tool but not call one would be half
//! of the mechanism, and the missing half is where the surprises live — so a
//! contribution carries the code that answers it, and the tests below call
//! one.

use std::collections::BTreeMap;

use rmcp::model::Tool;
use serde_json::Value;
use sync_memory::{MemoryError, Result};

use crate::domain::Domain;

/// What separates an extension's id from the tool it contributed.
///
/// A dot, and the reason is collision rather than taste. The engine's tools own
/// the `memory_` names and Sync's own the `sync_` ones; a contributed tool
/// carries the id of whoever contributed it, so two extensions may both publish
/// a `search` and neither shadows the other or anything already here.
pub const SEPARATOR: char = '.';

/// The name a contributed tool is published under.
#[must_use]
pub fn full_name(extension: &str, tool: &str) -> String {
    format!("{extension}{SEPARATOR}{tool}")
}

/// The extension a published name belongs to, if it belongs to one.
///
/// Split at the **last** dot, not the first: an extension id is itself dotted —
/// `acme.tracker` — so the first dot is inside the id rather than after it.
#[must_use]
pub fn extension_of(name: &str) -> Option<&str> {
    name.rsplit_once(SEPARATOR).map(|(extension, _)| extension)
}

/// One tool an extension contributes.
///
/// The description and the schema come from the extension's manifest — they are
/// what it says about itself, in its own words, and are not rewritten here for
/// the same reason the engine's are not.
pub struct Contribution {
    /// The extension's id, as the project's record spells it.
    pub extension: String,
    /// The bare tool name, without the id in front of it.
    pub tool: String,
    pub description: String,
    /// The tool's `inputSchema`, as an object.
    pub schema: serde_json::Map<String, Value>,
    /// What answers it.
    ///
    /// # Errors
    ///
    /// Whatever the contribution refused.
    #[allow(clippy::type_complexity)]
    pub run: Box<dyn Fn(&mut Domain, &Value) -> Result<Value> + Send + Sync>,
}

/// The tools contributed to this server, by published name.
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<String, Contribution>,
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bare tool names one extension contributed, in order.
    ///
    /// What makes `extension:<id>` an index rather than a paragraph: an agent
    /// asking about an extension is told what it can call, by name.
    #[must_use]
    pub fn contributed_by(&self, extension: &str) -> Vec<&str> {
        self.tools
            .iter()
            .filter(|(name, _)| extension_of(name) == Some(extension))
            .map(|(_, contribution)| contribution.tool.as_str())
            .collect()
    }

    /// Add one, replacing anything already published under its name.
    ///
    /// Unused until something contributes, which is the whole state of this
    /// stage: the runtime that will walk through this door is a later spec, and
    /// a door built afterwards is shaped by whatever was easiest to change.
    ///
    /// Replacing rather than refusing: an extension reinstalled at a new
    /// version contributes the same names, and the second is the one that
    /// should answer.
    #[allow(
        dead_code,
        reason = "the door is built before the thing that walks through it — see the module note"
    )]
    pub fn contribute(&mut self, contribution: Contribution) {
        self.tools.insert(
            full_name(&contribution.extension, &contribution.tool),
            contribution,
        );
    }

    /// Drop everything one extension contributed.
    ///
    /// What uninstalling is, from here: an extension that is no longer part of
    /// the project should not still be answering for it.
    #[allow(
        dead_code,
        reason = "the door is built before the thing that walks through it — see the module note"
    )]
    pub fn withdraw(&mut self, extension: &str) {
        self.tools
            .retain(|_, contribution| contribution.extension != extension);
    }

    /// Whether a name is one of the contributed ones.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Every contributed tool, described.
    #[must_use]
    pub fn tools(&self) -> Vec<Tool> {
        self.tools
            .iter()
            .map(|(name, contribution)| {
                Tool::new(
                    name.clone(),
                    contribution.description.clone(),
                    std::sync::Arc::new(contribution.schema.clone()),
                )
            })
            .collect()
    }

    /// Run one.
    ///
    /// # Errors
    ///
    /// `tool_not_found` when nothing was contributed under that name, and
    /// whatever the contribution refused otherwise.
    pub fn call(&self, domain: &mut Domain, name: &str, arguments: &Value) -> Result<Value> {
        let Some(contribution) = self.tools.get(name) else {
            return Err(MemoryError::domain(
                "tool_not_found",
                format!("no tool named `{name}`"),
                Value::Null,
            ));
        };
        (contribution.run)(domain, arguments)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn contribution(extension: &str, tool: &str, answer: &'static str) -> Contribution {
        Contribution {
            extension: extension.to_owned(),
            tool: tool.to_owned(),
            description: format!("What `{tool}` does."),
            schema: json!({"type": "object"}).as_object().unwrap().clone(),
            run: Box::new(move |_, arguments| Ok(json!({"answered": answer, "with": arguments}))),
        }
    }

    #[test]
    fn a_contributed_tool_is_published_under_the_id_of_whoever_contributed_it() {
        let mut registry = Registry::new();
        registry.contribute(contribution("acme.tracker", "search", "acme"));

        let names: Vec<String> = registry
            .tools()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(names, vec!["acme.tracker.search"]);
        assert!(registry.holds("acme.tracker.search"));
        assert!(
            !registry.holds("search"),
            "the bare name is nobody's, so nothing answers to it"
        );
    }

    #[test]
    fn two_extensions_may_contribute_the_same_tool_name() {
        let mut registry = Registry::new();
        registry.contribute(contribution("acme.tracker", "search", "acme"));
        registry.contribute(contribution("other.notes", "search", "other"));

        assert_eq!(registry.tools().len(), 2, "neither shadows the other");
    }

    #[test]
    fn a_contribution_is_run_rather_than_only_listed() {
        let mut registry = Registry::new();
        registry.contribute(contribution("acme.tracker", "search", "acme"));
        let mut domain = Domain::open(std::path::PathBuf::from("/nonexistent"), None);

        let answer = registry
            .call(&mut domain, "acme.tracker.search", &json!({"q": "one"}))
            .unwrap();

        assert_eq!(answer["answered"], "acme");
        assert_eq!(answer["with"]["q"], "one", "the arguments reach it whole");
    }

    #[test]
    fn withdrawing_an_extension_takes_everything_it_contributed() {
        let mut registry = Registry::new();
        registry.contribute(contribution("acme.tracker", "search", "acme"));
        registry.contribute(contribution("acme.tracker", "file", "acme"));
        registry.contribute(contribution("other.notes", "search", "other"));

        registry.withdraw("acme.tracker");

        let names: Vec<String> = registry
            .tools()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(names, vec!["other.notes.search"]);
    }

    #[test]
    fn a_name_nobody_contributed_is_refused_by_name() {
        let registry = Registry::new();
        let mut domain = Domain::open(std::path::PathBuf::from("/nonexistent"), None);

        let error = registry
            .call(&mut domain, "acme.tracker.search", &json!({}))
            .unwrap_err();

        assert_eq!(
            error.kind().map(sync_memory::MemoryErrorKind::as_wire),
            Some("tool_not_found")
        );
    }

    #[test]
    fn a_published_name_says_which_extension_it_belongs_to() {
        // The id is dotted, so the split is at the last dot and not the first.
        assert_eq!(extension_of("acme.tracker.search"), Some("acme.tracker"));
        assert_eq!(extension_of("plain.search"), Some("plain"));
        assert_eq!(extension_of("memory_search"), None);
        assert_eq!(extension_of("sync_project"), None);
    }
}
