//! The MCP server this installation runs, and the settings that describe it.
//!
//! One process, started with the application and outliving every window: agents
//! reach Sync through a port rather than by starting a copy of the engine each,
//! which is what makes several projects share one loaded model.
//!
//! Nothing here is lazy. The server starts when the application does, whether
//! or not an agent is connected and whether or not a window is open — a rule
//! about *when* to start would be a rule somebody has to know, and the answer
//! to "is it running" should not be "it depends".

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

use crate::project::{ProjectError, configuration_file, write_configuration};

/// What this installation's server is called in its own configuration.
const SETTINGS_FILE: &str = "mcp-server.json";

/// The registry the server serves, written by [`crate::project`].
const REGISTRY_FILE: &str = "registered-projects.json";

/// The host channel this installation's window and clock come in through.
///
/// A socket rather than a second port. The port above is written into every
/// agent's configuration and is therefore fixed and therefore collidable, and a
/// port already taken is reported rather than stepped around — which is
/// survivable exactly while the window does not depend on it. A socket in the
/// application's own directory collides with nothing, and its file permissions
/// are its whole access control: the token exists because an agent is
/// configured with a URL, and the window is configured with nothing.
const HOST_SOCKET_FILE: &str = "host.sock";

/// On macOS a socket address is a `sun_path` of 104 bytes, and a path over it
/// does not fail at bind with anything a person could act on. Guarded here,
/// with room left, so the refusal names the path and the limit.
const SOCKET_PATH_LIMIT: usize = 100;

/// Where the window and the clock reach this installation's engine.
///
/// # Errors
///
/// When the configuration directory cannot be resolved, or the path it gives is
/// longer than a socket address can hold.
pub fn host_socket<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, ProjectError> {
    let path = configuration_file(app, HOST_SOCKET_FILE)?;
    if path.as_os_str().len() > SOCKET_PATH_LIMIT {
        return Err(ProjectError::new(
            "configuration_failed",
            format!(
                "the memory engine's socket would be at {}, which is longer than the {SOCKET_PATH_LIMIT} characters a socket address holds",
                path.display()
            ),
        ));
    }
    Ok(path)
}

/// The port nothing else on a developer's machine tends to want.
///
/// Fixed rather than chosen at startup, and that is the point: the address is
/// written into every agent's configuration, so a port that moved between runs
/// would leave every one of those entries pointing at nothing. A port already
/// taken is therefore reported rather than stepped around.
const DEFAULT_PORT: u16 = 41_847;

/// What the person can decide about their server.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    /// The loopback port it listens on.
    pub port: u16,
    /// The bearer token every request carries.
    ///
    /// Kept here rather than in a keychain because Connect has to write it into
    /// an agent's own configuration file, which is a file on this disk with
    /// this person's permissions — a secret in a keychain that is copied into a
    /// plain file is a secret in a plain file.
    pub token: String,
}

impl ServerSettings {
    /// This installation's settings, invented on first use.
    pub(crate) fn load<R: Runtime>(app: &AppHandle<R>) -> Self {
        let stored = configuration_file(app, SETTINGS_FILE)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Self>(&text).ok());
        stored.unwrap_or_else(|| Self {
            port: DEFAULT_PORT,
            token: minted_token(),
        })
    }

    /// Where an agent is told to reach this server.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    fn address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

/// A token no other process can guess.
///
/// Sixteen bytes of the operating system's randomness, in hex. Not a word, not
/// a counter, and not derived from anything about this machine: the point of it
/// is to be unguessable by a process that can read everything about this
/// machine except this file.
fn minted_token() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).unwrap_or_else(|_| {
        // The operating system refusing randomness is not a case to paper over
        // with a fallback nobody would review. It cannot happen on any platform
        // this ships to, and a predictable token would be worse than no server.
        panic!("the operating system refused randomness for the server token")
    });
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The running server, or the reason there is not one.
#[derive(Default)]
pub struct RunningServer {
    child: Mutex<Option<Child>>,
    /// The write end of the engine's standard input, held and never written to.
    ///
    /// **This is the leash.** The engine reads that pipe to end-of-stream and
    /// exits when it gets one, and the operating system gives it one the moment
    /// this process ends — however it ends. Quitting Sync closes it; so does a
    /// crash, a `kill -9` and a development reload, which are exactly the ways
    /// an application ends without getting to run any code of its own.
    ///
    /// `RunEvent::Exit` kills the child as well, and that is not a duplicate:
    /// it makes a clean quit immediate rather than leaving somebody's engine up
    /// for as long as it takes to notice a closed pipe.
    leash: Mutex<Option<std::process::ChildStdin>>,
    /// The host channel of the process now running.
    ///
    /// Set when one is started and cleared when it is stopped, so that
    /// "does this installation run a resident engine" has one answer and it is
    /// a fact rather than a guess about a file on the disk. `None` in a test,
    /// which drives commands with no application around them and gets a process
    /// of its own — the one arrangement where one engine per project is right,
    /// because a test's corpus is nobody else's.
    socket: Mutex<Option<PathBuf>>,
}

impl RunningServer {
    /// Where the window and the clock reach the engine, when a process is up.
    #[must_use]
    pub fn socket(&self) -> Option<PathBuf> {
        self.socket.lock().ok().and_then(|held| held.clone())
    }
}

/// What the settings window shows about the server.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub port: u16,
    pub token: String,
    pub url: String,
    /// Whether the process is up right now.
    pub running: bool,
    /// Why it is not, when it is not.
    pub failure: Option<String>,
}

/// Whether an agent could reach this machine right now.
fn answering_agents<R: Runtime>(app: &AppHandle<R>) -> bool {
    std::net::TcpStream::connect_timeout(
        &ServerSettings::load(app).address(),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}

/// Whether anything is serving the host channel at `path`.
///
/// By connecting, because a socket file outlives the process that made it and
/// its presence says nothing at all.
fn answering(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// How long a freshly started process is given to open its door.
///
/// It loads a model before it listens, so this is generous. What it must not be
/// is absent: the window's memory now depends on this process, so "started"
/// has to mean "answering" rather than "spawned" — a child that dies a second
/// later would otherwise look exactly like a good start.
const OPENING: std::time::Duration = std::time::Duration::from_secs(30);

/// Start the engine, replacing whatever is serving.
///
/// **One engine per machine, and it is this application's.** Sync's rule is
/// that the engine lives exactly as long as Sync does, so anything still
/// holding the door when Sync starts is something that outlived the run that
/// began it — a development reload, a crash before the leash was noticed. It is
/// stopped and replaced rather than adopted: adopting one would mean this
/// window's memory depending on a process built from code nobody here is
/// looking at, with no way to end it.
///
/// # Errors
///
/// Reports whatever starting the process refused, including a port this machine
/// will not give out.
pub fn start<R: Runtime>(app: &AppHandle<R>, running: &RunningServer) -> Result<(), ProjectError> {
    stop(running);
    displace(&host_socket(app)?);

    let settings = ServerSettings::load(app);
    // Written back on every start: the first one mints the token, and a start
    // after the person changed the port has to leave the file agreeing with the
    // process.
    let path = configuration_file(app, SETTINGS_FILE)?;
    write_configuration(&path, &settings)?;
    restrict(&path);

    let registry = configuration_file(app, REGISTRY_FILE)?;
    if !registry.exists() {
        // An installation that has opened nothing still serves — it just serves
        // nothing yet. Starting without the file would have the server exit
        // over a file the first opened project is about to write.
        write_configuration(&registry, &Vec::<()>::new())?;
    }

    // The second door, on the same process and the same open projects. Asked
    // for rather than assumed by the binary, so a `sync-mcp` started by hand to
    // serve agents is still exactly that.
    let socket = host_socket(app)?;
    let mut child = Command::new(binary(app)?)
        .arg("--registry")
        .arg(&registry)
        .arg("--http")
        .arg(settings.address().to_string())
        .arg("--socket")
        .arg(&socket)
        // The leash. See [`RunningServer::leash`].
        .arg("--exit-when-orphaned")
        .env(sync_memory::SERVER_TOKEN_VARIABLE, &settings.token)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(log(app))
        .spawn()
        .map_err(|error| {
            ProjectError::new(
                "server_failed",
                format!("the MCP server could not be started: {error}"),
            )
        })?;

    let leash = child.stdin.take();
    if let Ok(mut held) = running.leash.lock() {
        *held = leash;
    }
    if let Ok(mut held) = running.child.lock() {
        *held = Some(child);
    }

    // Spawned is not started. Waited for here rather than discovered by the
    // first project that fails to open: a process that exits a second after
    // spawning — a port it cannot have, a socket somebody else is serving — is
    // indistinguishable from a good start until somebody asks it something.
    let deadline = std::time::Instant::now() + OPENING;
    while !answering(&socket) {
        if std::time::Instant::now() >= deadline {
            stop(running);
            return Err(ProjectError::new(
                "server_failed",
                format!(
                    "the memory engine started but did not open {} within {} seconds — its log is in the application's log directory",
                    socket.display(),
                    OPENING.as_secs()
                ),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    remember(running, &socket);
    Ok(())
}

/// End whatever is still serving the door, so this application can have it.
///
/// The engine writes its own process id beside its socket, because a socket
/// says that somebody is there and never who. Verified before it is acted on —
/// a process id is reused, and a stale file naming one that now belongs to
/// somebody's editor must not be a signal to end their editor.
///
/// Quiet throughout. Every failure here ends the same way: the bind that
/// follows refuses and says what is in the way, which is a better sentence than
/// anything this function could produce on its own.
fn displace(socket: &Path) {
    let pid_file = socket.with_extension("pid");
    let Ok(text) = std::fs::read_to_string(&pid_file) else {
        return;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return;
    };
    if !serves_this_socket(pid, socket) {
        // Somebody else's process, or none. The file is this application's to
        // tidy either way: left behind, it names something a later start would
        // have to check all over again.
        let _ = std::fs::remove_file(&pid_file);
        return;
    }
    let _ = Command::new("kill").arg(pid.to_string()).status();
    // Given a moment to go, because the bind that follows needs the door free
    // and a signal is a request rather than an event.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while serves_this_socket(pid, socket) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&pid_file);
}

/// Whether `pid` is a `sync-mcp` serving exactly this socket.
///
/// Read off the process's own command line rather than assumed from the file:
/// the question is not "is something alive with this number" but "is the thing
/// alive with this number the one that wrote this file".
///
/// `ps` rather than a crate, for the reason `git` is a command line here: it is
/// one process answering exactly what a person would type, and a linked library
/// would be a build dependency bought for one question.
fn serves_this_socket(pid: u32, socket: &Path) -> bool {
    let Ok(listed) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
    else {
        return false;
    };
    let said = String::from_utf8_lossy(&listed.stdout);
    said.contains("sync-mcp") && said.contains(&*socket.to_string_lossy())
}

fn remember(running: &RunningServer, socket: &Path) {
    if let Ok(mut held) = running.socket.lock() {
        *held = Some(socket.to_owned());
    }
}

/// Stop the server, if one is running.
pub fn stop(running: &RunningServer) {
    // The leash first: dropping the pipe is what tells an engine this process
    // has finished with it, and it is the half that works when the kill below
    // does not — a child that has already been reparented is nobody's to kill.
    if let Ok(mut held) = running.leash.lock() {
        held.take();
    }
    if let Ok(mut held) = running.child.lock()
        && let Some(mut child) = held.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
    // Cleared with the process. A path left behind would have the window go on
    // dialling a door that nobody is answering, and read the silence as its own
    // failure rather than as there being no server.
    if let Ok(mut held) = running.socket.lock() {
        *held = None;
    }
}

/// Start the server again — after the registry changed, or the port did.
///
/// # Errors
///
/// Reports whatever starting it refused.
#[tauri::command(async)]
pub fn server_restart<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, RunningServer>,
) -> Result<ServerStatus, ProjectError> {
    let failure = start(&app, &running).err();
    Ok(status_of(&app, &running, failure))
}

/// What the server is doing, and how to reach it.
#[tauri::command(async)]
pub fn server_status<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, RunningServer>,
) -> ServerStatus {
    status_of(&app, &running, None)
}

/// Listen on a different port, and start again there.
///
/// # Errors
///
/// Reports whatever starting it on the new port refused. The port is written
/// either way: a port that could not be listened on is still the port the
/// person chose, and forgetting it would hide the mistake rather than let them
/// correct it.
#[tauri::command(async)]
pub fn server_set_port<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, RunningServer>,
    port: u16,
) -> Result<ServerStatus, ProjectError> {
    let mut settings = ServerSettings::load(&app);
    settings.port = port;
    let path = configuration_file(&app, SETTINGS_FILE)?;
    write_configuration(&path, &settings)?;
    restrict(&path);
    server_restart(app, running)
}

/// Mint a new token, and start again with it.
///
/// Every agent connected to this server stops being able to reach it until it
/// is connected again — which is what revoking a token means, and why it is a
/// gesture rather than something that happens on its own.
///
/// # Errors
///
/// Reports whatever starting it refused.
#[tauri::command(async)]
pub fn server_new_token<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, RunningServer>,
) -> Result<ServerStatus, ProjectError> {
    let mut settings = ServerSettings::load(&app);
    settings.token = minted_token();
    let path = configuration_file(&app, SETTINGS_FILE)?;
    write_configuration(&path, &settings)?;
    restrict(&path);
    server_restart(app, running)
}

fn status_of<R: Runtime>(
    app: &AppHandle<R>,
    running: &RunningServer,
    failure: Option<ProjectError>,
) -> ServerStatus {
    let settings = ServerSettings::load(app);
    // **Asked of the door, not of the child.** Two facts became separate the day
    // the process gained a second door: it now survives a port it cannot have,
    // because the window's memory must not depend on whatever else is listening
    // on 41847. So a live child says nothing about whether an agent can reach
    // this machine, and this question is only ever asked about agents — the
    // settings window shows it beside the address they would connect to.
    let running_for_agents = answering_agents(app);
    let _ = running;
    ServerStatus {
        port: settings.port,
        token: settings.token.clone(),
        url: settings.url(),
        running: running_for_agents,
        failure: failure.map(|error| error.message),
    }
}

/// Where the sidecar is, beside the application that bundles it.
fn binary<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, ProjectError> {
    if let Some(named) = std::env::var_os(crate::memory::BINARY_OVERRIDE) {
        return Ok(PathBuf::from(named));
    }
    let _ = app;
    std::env::current_exe()
        .ok()
        .and_then(|executable| {
            executable
                .parent()
                .map(|directory| directory.join(crate::memory::BUNDLED_BINARY))
        })
        .ok_or_else(|| {
            ProjectError::new(
                "server_failed",
                "the application executable has no directory to find the server beside".to_owned(),
            )
        })
}

/// Where the server's own complaints go.
fn log<R: Runtime>(app: &AppHandle<R>) -> std::process::Stdio {
    // Dropped rather than kept when the log cannot be opened: a server that
    // refused to start because its log file was unavailable would be a server
    // held hostage by its own diagnostics.
    app.path()
        .app_log_dir()
        .ok()
        .and_then(|directory| {
            std::fs::create_dir_all(&directory).ok()?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("mcp-server.log"))
                .ok()
        })
        .map_or_else(std::process::Stdio::null, std::process::Stdio::from)
}

/// Keep the file holding the token to this person.
///
/// A token readable by every account on the machine is a token, but not a
/// secret. Best effort: a filesystem that has no opinion about permissions is
/// not a reason to refuse to run.
fn restrict(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}
