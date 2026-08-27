# Fixtures

Frames captured off real agent CLIs during a live probe of five of them. They
are here because the protocol's documentation says how ACP *should* look and
these say how it *is*, and the two differ — by enough that a client built from
the documentation alone would have had to be rewritten.

## `initialize-frames.json`

Verbatim. The complete `initialize` response frame from each of the five CLIs
probed, exactly as it came off the wire:
`{"<agent>": {"at": "<timestamp>", "frame": {"jsonrpc", "id", "result"}}}`.

## `session-update-frames.json`

`session/update` notification params (`{"sessionId": …, "update": {…}}`), one
per distinct `sessionUpdate` variant per agent, from the four CLIs that
completed a session.

**Shortened, and it says so here rather than claiming otherwise.** Three of the
four advertised their operator's whole command catalogue — 82 to 111 entries
each, naming private work and carrying the absolute path of every file on that
machine. None of that is what the fixture is for, and a repository is a poor
place to keep somebody's home directory. So each list keeps a handful of
entries and a home directory reads `example`.

What was preserved is every distinct **shape** the capture showed, because that
is the whole value of a captured frame — a decoder that handles one and not
another is the failure these tests exist to catch:

| agent | shapes kept |
| --- | --- |
| `claude` | `input: null` and `input: {hint}` |
| `codex` | untouched — all six entries, which a test names one by one |
| `opencode` | no `input` member at all |
| `grok` | `_meta: {scope, path}` in both of its `scope` values, against both `input` forms |

Every other member of every frame is unchanged, and `usage_update` and
`agent_thought_chunk` were not touched at all.

Only the variants the probe happened to store raw are here — chiefly
`available_commands_update`, `usage_update` and `agent_thought_chunk`. The other
variants the probe *observed by name* (`agent_message_chunk`, `tool_call`,
`tool_call_update`, `session_info_update`, `config_option_update`,
`user_message_chunk`) were not kept as raw frames, so the tests that cover them
build the frames instead, and each says which kind it is.
