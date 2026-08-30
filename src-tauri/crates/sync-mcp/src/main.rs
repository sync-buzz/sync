//! `sync-mcp` — one process per project, serving whoever opened it.
//!
//! The engine is linked, not spoken to: `memory-hub-mcp` is a library here and
//! its session is called in process. The protocol in this binary faces outward,
//! and which one it is depends on who spawned it:
//!
//! - an agent gets **MCP** over stdio — a curated, described tool surface;
//! - the window gets its **host channel** over stdio, with `--host` — product
//!   operations, no descriptions, no schemas written for a model.
//!
//! Two dispatchers, not one list with a filter over it: the agent's connection
//! has no route to the window's operations, so there is nothing to guess the
//! name of.

use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use serde_json::{Value, json};

mod application;
mod domain;
mod engine;
mod host;
mod http;
mod link;
mod own;
mod projects;
mod published;
mod server;
mod socket;

type Fallible = Result<(), Box<dyn std::error::Error>>;

/// The model this process resolved, shared by everything that opens a project.
///
/// A named type because it is passed around: every `resolve_active_provider` is
/// another copy of the largest thing this process holds, so there is one call in
/// this binary per invocation and everything else is handed a clone of it.
type Embeddings = Option<std::sync::Arc<dyn memory_hub_mcp::EmbeddingProvider>>;

/// What this process will answer for, and the model it will answer with.
type Answering = (projects::Projects, Embeddings);

fn main() -> Fallible {
    match invocation()? {
        Invocation::Say(message) => {
            println!("{message}");
            Ok(())
        }
        Invocation::Host(project) => serve_host(&project),
        Invocation::Serve(project) => serve_agent(projects_of(&Invocation::Serve(project))?.0),
        Invocation::Registered(registry) => {
            serve_agent(projects_of(&Invocation::Registered(registry))?.0)
        }
        Invocation::Http {
            registry,
            address,
            socket,
            exit_when_orphaned,
        } => {
            let (projects, embeddings) = projects_of(&Invocation::Registered(registry))?;
            if exit_when_orphaned {
                leashed();
            }
            serve_http(projects, embeddings, address, socket)
        }
    }
}

/// Serve the window over stdio.
///
/// Synchronous on purpose: one caller owns this pipe and waits for each answer,
/// so there is nothing for a runtime to overlap. [`host::Host`] knows none of
/// this — the day this channel becomes a socket, only this function changes.
fn serve_host(project: &Path) -> Fallible {
    // Pointed at one project by the command line, so the project travels with
    // the door rather than with each call and **the wire is unchanged**: this
    // is the same protocol, message for message, that the window speaks today.
    // What moved is where the memory is held, which is now the same place the
    // agent's side holds it — one `Domain` per repository in this process,
    // whichever door asked for it.
    let embeddings = memory_hub_mcp::resolve_active_provider();
    let host = host::Host::over(
        std::sync::Arc::new(projects::Projects::over(Vec::new(), embeddings.as_ref())),
        embeddings,
    );
    let input = io::stdin().lock();
    let mut output = io::stdout().lock();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // Answered, not fatal. A refusal from the domain comes back as an error
        // with an id on it, and a line that is not JSON used to come back as
        // the process ending — the same channel reporting the same class of
        // caller mistake two completely different ways. There is no id to
        // answer with, so it is `null`, which is what JSON-RPC says to do.
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let answer = json!({"jsonrpc": "2.0", "id": Value::Null, "error": {
                    "code": -32700,
                    "message": format!("the request could not be read as JSON: {error}"),
                }});
                writeln!(output, "{answer}")?;
                output.flush()?;
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let answer = match host.dispatch(project, method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            // The engine's own `kind` travels in `data`, so the window can tell
            // a stale revision from a locked project without reading prose.
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32000,
                "message": error.to_string(),
                "data": {"kind": error.kind().map(sync_memory::MemoryErrorKind::as_wire)},
            }}),
        };
        writeln!(output, "{answer}")?;
        output.flush()?;
    }
    Ok(())
}

/// The projects an invocation asks this process to answer for.
///
/// The vector model is resolved once, here, and handed to every project. It is
/// the largest thing this process holds, and a machine with several projects
/// open would otherwise pay for one copy of it per project.
///
/// A registry is a file that goes on changing after this returns, so the model
/// is handed to [`projects::Projects`] rather than only to the projects named
/// today: a project opened in the window an hour from now is served by this
/// same process, and has to be served by this same model.
fn projects_of(invocation: &Invocation) -> Result<Answering, Box<dyn std::error::Error>> {
    let embeddings = memory_hub_mcp::resolve_active_provider();
    // Handed back rather than resolved again by whoever else needs it. Every
    // call to `resolve_active_provider` is another copy of the largest thing
    // this process holds, so there is exactly one in this binary and everything
    // downstream is given a clone of the same handle.
    let projects = match invocation {
        Invocation::Http { registry, .. } | Invocation::Registered(registry) => {
            projects::Projects::registered(registry.clone(), embeddings.clone())?
        }
        Invocation::Serve(project) => projects::Projects::just(project.clone(), embeddings.clone()),
        _ => projects::Projects::over(Vec::new(), embeddings.as_ref()),
    };
    Ok((projects, embeddings))
}

/// End when whoever started this process does.
///
/// **The pipe is the leash.** Sync spawns this process holding the write end of
/// its standard input and never writes to it; the operating system closes that
/// end when Sync ends, however it ends — quit, crash, `kill -9`, a development
/// reload — and the read here returns end-of-stream. One mechanism, no polling,
/// no process ids, nothing platform-specific, and nothing to go wrong in the
/// one case that matters: the ways an application dies without getting to run
/// any code of its own.
///
/// It is asked for rather than assumed, because a `sync-mcp` somebody starts in
/// a terminal to serve agents is nobody's child and should not end when they
/// press Ctrl-D.
///
/// Why the engine ends at all, rather than serving on: it holds a port, a
/// socket, an open repository per project and a loaded model. That is a real
/// amount of a person's machine to spend on an application they have closed —
/// and it is why Sync offers to start with the system, so that "my agents can
/// reach my memory" is answered by starting Sync rather than by leaving its
/// engine behind.
fn leashed() {
    std::thread::spawn(|| {
        let mut ignored = Vec::new();
        // Reads to end-of-stream. Nothing is ever sent, so this blocks for the
        // whole life of the application and returns exactly once.
        let _ = io::stdin().lock().read_to_end(&mut ignored);
        // `exit`, not a return: this is a thread beside a running server, and
        // the point is to end the process rather than to unwind one thread of
        // it. Nothing here is mid-write — a call in flight is a caller's to
        // retry, and a caller whose application has just ended has no retry to
        // make.
        std::process::exit(0);
    });
}

/// Serve every agent on this machine over HTTP.
///
/// The token is read from the environment rather than taken as an argument:
/// see [`http::TOKEN_VARIABLE`]. Absent, the door does not open — an HTTP
/// server that fell back to serving without one would be a machine's memory on
/// a port for anything that can open a socket.
fn serve_http(
    projects: projects::Projects,
    embeddings: Embeddings,
    address: std::net::SocketAddr,
    socket: Option<PathBuf>,
) -> Fallible {
    let token = std::env::var(http::TOKEN_VARIABLE).map_err(|_| {
        format!(
            "{} names the bearer token every request has to carry, and is not set",
            http::TOKEN_VARIABLE
        )
    })?;
    // One `Projects` behind both doors, which is the whole point: an agent and
    // the window asking about the same repository reach the same memory, and
    // the model is loaded once for all of them.
    let projects = std::sync::Arc::new(projects);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            // One channel back, held by both doors: the socket puts Sync's
            // connection into it and the agents' server takes tool calls out.
            let application = std::sync::Arc::new(application::Application::new());
            let agents = server::SyncMcp::over(
                std::sync::Arc::clone(&projects),
                std::sync::Arc::clone(&application),
            );
            let Some(path) = socket else {
                // No host door asked for: this is a `sync-mcp` somebody started
                // to serve agents, and the port is the whole of it.
                return http::serve(agents, address, token).await;
            };
            let host = std::sync::Arc::new(host::Host::over(projects, embeddings));
            // **The agents' door goes in the background, and the host door is
            // the one this process lives on.** The port is fixed, written into
            // every agent's configuration, and therefore collidable — and Sync
            // has always treated a taken port as survivable *because the window
            // did not depend on it*. It does now, so the dependency is inverted
            // rather than accepted: whatever else is listening on 41847 costs
            // this machine its agents, and never its memory.
            tokio::spawn(async move {
                if let Err(error) = http::serve(agents, address, token).await {
                    eprintln!("the agents' door did not open: {error}");
                }
            });
            socket::serve(host, application, path)
                .await
                .map_err(Into::into)
        })
}

/// Serve an agent over stdio, speaking MCP.
fn serve_agent(projects: projects::Projects) -> Fallible {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let running = server::SyncMcp::new(projects).serve(stdio()).await?;
            running.waiting().await?;
            Ok(())
        })
}

/// What the command line asked for.
enum Invocation {
    /// Serve an agent over MCP, over the one project named on the command line.
    Serve(PathBuf),
    /// Serve every agent on this machine over HTTP, over every project a
    /// registry names — and, where one is named, serve Sync's own host channel
    /// on a socket from the same process and the same open projects.
    Http {
        registry: PathBuf,
        address: std::net::SocketAddr,
        socket: Option<PathBuf>,
        /// End when the process that started this one does.
        ///
        /// Sync's rule: the engine lives exactly as long as the application.
        /// Quitting Sync ends it, and so does anything else that ends Sync —
        /// a crash, a `kill -9`, a development reload. See [`leashed`].
        exit_when_orphaned: bool,
    },
    /// Serve an agent over MCP, over every project a registry names.
    Registered(PathBuf),
    /// Serve the window over the host channel.
    Host(PathBuf),
    /// Print something and stop.
    Say(String),
}

/// Read the command line.
///
/// The project is named by the caller, because the process is spawned by an
/// agent whose working directory is its own business; the current directory is
/// the fallback rather than the rule.
///
/// `--version` earns its place by being how the build checks that the binary it
/// just staged runs at all — on macOS a sidecar that cannot start is a signing
/// failure, and finding that out at bundle time beats finding it out from a
/// window that will not open.
fn invocation() -> Result<Invocation, Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Ok(Invocation::Serve(std::env::current_dir()?));
    };
    match first.to_str() {
        Some("--version" | "-V") => Ok(Invocation::Say(format!(
            "{} {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ))),
        Some("--help" | "-h") => Ok(Invocation::Say(
            "sync-mcp [--host PROJECT | --registry FILE [--http ADDRESS] | PROJECT]\n\nServes memory: MCP for an agent, or Sync's own host channel with --host.\nWith --registry, serves every project the file lists — over stdio, or over HTTP with --http.\nAn HTTP door takes its bearer token from SYNC_MCP_TOKEN and binds to the loopback only.\nWith PROJECT or nothing, serves the one at that path or in the current directory."
                .to_owned(),
        )),
        Some("--registry") => {
            let registry = PathBuf::from(
                args.next()
                    .ok_or("--registry names the file listing this machine's projects")?,
            );
            // `--http` after it, or stdio. The port is the window's way of
            // serving every agent on the machine at once; the pipe is what a
            // single agent still gets when it starts the process itself.
            match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
                Some("--http") => {
                    let address = args
                        .next()
                        .ok_or("--http names the address to listen on")?
                        .to_str()
                        .ok_or("the address is not readable text")?
                        .parse()?;
                    // Optional. A process serving only agents is a whole
                    // product — it is what a machine with every window closed
                    // has been running all along — so the second door is asked
                    // for rather than assumed.
                    let mut socket = None;
                    let mut exit_when_orphaned = false;
                    while let Some(option) = args.next() {
                        match option.to_str() {
                            Some("--socket") => {
                                socket = Some(PathBuf::from(args.next().ok_or(
                                    "--socket names the path to serve the host channel on",
                                )?));
                            }
                            Some("--exit-when-orphaned") => exit_when_orphaned = true,
                            Some(unexpected) => {
                                return Err(
                                    format!("`{unexpected}` is not an option here").into()
                                );
                            }
                            None => return Err("an option is not readable text".into()),
                        }
                    }
                    Ok(Invocation::Http {
                        registry,
                        address,
                        socket,
                        exit_when_orphaned,
                    })
                }
                Some(unexpected) => Err(format!("`{unexpected}` is not an option here").into()),
                None => Ok(Invocation::Registered(registry)),
            }
        }
        Some("--host") => Ok(Invocation::Host(match args.next() {
            Some(path) => PathBuf::from(path),
            None => std::env::current_dir()?,
        })),
        _ => Ok(Invocation::Serve(PathBuf::from(first))),
    }
}
