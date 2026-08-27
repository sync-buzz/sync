//! What a session sends to the window.
//!
//! # Why the payload travels raw
//!
//! An update is forwarded as the JSON the agent wrote, not as a canon of our
//! own. The client crate's whole design is that an update it cannot read still
//! arrives intact, and a normalising layer here would have to make a decision
//! about every variant it has never seen — which is the one thing that is
//! guaranteed to go stale, because the agents already disagree about which
//! variants they emit and will disagree more. `recognized` says whether the
//! compiled protocol types could read it, and that is the whole of the
//! interpretation done on this side.
//!
//! # Why the time is stamped here
//!
//! A conversation is assembled in the window: a run of text chunks becomes one
//! message, and a **pause** between chunks is one of the things that ends it.
//! Arrival time in the window cannot be that measurement — a re-subscribing
//! area is handed the whole history at once, and every event in it would look
//! simultaneous, so a reopened conversation would collapse into a single block
//! exactly where a live one read correctly. The clock that matters is the one
//! that was running when the frame arrived, so the time is taken here and
//! carried.

use serde::Serialize;

/// Where a session is, as one word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// The process is being raised and the session opened. Where a session
    /// starts, so also the default.
    #[default]
    Starting,
    /// Open, and not in a turn.
    Ready,
    /// A turn is running: the agent is working.
    Working,
    /// The agent is waiting for an answer to a permission request.
    Asking,
    /// It ended by itself, or its process died.
    Ended,
    /// It could not be raised, or it fell over.
    Failed,
}

/// A pasted image, as the window is told about it.
///
/// The bytes are not here. They are in the session, under `id`, until the
/// conversation ends — nothing is written to disk, and nothing survives the
/// application closing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PastedImage {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// How many bytes it is, so a window can say so without fetching it.
    pub bytes: u64,
}

/// One thing that happened in a session.
///
/// Serialised with a `kind` discriminator so the window switches on one field,
/// and every variant carries `seq` — the ordinal of the event in this session —
/// so a replayed history and a live stream are the same sequence and the window
/// can tell where the replay stopped.
/// `rename_all_fields`, not just `rename_all`. On an enum the latter renames the
/// **variants** and leaves the fields inside them alone, so `at_ms` crossed as
/// `at_ms` while the window read `atMs` and got `undefined` — silently, because
/// a field the other side cannot find simply reads as absent. That cost the
/// whole message-boundary rule: the pause was measured as `NaN`, no comparison
/// against it was ever true, and every chunk became its own paragraph. Answering
/// a permission was broken the same way, by `request_id`. The test at the foot
/// of this file is what stops it happening a third time.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionEvent {
    /// The session moved.
    Status {
        seq: u64,
        at_ms: u64,
        status: Status,
        /// Why, for the states where a word is not enough.
        detail: Option<String>,
    },
    /// What a person said.
    ///
    /// Not a protocol event: ACP has no notification for what the client sent,
    /// so nothing would ever arrive to record it. It is kept here anyway,
    /// because the alternative is what the first version did — hold it in the
    /// screen's own state, where it is destroyed by an unmount. Leaving a
    /// section and coming back then showed the agent answering questions
    /// nobody had asked.
    /// `attachments` are the files that were sent with it, as absolute paths.
    /// The agent is handed them as resource links and reads them itself, so
    /// what is recorded here is the same thing that was sent — a path — rather
    /// than a copy of a file this application never took.
    ///
    /// `images` are the pictures pasted into it. A path is no use for one —
    /// there is no file — so what travels is the id the session holds the
    /// bytes under, and the window asks for them when it draws them. Which is
    /// also what keeps a megabyte of image out of a history that is replayed
    /// whole every time a screen comes back to the conversation.
    Prompt {
        seq: u64,
        at_ms: u64,
        text: String,
        attachments: Vec<String>,
        images: Vec<PastedImage>,
    },
    /// A `session/update` notification, exactly as the agent wrote it.
    Update {
        seq: u64,
        at_ms: u64,
        /// The `sessionUpdate` discriminator, when the payload carried one.
        update: Option<String>,
        /// Whether the compiled protocol types could read it.
        recognized: bool,
        payload: serde_json::Value,
        /// Whether this arrived while the agent was replaying a loaded session
        /// rather than saying something new.
        ///
        /// The window needs the difference for exactly one variant, and it is
        /// the one that decides whether a resumed conversation has a person in
        /// it. `user_message_chunk` is the agent quoting what somebody typed:
        /// during a live turn Sync already recorded that as a `Prompt` when it
        /// was sent, so folding the echo too would print the sentence twice —
        /// but during a replay the `Prompt` never happened, and dropping the
        /// echo leaves the agent talking to nobody.
        ///
        /// It is stated here rather than worked out in the window because only
        /// this side knows: replaying is the span between asking for
        /// `session/load` and being answered, and the window never sees either.
        replayed: bool,
    },
    /// The agent is asking to be allowed to do something, and its turn is
    /// stopped until it hears back.
    Permission {
        seq: u64,
        at_ms: u64,
        /// This session's own number for the question, and what an answer names.
        request_id: u64,
        /// The tool being asked about, under the name this application uses for
        /// it rather than under the four spellings the agents use.
        tool_name: Option<String>,
        /// The request as the agent wrote it — above all its `options`, in its
        /// own order and with its own `kind`s. An agent that offers no
        /// "always allow" must not be shown one.
        request: serde_json::Value,
    },
    /// A question is no longer open, because it was answered, or because the
    /// session ended under it.
    PermissionSettled {
        seq: u64,
        at_ms: u64,
        request_id: u64,
        /// The option that was chosen, or `None` where nothing was.
        chosen: Option<String>,
    },
    /// The session's configuration, as the agent stated it — the model among
    /// it. Sent when a session opens and again whenever the agent restates it.
    Configuration {
        seq: u64,
        at_ms: u64,
        options: serde_json::Value,
    },
    /// The modes the agent says it can work in, and which one it is in now.
    ///
    /// Its own event rather than part of the configuration, because the two are
    /// separate answers in the protocol and arrive separately: an agent may
    /// state either without the other. Sent when a session opens or is
    /// reloaded, and again whenever a mode is set.
    ///
    /// The whole state travels, the current id with it, because that is the
    /// shape the protocol answers in and splitting it here would mean
    /// reassembling it there. Which mode the agent moved to *of its own accord*
    /// still arrives as an ordinary `current_mode_update` on
    /// [`SessionEvent::Update`]. Both fold into the one field the window holds
    /// for it — two writers in sequence rather than two facts to keep in step,
    /// which is what a separate carrier for the current mode would have been.
    Modes {
        seq: u64,
        at_ms: u64,
        modes: serde_json::Value,
    },
}

impl SessionEvent {
    /// The ordinal this event was given.
    pub fn seq(&self) -> u64 {
        match self {
            Self::Status { seq, .. }
            | Self::Prompt { seq, .. }
            | Self::Update { seq, .. }
            | Self::Permission { seq, .. }
            | Self::PermissionSettled { seq, .. }
            | Self::Configuration { seq, .. }
            | Self::Modes { seq, .. } => *seq,
        }
    }
}

/// Milliseconds since the epoch, or 0 on a machine whose clock is before it.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names the window reads, pinned as text.
    ///
    /// Not a style check. This boundary fails silently in one direction: a field
    /// whose name does not match is not an error on either side, it is simply
    /// absent when read, and what breaks is somewhere else entirely and much
    /// later. So the wire is asserted as a string rather than trusted to an
    /// attribute.
    #[test]
    fn every_field_crosses_under_the_name_the_window_reads() {
        let status = serde_json::to_string(&SessionEvent::Status {
            seq: 1,
            at_ms: 1234,
            status: Status::Working,
            detail: None,
        })
        .expect("a status serialises");
        assert_eq!(
            status,
            r#"{"kind":"status","seq":1,"atMs":1234,"status":"working","detail":null}"#
        );

        let question = serde_json::to_string(&SessionEvent::Permission {
            seq: 2,
            at_ms: 5678,
            request_id: 9,
            tool_name: Some("sync/read".to_owned()),
            request: serde_json::Value::Null,
        })
        .expect("a question serialises");
        assert_eq!(
            question,
            r#"{"kind":"permission","seq":2,"atMs":5678,"requestId":9,"toolName":"sync/read","request":null}"#
        );

        let settled = serde_json::to_string(&SessionEvent::PermissionSettled {
            seq: 3,
            at_ms: 9,
            request_id: 9,
            chosen: None,
        })
        .expect("a settlement serialises");
        assert_eq!(
            settled,
            r#"{"kind":"permissionSettled","seq":3,"atMs":9,"requestId":9,"chosen":null}"#
        );

        let said = serde_json::to_string(&SessionEvent::Prompt {
            seq: 5,
            at_ms: 11,
            text: "hello".to_owned(),
            attachments: vec!["/tmp/shot.png".to_owned()],
            images: vec![PastedImage {
                id: "p0".to_owned(),
                name: "Pasted image".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: 2048,
            }],
        })
        .expect("a prompt serialises");
        assert_eq!(
            said,
            r#"{"kind":"prompt","seq":5,"atMs":11,"text":"hello","attachments":["/tmp/shot.png"],"images":[{"id":"p0","name":"Pasted image","mimeType":"image/png","bytes":2048}]}"#
        );

        let update = serde_json::to_string(&SessionEvent::Update {
            seq: 4,
            at_ms: 10,
            update: Some("agent_message_chunk".to_owned()),
            recognized: true,
            payload: serde_json::Value::Null,
            replayed: false,
        })
        .expect("an update serialises");
        assert_eq!(
            update,
            r#"{"kind":"update","seq":4,"atMs":10,"update":"agent_message_chunk","recognized":true,"payload":null,"replayed":false}"#
        );

        // The one the window branches on. Asserted as text for the same reason
        // as the rest: an attribute that looks right is not evidence that the
        // field crossed under the name the other side reads.
        let replayed = serde_json::to_string(&SessionEvent::Update {
            seq: 5,
            at_ms: 11,
            update: Some("user_message_chunk".to_owned()),
            recognized: true,
            payload: serde_json::Value::Null,
            replayed: true,
        })
        .expect("a replayed update serialises");
        assert!(
            replayed.contains(r#""replayed":true"#),
            "the window folds a person's words only on a replay: {replayed}"
        );

        // The list the composer's mode picker is drawn from. It travels as the
        // agent's own shape, so the assertion is about the envelope this side
        // owns — the kind and the member name — and not about what is inside.
        let modes = serde_json::to_string(&SessionEvent::Modes {
            seq: 6,
            at_ms: 12,
            modes: serde_json::json!({
                "currentModeId": "plan",
                "availableModes": [{ "id": "plan", "name": "Plan" }],
            }),
        })
        .expect("a mode state serialises");
        assert_eq!(
            modes,
            r#"{"kind":"modes","seq":6,"atMs":12,"modes":{"currentModeId":"plan","availableModes":[{"id":"plan","name":"Plan"}]}}"#
        );
    }
}
