//! Who this machine lets in from somewhere else, and what it is called there.
//!
//! The door itself is in the engine — it is the engine's dispatcher a device
//! reaches, over the engine's own transport. What is here is everything that
//! decides *who may*: the identity this machine is known by, the devices a
//! person has paired, and the secret each of them holds.
//!
//! # Why the secrets live here and not there
//!
//! Because `src-tauri/src/vault.rs` is the only module in this tree that opens
//! the keychain, and that is a rule rather than an accident: *how* a secret is
//! kept and *who may ask for one* are two questions, and the second is answered
//! where the application knows who is calling. An engine that read these itself
//! would be a second answer to the second question, in a process whose whole
//! point is that it cannot see the application.
//!
//! It would cost something visible too. macOS decides who may read an entry
//! from the signature on the program asking, and a build from source carries
//! none — so a sidecar reaching for these would put a password dialog in front
//! of anybody running `tauri dev`, once per start, for a secret this
//! application had just written.
//!
//! So the application reads them and states them over the host channel, and the
//! engine holds them in memory for as long as it is up.
//!
//! That is also what makes revoking one an act rather than an edit. The set is
//! replaced whole on every change, so a device stops being admitted at the next
//! connection — there is no file for a stale copy to survive in.
//!
//! # Where *when it was last here* comes from
//!
//! The engine sees the connection, so the time is its; the list somebody reads
//! is this side's, so keeping it is this side's. It travels back on the same
//! call that states the set, and it is only ever moved forward — a restarted
//! engine holding nothing must not empty a column somebody was reading.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sync_vault::{Slot, Vault, VaultError};
use tauri::{AppHandle, Runtime, State};

use crate::project::{ProjectError, configuration_file, write_configuration};
use crate::server::RunningServer;

/// What this installation's remote access is called in its own configuration.
///
/// Its own file rather than a section of the server's: that one is the port and
/// the token every agent on this machine is configured with, and this is the
/// answer to a different question — who may talk to this machine at all. A
/// person looking for the second would not find it under the first.
const SETTINGS_FILE: &str = "remote-access.json";

/// The half of a keychain entry's name that says these are Sync's own.
///
/// Not a package id, and it cannot collide with one: a package's namespace is
/// the id resolved against the extension store, and nothing is installed under
/// a name with a space in it.
const OWNER: &str = "remote access";

/// What this machine's own identity is called in the keychain.
const IDENTITY: &str = "this machine";

/// How long a fingerprint is.
///
/// Enough that two devices do not collide, short enough to be read out loud
/// when somebody is deciding which of two phones to revoke.
const FINGERPRINT: usize = 12;

/// One device somebody paired, as this machine remembers it.
///
/// The secret is not here and never is: this is the record, and what it is a
/// record *of* is in the keychain under the fingerprint below.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// What the person called it. Theirs, and never matched on.
    pub name: String,
    /// What names this device's entry in the keychain, and what a person
    /// revokes by. Minted with the secret and derived from nothing about it.
    pub fingerprint: String,
    /// Seconds since the epoch. Formatted by whoever shows it — a machine that
    /// wrote a formatted date into its own configuration would be a machine
    /// that has to parse one back.
    pub paired_at: u64,
    /// When this device last came in, or nothing if it never has.
    ///
    /// The engine is what sees a connection, and it holds this in memory only.
    /// Kept here as well, and only ever moved forward, because an engine that
    /// restarted would otherwise have a person's list forget every device had
    /// ever been here — which reads as *it stopped working* rather than as
    /// *this process is new*.
    #[serde(default)]
    pub last_seen: Option<u64>,
}

/// What this installation has decided about being reachable.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAccess {
    /// Off until somebody says otherwise. A machine that arrived reachable
    /// would be a machine nobody agreed to make reachable.
    pub enabled: bool,
    pub devices: Vec<Device>,
}

impl RemoteAccess {
    fn load<R: Runtime>(app: &AppHandle<R>) -> Self {
        configuration_file(app, SETTINGS_FILE)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), ProjectError> {
        let path = configuration_file(app, SETTINGS_FILE)?;
        write_configuration(&path, self)
    }
}

/// What the settings window shows about remote access.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    /// What a device dials to reach this machine, once the engine has a door
    /// open. `None` while remote access is off, and also for the moment
    /// between turning it on and the endpoint binding.
    pub endpoint: Option<String>,
    pub devices: Vec<Device>,
    /// Why the engine is not holding what this side believes it sent.
    pub failure: Option<String>,
}

/// A device that has just been paired, and the one time its secret is legible.
///
/// The secret is answered here and nowhere else, ever again: it goes to the
/// device being paired and then exists only in the keychain. A command that
/// could read one back would be a command that turns the window into a way of
/// exporting every device's key.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paired {
    pub device: Device,
    pub secret: String,
    pub endpoint: Option<String>,
    /// The address and the key as one thing, for the window to draw as a code
    /// the device reads with its camera.
    ///
    /// Composed here rather than in the window because the device parses it
    /// with the same function, out of the same crate — a format spelled twice
    /// is a format that gets changed once. `None` where this machine has no
    /// address yet: half a payload would send somebody's camera at a code that
    /// cannot work.
    pub pairing: Option<String>,
}

/// What remote access is doing, and who is admitted to it.
///
/// # Errors
///
/// Reports whatever reading the configuration refused.
#[tauri::command(async)]
pub async fn remote_status<R: Runtime>(app: AppHandle<R>) -> Result<RemoteStatus, ProjectError> {
    on_the_pool(move || {
        let held = RemoteAccess::load(&app);
        Ok(announced(&app, &held))
    })
    .await
}

/// Turn remote access on or off.
///
/// Restarting the engine is the whole of turning it on: the identity travels in
/// the environment the process is started with, for the reason the bearer token
/// does, and a process cannot be handed one afterwards. Off is the same act in
/// reverse — the engine comes back without an identity and binds nothing.
///
/// # Errors
///
/// Reports a keychain that would not hold the identity, or an engine that would
/// not start again.
#[tauri::command(async)]
pub async fn remote_enable<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, RunningServer>,
    enabled: bool,
) -> Result<RemoteStatus, ProjectError> {
    let mut held = RemoteAccess::load(&app);
    held.enabled = enabled;
    if enabled {
        // Minted before the engine is asked to start rather than by it: the
        // engine cannot write the keychain, so an identity invented there would
        // be a new one every restart and every paired device would be dialling
        // a machine that no longer answers to that name.
        identity(&app)?;
    }
    held.save(&app)?;
    crate::server::start(&app, &running)?;
    Ok(announced(&app, &held))
}

/// Pair a device, and answer with the secret it is to hold.
///
/// # Errors
///
/// Reports a keychain that would not take the secret. The device is recorded
/// only once the secret is stored: a record with nothing behind it would be a
/// device in the list that can never connect and cannot be told apart from one
/// that can.
#[tauri::command(async)]
pub async fn remote_pair<R: Runtime>(
    app: AppHandle<R>,
    name: String,
) -> Result<Paired, ProjectError> {
    on_the_pool(move || paired(&app, &name)).await
}

fn paired<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<Paired, ProjectError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(ProjectError::new(
            "remote_failed",
            "a device is paired under a name, so that it can be told from the others when one of \
             them is to be revoked"
                .to_owned(),
        ));
    }
    let secret = minted(32);
    let fingerprint = minted(FINGERPRINT / 2);
    keychain(app)?
        .write(&slot(&fingerprint)?, &secret)
        .map_err(refused)?;

    let mut held = RemoteAccess::load(app);
    let device = Device {
        name,
        fingerprint,
        paired_at: now(),
        last_seen: None,
    };
    held.devices.push(device.clone());
    held.save(app)?;

    let stated = announced(app, &held);
    Ok(Paired {
        pairing: stated
            .endpoint
            .as_ref()
            .map(|endpoint| sync_memory::pairing::pairing(endpoint, &secret)),
        device,
        secret,
        endpoint: stated.endpoint,
    })
}

/// Stop admitting a device.
///
/// The record goes and so does the secret, and the engine is told the set
/// without it. Not a new identity for this machine: a phone somebody left in a
/// taxi must not cost them every other device they have paired.
///
/// # Errors
///
/// Reports a keychain that would not let go of the secret. The record is kept
/// in that case rather than removed, because a list that no longer shows a
/// device whose secret is still admitted is the worse of the two states.
#[tauri::command(async)]
pub async fn remote_revoke<R: Runtime>(
    app: AppHandle<R>,
    fingerprint: String,
) -> Result<RemoteStatus, ProjectError> {
    on_the_pool(move || revoked(&app, &fingerprint)).await
}

fn revoked<R: Runtime>(
    app: &AppHandle<R>,
    fingerprint: &str,
) -> Result<RemoteStatus, ProjectError> {
    let vault = keychain(app)?;
    match vault.forget(&slot(fingerprint)?) {
        // Already gone is the outcome asked for. Somebody deleting the entry in
        // Keychain Access is not a reason to refuse to tidy the list.
        Ok(()) | Err(VaultError::Missing) => {}
        Err(error) => return Err(refused(error)),
    }
    let mut held = RemoteAccess::load(app);
    held.devices
        .retain(|device| device.fingerprint != fingerprint);
    held.save(app)?;
    Ok(announced(app, &held))
}

/// Run work that waits on somebody off the pool the window shares.
///
/// Everything here waits on something with a person or another process at the
/// far end: the keychain, which may put a dialog up on an unsigned build, and
/// the engine, which may be starting. `vault.rs` puts the same kind of work on
/// the same pool for the same reason, and it is the reason rather than the
/// habit that matters — a call that may sit for twenty seconds must not sit on
/// a worker the rest of the application is sharing.
///
/// [`remote_enable`] is the exception and stays where it is: it needs the
/// running server out of managed state, which is a borrow and cannot cross onto
/// a thread. It is the same arrangement `server_restart` already has, and
/// making one of the two different would be the surprise.
async fn on_the_pool<T, F>(work: F) -> Result<T, ProjectError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProjectError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| {
            ProjectError::new("remote_failed", format!("the work did not run: {error}"))
        })?
}

/// The identity this machine is known by, minted the first time it is asked
/// for.
///
/// Thirty-two bytes in hex — what an `iroh` endpoint is built from, and
/// therefore what a device dials. Kept for the life of the installation: it is
/// the name every paired device holds, so minting a new one would silently
/// revoke all of them.
///
/// # Errors
///
/// Reports a keychain that would not hold it.
pub(crate) fn identity<R: Runtime>(app: &AppHandle<R>) -> Result<String, ProjectError> {
    let vault = keychain(app)?;
    let slot = slot(IDENTITY)?;
    match vault.read(&slot) {
        Ok(held) => Ok(held),
        Err(VaultError::Missing) => {
            let minted = minted(32);
            vault.write(&slot, &minted).map_err(refused)?;
            Ok(minted)
        }
        Err(error) => Err(refused(error)),
    }
}

/// The identity to start the engine with, or nothing where the person has not
/// asked to be reachable.
///
/// Quiet about a keychain that refused: the engine starting without a door is
/// the same outcome as remote access being off, and the settings section says
/// which it is by asking for the identity itself.
pub(crate) fn identity_for_engine<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    if !RemoteAccess::load(app).enabled {
        return None;
    }
    match identity(app) {
        Ok(key) => Some(key),
        Err(error) => {
            eprintln!("remote access is on but has no identity: {}", error.message);
            None
        }
    }
}

/// Tell the engine which secrets to admit, and ask what it is called.
///
/// Called after every change and on every read, and that is deliberate rather
/// than wasteful: the engine holds this set in memory only, so an engine that
/// restarted under a window that did not notice is holding nothing. Stating it
/// again on the way to reading the status is what puts the two back in
/// agreement without anybody having to detect that they were not.
pub(crate) fn announce<R: Runtime>(app: &AppHandle<R>) {
    let held = RemoteAccess::load(app);
    let _ = announced(app, &held);
}

fn announced<R: Runtime>(app: &AppHandle<R>, held: &RemoteAccess) -> RemoteStatus {
    let stating = if held.enabled {
        admitted(app, held)
    } else {
        Vec::new()
    };
    let answered = state(app, &stating);

    let mut devices = held.devices.clone();
    let mut moved = false;
    if let Ok(answer) = answered.as_ref() {
        for (fingerprint, seen) in seen_in(answer) {
            if let Some(device) = devices
                .iter_mut()
                .find(|device| device.fingerprint == fingerprint)
                && device.last_seen < Some(seen)
            {
                device.last_seen = Some(seen);
                moved = true;
            }
        }
    }
    if moved {
        // Written back so the column survives the engine, and only where
        // something actually moved: this runs on every read of the status, and
        // a write per read would be a file rewritten while somebody scrolls.
        let _ = RemoteAccess {
            enabled: held.enabled,
            devices: devices.clone(),
        }
        .save(app);
    }

    RemoteStatus {
        enabled: held.enabled,
        endpoint: answered
            .as_ref()
            .ok()
            .and_then(|answer| answer.get("endpoint").and_then(Value::as_str))
            .map(str::to_owned),
        devices,
        failure: answered.err(),
    }
}

/// When the engine says each device was last here.
fn seen_in(answer: &Value) -> Vec<(String, u64)> {
    answer
        .get("devices")
        .and_then(Value::as_array)
        .map(|devices| {
            devices
                .iter()
                .filter_map(|device| {
                    Some((
                        device.get("fingerprint")?.as_str()?.to_owned(),
                        device.get("lastSeen")?.as_u64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The devices this machine admits, each with the secret behind it.
///
/// The fingerprint travels so that the engine can say something about a device
/// without saying its secret back: it is minted beside one and derived from
/// nothing about it, which is what makes it safe to carry in an answer.
///
/// A record whose secret has gone from the keychain is skipped rather than
/// reported: it cannot be admitted anyway, and the honest place to notice it is
/// the list, which shows the record.
fn admitted<R: Runtime>(app: &AppHandle<R>, held: &RemoteAccess) -> Vec<Value> {
    let Ok(vault) = keychain(app) else {
        return Vec::new();
    };
    held.devices
        .iter()
        .filter_map(|device| {
            let slot = slot(&device.fingerprint).ok()?;
            let secret = vault.read(&slot).ok()?;
            Some(json!({"fingerprint": device.fingerprint, "secret": secret}))
        })
        .collect()
}

/// State the set on the host channel and read what came back.
///
/// A connection of its own, opened and closed: this is one message and an
/// answer, and holding a connection for it would be holding one for the times
/// nobody is changing anything.
fn state<R: Runtime>(app: &AppHandle<R>, devices: &[Value]) -> Result<Value, String> {
    let path = crate::server::host_socket(app).map_err(|error| error.message)?;
    let mut stream = UnixStream::connect(&path)
        .map_err(|error| format!("the engine is not answering: {error}"))?;
    // The one wait here with nothing else bounding it. An engine that accepted
    // the connection and then said nothing would otherwise hold this call for
    // as long as the application runs, and what is being waited for is a write
    // into a set held in memory — if it takes seconds, it is not coming.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let asked = json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": sync_memory::REMOTE_DEVICES,
        "params": {"devices": devices},
    });
    writeln!(stream, "{asked}").map_err(|error| format!("the engine did not hear: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("the engine did not hear: {error}"))?;

    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|error| format!("the engine did not answer: {error}"))?;
    let answer: Value = serde_json::from_str(line.trim())
        .map_err(|_| "the engine answered something that is not JSON".to_owned())?;
    if let Some(error) = answer.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the engine refused")
            .to_owned());
    }
    answer
        .get("result")
        .cloned()
        .ok_or_else(|| "the engine answered with neither a result nor a reason".to_owned())
}

/// Bytes of the operating system's randomness, in hex.
///
/// The same reasoning as the server's own token: not a word, not a counter, and
/// not derived from anything about this machine.
fn minted(bytes: usize) -> String {
    let mut held = vec![0_u8; bytes];
    getrandom::fill(&mut held).unwrap_or_else(|_| {
        panic!("the operating system refused randomness for a device's secret")
    });
    held.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn slot(name: &str) -> Result<Slot, ProjectError> {
    Slot::new(OWNER, name).map_err(refused)
}

fn keychain<R: Runtime>(app: &AppHandle<R>) -> Result<Vault, ProjectError> {
    let _ = app;
    Vault::system().map_err(refused)
}

fn refused(error: VaultError) -> ProjectError {
    ProjectError::new("remote_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// Two secrets minted one after the other are not the same secret. Weak as
    /// a test of randomness and not meant as one: what it catches is the
    /// mistake that matters, a constant returned by something that reads like
    /// it mints.
    #[test]
    fn a_minted_secret_is_not_the_last_one() {
        let first = minted(32);
        assert_eq!(first.len(), 64, "thirty-two bytes in hex");
        assert_ne!(first, minted(32));
    }

    /// The fingerprint names the keychain entry, so it has to be a name the
    /// keychain will take — and in particular it must not look like a way into
    /// somebody else's namespace.
    #[test]
    fn a_fingerprint_is_a_name_the_keychain_takes() {
        let fingerprint = minted(FINGERPRINT / 2);
        assert_eq!(fingerprint.len(), FINGERPRINT);
        let slot = slot(&fingerprint).expect("a fingerprint names a slot");
        assert_eq!(slot.owner(), OWNER);
        assert_eq!(slot.name(), fingerprint);
    }

    /// Off with nothing paired is what an installation that has never been
    /// asked about this looks like, and it is what the absence of the file has
    /// to read as.
    #[test]
    fn an_installation_that_was_never_asked_is_off_and_admits_nobody() {
        let held = RemoteAccess::default();
        assert!(!held.enabled);
        assert!(held.devices.is_empty());
    }

    /// The record and the secret are different things kept in different places,
    /// and this is the join between them: what goes over the channel is the
    /// secret, never the record.
    #[test]
    fn a_device_record_carries_no_secret() {
        let device = Device {
            name: "a phone".to_owned(),
            fingerprint: minted(FINGERPRINT / 2),
            paired_at: now(),
            last_seen: None,
        };
        let written = serde_json::to_string(&device).expect("a device is JSON");
        assert!(written.contains("fingerprint"));
        assert!(!written.contains("secret"), "{written}");
    }
}
