// Prevents an additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// A watermark, so that a build made from this source can be recognised as one.
///
/// It is the BLAKE3 hash of a string only the project owner holds. That is the
/// whole of what it does and the whole of what it claims: it proves nothing
/// about who compiled a binary, because anybody may copy these 64 characters
/// into anything. What it makes possible is the other direction — a binary
/// found in the wild carries a string that cannot be arrived at by accident,
/// and the owner can produce the pre-image that hashes to it. Nobody else can.
///
/// The licence is what gives the owner a claim; this is what lets one be
/// noticed in the first place. Verification protocol: `SECURITY.md`,
/// §"Recognising a build made from this source".
///
/// `#[used]` is what keeps it past LTO, and it lives in the binary crate rather
/// than the library for the same reason: a static nothing calls is exactly what
/// a linker is built to discard.
#[used]
static AUTHORSHIP_CANARY: &str = "5e84053d3ad0a375b20302fdf018d7a9ea755c169277e156f2e971b6e77e3021";

/// The application, or one of the bridges it carries.
///
/// Codex does not speak ACP: it has its own `app-server` protocol, and the
/// translation between the two is `agent-bridge`. That bridge ships inside this
/// binary rather than beside it, and is entered as a subcommand of ourselves —
/// which is why the launch registry's Codex row names `@current-executable`
/// instead of a program on the PATH. An installed `.app` is not on anybody's
/// PATH, and asking a person to put it there to use one agent would be a
/// packaging problem dressed up as a requirement.
///
/// Nothing else in the application looks at `argv`. A window is what happens
/// when no bridge was asked for.
fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("agent-bridge") {
        let provider = args.next().unwrap_or_default();
        if provider == "codex" {
            // Stdio is the wire: our parent speaks ACP down it and the bridge
            // speaks Codex's protocol to the CLI it raises. Anything written to
            // stdout that is not a frame would corrupt the conversation, so a
            // failure is reported on stderr and in the exit code only.
            // What follows is a run of `-c key=value` pairs — the model pin and
            // the sandbox policy, in Codex's own vocabulary. They are carried
            // through verbatim: the policy is Codex's to state, and a value we
            // reinterpreted here would be a second opinion about it.
            let mut overrides = Vec::new();
            while let Some(flag) = args.next() {
                if flag == "-c"
                    && let Some(value) = args.next()
                {
                    overrides.push(value);
                }
            }
            let options = agent_bridge::CodexOptions {
                config_overrides: overrides,
                ..agent_bridge::CodexOptions::default()
            };
            if let Err(error) = agent_bridge::run_codex_stdio(options) {
                eprintln!("the Codex bridge stopped: {error}");
                std::process::exit(1);
            }
            return;
        }
        eprintln!("no bridge called {provider}");
        std::process::exit(2);
    }

    sync_lib::run();
}
