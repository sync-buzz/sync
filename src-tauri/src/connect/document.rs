//! Putting one entry into somebody else's configuration file, and taking it
//! out again.
//!
//! The rule these are written to is that **the rest of the file is not ours**.
//! An agent's configuration holds that person's servers, their preferences,
//! their comments and their formatting; Sync is a guest in it for the length of
//! one object member. So neither of these functions round-trips the document
//! through a parser and prints it back: they locate a span and splice text into
//! it, and every byte outside that span comes out exactly as it went in.
//!
//! That matters most where it is least visible. `claude_desktop_config.json`
//! sits beside `coworkUserFilesPath` and `preferences`, and `~/.codex/config.toml`
//! is a file somebody has been editing by hand for months — a "helpful" reformat
//! of either is a diff nobody asked for in a file nobody expected Sync to touch.

use jsonc_parser::ast::{ObjectProp, Value as Node};
use jsonc_parser::{CollectOptions, ParseOptions, parse_to_ast, parse_to_serde_value};

/// How wide one level of indentation is when this has to invent some.
const STEP: &str = "  ";

/// What happened to a document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change {
    /// The entry was not there and now is.
    Added,
    /// The entry was there, saying something else, and now says this.
    Updated,
    /// The entry was there, saying exactly this. Nothing was written.
    Unchanged,
    /// The entry was there and now is not.
    Removed,
    /// There was nothing to remove.
    Absent,
}

/// A document that could not be edited, in words that name the file's problem.
#[derive(Debug)]
pub enum Trouble {
    /// It is not JSON, or not TOML, and guessing what was meant is not this
    /// program's business.
    Unreadable(String),
    /// It parses, but the shape it has leaves nowhere to put an entry — the
    /// root is an array, or `mcpServers` is a string.
    WrongShape(String),
}

impl std::fmt::Display for Trouble {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(detail) | Self::WrongShape(detail) => f.write_str(detail),
        }
    }
}

type Result<T> = std::result::Result<T, Trouble>;

// ── JSON ────────────────────────────────────────────────────────────────────

/// Put `entry` under `servers` → `name`, and answer with the new text.
///
/// `entry` is the member's value, already rendered — an object, indented from
/// column zero. It is re-indented to sit where it lands.
///
/// # Errors
///
/// [`Trouble`] when the document cannot be read, or when the place this member
/// belongs is occupied by something that is not an object.
pub fn json_put(text: &str, servers: &str, name: &str, entry: &str) -> Result<(String, Change)> {
    if text.trim().is_empty() {
        let body = indent_block(entry, &format!("{STEP}{STEP}"));
        return Ok((
            format!("{{\n{STEP}\"{servers}\": {{\n{STEP}{STEP}\"{name}\": {body}\n{STEP}}}\n}}\n"),
            Change::Added,
        ));
    }

    let root = root_object(text)?;
    let Some(holder) = member(&root, servers) else {
        // No `mcpServers` at all: the whole block goes in as one member of the
        // root, which is the only case where this writes two levels at once.
        let body = indent_block(entry, &format!("{STEP}{STEP}"));
        let block =
            format!("\"{servers}\": {{\n{STEP}{STEP}\"{name}\": {body}\n{STEP}}}").to_owned();
        return Ok((
            insert_member(text, root.range.start, root.properties.is_empty(), &block),
            Change::Added,
        ));
    };

    let Node::Object(servers_object) = &holder.value else {
        return Err(Trouble::WrongShape(format!(
            "`{servers}` is not an object, so there is nowhere to put a server in it."
        )));
    };

    match member(servers_object, name) {
        Some(existing) => {
            let start = value_start(existing);
            let end = existing.range.end;
            let body = indent_block(entry, &column_of(text, start));
            if text[start..end].trim() == body.trim() {
                return Ok((text.to_owned(), Change::Unchanged));
            }
            let mut edited = String::with_capacity(text.len() + body.len());
            edited.push_str(&text[..start]);
            edited.push_str(&body);
            edited.push_str(&text[end..]);
            Ok((edited, Change::Updated))
        }
        None => {
            let inner = format!("{}{STEP}", column_of(text, servers_object.range.start));
            let body = indent_block(entry, &inner);
            let member = format!("\"{name}\": {body}");
            Ok((
                insert_member(
                    text,
                    servers_object.range.start,
                    servers_object.properties.is_empty(),
                    &member,
                ),
                Change::Added,
            ))
        }
    }
}

/// Take `servers` → `name` back out.
///
/// # Errors
///
/// [`Trouble`] when the document cannot be read.
pub fn json_take(text: &str, servers: &str, name: &str) -> Result<(String, Change)> {
    if text.trim().is_empty() {
        return Ok((text.to_owned(), Change::Absent));
    }
    let root = root_object(text)?;
    let Some(holder) = member(&root, servers) else {
        return Ok((text.to_owned(), Change::Absent));
    };
    let Node::Object(servers_object) = &holder.value else {
        return Ok((text.to_owned(), Change::Absent));
    };
    let Some(existing) = member(servers_object, name) else {
        return Ok((text.to_owned(), Change::Absent));
    };
    Ok((
        cut_member(text, existing.range.start, existing.range.end),
        Change::Removed,
    ))
}

/// Whether the document holds `servers` → `name`, and what it says.
///
/// # Errors
///
/// [`Trouble`] when the document cannot be read.
pub fn json_read(text: &str, servers: &str, name: &str) -> Result<Option<serde_json::Value>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    // The same parser the writing goes through, and that is the point rather
    // than a convenience. These files are JSONC — Zed ends every object with a
    // trailing comma, VS Code ships its settings full of comments — and a
    // reader stricter than the writer would call a file unreadable that Sync
    // had just written into successfully.
    let held: serde_json::Value = parse_to_serde_value(text, &ParseOptions::default())
        .map_err(|error| Trouble::Unreadable(format!("this file is not JSON: {error}")))?;
    Ok(held
        .get(servers)
        .and_then(|servers| servers.get(name))
        .cloned())
}

fn root_object(text: &str) -> Result<jsonc_parser::ast::Object<'_>> {
    let parsed = parse_to_ast(text, &CollectOptions::default(), &ParseOptions::default())
        .map_err(|error| Trouble::Unreadable(format!("this file is not JSON: {error}")))?;
    match parsed.value {
        Some(Node::Object(object)) => Ok(object),
        _ => Err(Trouble::WrongShape(
            "this file's contents are not a JSON object, so there is nowhere to add a server."
                .to_owned(),
        )),
    }
}

fn member<'a, 'b>(
    object: &'a jsonc_parser::ast::Object<'b>,
    name: &str,
) -> Option<&'a ObjectProp<'b>> {
    object
        .properties
        .iter()
        .find(|property| property.name.as_str() == name)
}

/// Where a member's value starts, which is past its name and its colon.
fn value_start(property: &ObjectProp<'_>) -> usize {
    match &property.value {
        Node::StringLit(node) => node.range.start,
        Node::NumberLit(node) => node.range.start,
        Node::BooleanLit(node) => node.range.start,
        Node::Object(node) => node.range.start,
        Node::Array(node) => node.range.start,
        Node::NullKeyword(node) => node.range.start,
    }
}

/// Put one member first inside the object whose `{` is at `brace`.
///
/// First rather than last, and the reason is arithmetic rather than taste: the
/// text right after `{` is a fixed place, while the text before `}` may be a
/// trailing comma, a comment, or a member somebody is in the middle of writing.
fn insert_member(text: &str, brace: usize, empty: bool, member: &str) -> String {
    let inner = format!("{}{STEP}", column_of(text, brace));
    let after = brace + 1;
    let separator = if empty { "" } else { "," };
    let tail = if empty {
        format!("\n{}", column_of(text, brace))
    } else {
        String::new()
    };
    let mut edited = String::with_capacity(text.len() + member.len() + inner.len() + 4);
    edited.push_str(&text[..after]);
    edited.push('\n');
    edited.push_str(&inner);
    edited.push_str(member);
    edited.push_str(separator);
    edited.push_str(&tail);
    edited.push_str(&text[after..]);
    edited
}

/// Take out the member spanning `start..end`, and the comma that joined it.
///
/// The comma after it when there is one, so what follows becomes the first
/// member; otherwise the comma before it, so what precedes it becomes the last.
/// Leaving either behind is a file the agent will refuse to read, which is a
/// worse outcome than not having disconnected at all.
fn cut_member(text: &str, start: usize, end: usize) -> String {
    let mut cut_from = start;
    let mut cut_to = end;

    let after = text[end..].find(|c: char| !c.is_whitespace());
    if let Some(offset) = after
        && text.as_bytes()[end + offset] == b','
    {
        cut_to = end + offset + 1;
    } else if let Some(comma) = text[..start].rfind(',') {
        // Nothing but whitespace may sit between that comma and this member,
        // or it is a comma belonging to something else entirely.
        if text[comma + 1..start].trim().is_empty() {
            cut_from = comma;
        }
    }

    // The blank line the member used to occupy goes with it.
    let line_start = text[..cut_from].rfind('\n').map_or(cut_from, |at| at + 1);
    if text[line_start..cut_from].trim().is_empty() {
        cut_from = line_start;
        if let Some(newline) = text[cut_to..].find('\n')
            && text[cut_to..cut_to + newline].trim().is_empty()
        {
            cut_to += newline + 1;
        }
    }

    format!("{}{}", &text[..cut_from], &text[cut_to..])
}

/// The whitespace at the start of the line `at` falls on.
fn column_of(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    text[line_start..at]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect()
}

/// Re-indent a block written from column zero so it sits at `indent`.
///
/// The first line is left alone: it follows a `"name": ` on a line that is
/// already indented.
fn indent_block(block: &str, indent: &str) -> String {
    let mut lines = block.lines();
    let mut out = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        out.push('\n');
        if !line.trim().is_empty() {
            out.push_str(indent);
        }
        out.push_str(line);
    }
    out
}

// ── TOML ────────────────────────────────────────────────────────────────────

/// Put a server under `[mcp_servers.<name>]`.
///
/// The entry is built by the caller, because no two clients spell one the same
/// way: Codex CLI wants `url` beside an `http_headers` table, Grok wants `url`,
/// `enabled` and a nested `headers` one. Deciding that here would put one
/// client's vocabulary in a file that serves both.
///
/// `toml_edit` keeps the document as it was written — spacing, ordering,
/// comments — so nothing here has to be careful about the rest of the file.
///
/// # Errors
///
/// [`Trouble`] when the document is not TOML.
pub fn toml_put(
    text: &str,
    table: &str,
    name: &str,
    entry: toml_edit::Table,
) -> Result<(String, Change)> {
    let mut document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| Trouble::Unreadable(format!("this file is not TOML: {error}")))?;

    let existing = document
        .get(table)
        .and_then(|servers| servers.get(name))
        .map(std::string::ToString::to_string);
    let change = match existing.as_deref() {
        Some(held) if held == entry.to_string() => return Ok((text.to_owned(), Change::Unchanged)),
        Some(_) => Change::Updated,
        None => Change::Added,
    };

    // Dotted rather than nested, because that is how the file already reads:
    // `[mcp_servers.git-sync]`, one header per server.
    let servers = document
        .entry(table)
        .or_insert(toml_edit::Item::Table({
            let mut created = toml_edit::Table::new();
            created.set_implicit(true);
            created
        }))
        .as_table_mut()
        .ok_or_else(|| {
            Trouble::WrongShape(format!(
                "`{table}` is not a table, so there is nowhere to put a server in it."
            ))
        })?;
    servers.insert(name, toml_edit::Item::Table(entry));
    Ok((document.to_string(), change))
}

/// Take `[mcp_servers.<name>]` back out.
///
/// # Errors
///
/// [`Trouble`] when the document is not TOML.
pub fn toml_take(text: &str, table: &str, name: &str) -> Result<(String, Change)> {
    let mut document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| Trouble::Unreadable(format!("this file is not TOML: {error}")))?;
    let Some(servers) = document
        .get_mut(table)
        .and_then(toml_edit::Item::as_table_mut)
    else {
        return Ok((text.to_owned(), Change::Absent));
    };
    if servers.remove(name).is_none() {
        return Ok((text.to_owned(), Change::Absent));
    }
    Ok((document.to_string(), Change::Removed))
}

/// What `[mcp_servers.<name>]` says, if it says anything.
///
/// # Errors
///
/// [`Trouble`] when the document is not TOML.
pub fn toml_read(text: &str, table: &str, name: &str) -> Result<Option<serde_json::Value>> {
    let document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| Trouble::Unreadable(format!("this file is not TOML: {error}")))?;
    let Some(entry) = document.get(table).and_then(|servers| servers.get(name)) else {
        return Ok(None);
    };
    // The whole entry rather than the fields one client happens to use. What an
    // entry contains is that client's business — `url` beside `http_headers`,
    // or `command` beside `args` — and a reader that knew the shapes would be a
    // third place to update whenever one of them gains a field.
    Ok(Some(as_json(entry)))
}

/// One TOML item, as the JSON the caller reads it with.
fn as_json(item: &toml_edit::Item) -> serde_json::Value {
    match item {
        toml_edit::Item::Value(value) => value_as_json(value),
        toml_edit::Item::Table(table) => serde_json::Value::Object(
            table
                .iter()
                .map(|(key, held)| (key.to_owned(), as_json(held)))
                .collect(),
        ),
        toml_edit::Item::ArrayOfTables(tables) => serde_json::Value::Array(
            tables
                .iter()
                .map(|table| {
                    serde_json::Value::Object(
                        table
                            .iter()
                            .map(|(key, held)| (key.to_owned(), as_json(held)))
                            .collect(),
                    )
                })
                .collect(),
        ),
        toml_edit::Item::None => serde_json::Value::Null,
    }
}

fn value_as_json(value: &toml_edit::Value) -> serde_json::Value {
    match value {
        toml_edit::Value::String(held) => serde_json::Value::String(held.value().clone()),
        toml_edit::Value::Integer(held) => serde_json::json!(held.value()),
        toml_edit::Value::Float(held) => serde_json::json!(held.value()),
        toml_edit::Value::Boolean(held) => serde_json::Value::Bool(*held.value()),
        // A date is a date to TOML and a string to everything reading this.
        toml_edit::Value::Datetime(held) => serde_json::Value::String(held.value().to_string()),
        toml_edit::Value::Array(held) => {
            serde_json::Value::Array(held.iter().map(value_as_json).collect())
        }
        toml_edit::Value::InlineTable(held) => serde_json::Value::Object(
            held.iter()
                .map(|(key, held)| (key.to_owned(), value_as_json(held)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const ENTRY: &str = "{\n  \"command\": \"/opt/sync-mcp\",\n  \"args\": [\n    \"/w/p\"\n  ]\n}";

    #[test]
    fn a_missing_file_becomes_one_holding_only_our_entry() {
        let (text, change) = json_put("", "mcpServers", "sync", ENTRY).unwrap();
        assert_eq!(change, Change::Added);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["mcpServers"]["sync"]["command"], "/opt/sync-mcp");
    }

    #[test]
    fn everything_that_was_already_in_the_file_is_still_in_it_byte_for_byte() {
        // The shape `claude_desktop_config.json` actually has: other top-level
        // keys, another server, and a comment somebody wrote.
        let before = r#"{
  // where my files are
  "coworkUserFilesPath": "/Users/someone/files",
  "mcpServers": {
    "other": {
      "command": "/usr/local/bin/other",
      "args": ["--serve"]
    }
  },
  "preferences": { "theme": "dark" }
}
"#;
        let (after, change) = json_put(before, "mcpServers", "sync", ENTRY).unwrap();
        assert_eq!(change, Change::Added);

        for kept in [
            "// where my files are",
            "\"coworkUserFilesPath\": \"/Users/someone/files\"",
            "\"command\": \"/usr/local/bin/other\"",
            "\"args\": [\"--serve\"]",
            "\"preferences\": { \"theme\": \"dark\" }",
        ] {
            assert!(after.contains(kept), "`{kept}` survived:\n{after}");
        }

        assert_eq!(
            json_read(&after, "mcpServers", "sync").unwrap().unwrap()["command"],
            "/opt/sync-mcp"
        );
        assert_eq!(
            json_read(&after, "mcpServers", "other").unwrap().unwrap()["command"],
            "/usr/local/bin/other"
        );
    }

    #[test]
    fn a_file_with_no_servers_object_gains_one_and_keeps_the_rest() {
        let before = "{\n  \"preferences\": { \"theme\": \"dark\" }\n}\n";
        let (after, _) = json_put(before, "mcpServers", "sync", ENTRY).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["preferences"]["theme"], "dark");
        assert_eq!(parsed["mcpServers"]["sync"]["args"][0], "/w/p");
    }

    #[test]
    fn an_empty_object_is_filled_rather_than_broken() {
        let before = "{\n  \"mcpServers\": {}\n}\n";
        let (after, _) = json_put(before, "mcpServers", "sync", ENTRY).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["mcpServers"]["sync"]["command"], "/opt/sync-mcp");
    }

    #[test]
    fn writing_the_same_entry_twice_writes_nothing_the_second_time() {
        let (once, _) = json_put("", "mcpServers", "sync", ENTRY).unwrap();
        let (twice, change) = json_put(&once, "mcpServers", "sync", ENTRY).unwrap();
        assert_eq!(change, Change::Unchanged);
        assert_eq!(once, twice, "an unchanged file is not rewritten");
    }

    #[test]
    fn a_stale_entry_is_replaced_and_only_it() {
        let before = r#"{
  "mcpServers": {
    "sync": { "command": "/old/sync-mcp", "args": ["/w/p"] },
    "other": { "command": "/usr/local/bin/other" }
  }
}
"#;
        let (after, change) = json_put(before, "mcpServers", "sync", ENTRY).unwrap();
        assert_eq!(change, Change::Updated);
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["mcpServers"]["sync"]["command"], "/opt/sync-mcp");
        assert_eq!(
            parsed["mcpServers"]["other"]["command"],
            "/usr/local/bin/other"
        );
    }

    #[test]
    fn taking_it_out_leaves_a_file_the_agent_can_still_read() {
        let before = r#"{
  "mcpServers": {
    "sync": { "command": "/opt/sync-mcp", "args": ["/w/p"] },
    "other": { "command": "/usr/local/bin/other" }
  },
  "preferences": { "theme": "dark" }
}
"#;
        let (after, change) = json_take(before, "mcpServers", "sync").unwrap();
        assert_eq!(change, Change::Removed);
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert!(parsed["mcpServers"].get("sync").is_none(), "{after}");
        assert_eq!(
            parsed["mcpServers"]["other"]["command"],
            "/usr/local/bin/other"
        );
        assert_eq!(parsed["preferences"]["theme"], "dark");
    }

    #[test]
    fn taking_out_the_last_one_leaves_a_valid_empty_object() {
        let before =
            "{\n  \"mcpServers\": {\n    \"sync\": { \"command\": \"/opt/sync-mcp\" }\n  }\n}\n";
        let (after, change) = json_take(before, "mcpServers", "sync").unwrap();
        assert_eq!(change, Change::Removed);
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert!(
            parsed["mcpServers"].as_object().unwrap().is_empty(),
            "{after}"
        );
    }

    #[test]
    fn taking_out_what_is_not_there_changes_nothing() {
        let before = "{\n  \"mcpServers\": {}\n}\n";
        let (after, change) = json_take(before, "mcpServers", "sync").unwrap();
        assert_eq!(change, Change::Absent);
        assert_eq!(before, after);
    }

    #[test]
    fn a_file_that_is_not_json_is_named_as_the_problem_rather_than_replaced() {
        let error = json_put("this is not json", "mcpServers", "sync", ENTRY).unwrap_err();
        assert!(matches!(error, Trouble::Unreadable(_)), "{error}");
    }

    #[test]
    fn a_toml_file_keeps_its_comments_its_order_and_everything_else() {
        // The shape `~/.codex/config.toml` actually has: settings at the top,
        // other servers, and per-tool blocks under them.
        let before = r#"model = "gpt-5.6-sol"
personality = "pragmatic"

# the one I already had
[mcp_servers.git-sync]
args = ["mcp"]
command = "git-sync"

[mcp_servers.git-sync.tools.sync_search]
approval_mode = "approve"
"#;
        // The entry a client wants, built the way `connect.rs` builds it: an
        // address and a header, in a nested table.
        let mut entry = toml_edit::Table::new();
        entry.insert("url", toml_edit::value("http://127.0.0.1:41847/mcp"));
        let mut headers = toml_edit::Table::new();
        headers.insert("Authorization", toml_edit::value("Bearer abc123"));
        entry.insert("http_headers", toml_edit::Item::Table(headers));
        let (after, change) = toml_put(before, "mcp_servers", "sync", entry).unwrap();
        assert_eq!(change, Change::Added);

        for kept in [
            "model = \"gpt-5.6-sol\"",
            "personality = \"pragmatic\"",
            "# the one I already had",
            "[mcp_servers.git-sync]",
            "[mcp_servers.git-sync.tools.sync_search]",
            "approval_mode = \"approve\"",
        ] {
            assert!(after.contains(kept), "`{kept}` survived:\n{after}");
        }
        assert!(after.contains("[mcp_servers.sync]"), "{after}");

        let read = toml_read(&after, "mcp_servers", "sync").unwrap().unwrap();
        assert_eq!(read["url"], "http://127.0.0.1:41847/mcp");
        assert_eq!(read["http_headers"]["Authorization"], "Bearer abc123");

        let (removed, change) = toml_take(&after, "mcp_servers", "sync").unwrap();
        assert_eq!(change, Change::Removed);
        assert!(!removed.contains("[mcp_servers.sync-p]"), "{removed}");
        assert!(removed.contains("[mcp_servers.git-sync]"), "{removed}");
        assert!(removed.contains("# the one I already had"), "{removed}");
    }

    #[test]
    fn a_comment_is_not_mistaken_for_a_member_when_reading_a_value_back() {
        let text = "{\n  // \"mcpServers\": { \"sync\": {} }\n  \"mcpServers\": { \"sync\": { \"command\": \"/opt/sync-mcp\" } }\n}";
        let read = json_read(text, "mcpServers", "sync").unwrap().unwrap();
        assert_eq!(read["command"], "/opt/sync-mcp");
    }

    /// The shape Zed's `settings.json` actually has: JSON with a comma after
    /// the last member of every object, which a strict parser rejects and a
    /// round-trip through one would silently delete on the way back out.
    #[test]
    fn a_trailing_comma_survives_being_written_into_and_read_back() {
        let before = "{\n  \"theme\": \"One Dark\",\n  \"project_panel\": {\n    \"dock\": \"left\",\n  },\n}\n";

        let (after, change) = json_put(before, "context_servers", "sync", ENTRY).unwrap();
        assert_eq!(change, Change::Added);
        assert!(after.contains("\"dock\": \"left\",\n  },"), "{after}");
        assert_eq!(
            json_read(&after, "context_servers", "sync")
                .unwrap()
                .unwrap()["command"],
            "/opt/sync-mcp"
        );

        // And back out again, leaving the file the way it was found.
        let (removed, change) = json_take(&after, "context_servers", "sync").unwrap();
        assert_eq!(change, Change::Removed);
        assert!(removed.contains("\"theme\": \"One Dark\","), "{removed}");
        assert!(
            json_read(&removed, "context_servers", "sync")
                .unwrap()
                .is_none(),
            "{removed}"
        );
    }
}
