//! Sync for iOS.
//!
//! The screen is the same statically exported frontend the desktop window
//! shows; what differs is where a call is executed. On the phone there is no
//! engine to ask, so the answer comes from a computer over the host channel —
//! which is why this crate depends on the channel's vocabulary and on nothing
//! else of Sync.
//!
//! There is no binary target beside this library. A phone has no command line,
//! and the entry point Xcode calls is a C symbol here.

mod channel;
mod packages;
mod window;

use serde_json::{Value, json};
use tauri::{Manager as _, State};

use channel::{Channel, Pairing, Trouble};
use sync_vault::{Slot, Vault};

/// Who the pairing belongs to, in the keychain's two-part name.
///
/// The vault's names are *package* and *what that package calls it*, because on
/// the computer every secret in it belongs to a package. This one belongs to
/// the application itself, and says so rather than borrowing a package's name.
const KEEPER: &str = "sync";

/// What that is: the whole pairing line, address and secret together.
///
/// One entry rather than two, and the line exactly as the code carried it —
/// which means the thing that reads it back is the parser that already has
/// tests, rather than a second reader of a format written down twice.
const PAIRING: &str = "pairing";

/// What this window is running on, said before the document runs.
///
/// The window's code is one export shown by both applications, and nothing in
/// it can tell the two apart by looking: the same document, the same commands,
/// the same Tauri underneath. So the phone says which it is, and the Mac says
/// nothing at all — the absence is the answer for it and for a browser during
/// development alike, and a desktop that has to be edited to describe itself
/// would be a desktop changed for a phone's benefit.
///
/// Absence is safe as a signal here for a reason worth stating: this runs as
/// part of creating the webview, before the document is parsed. A script that
/// did not run is a window that has nothing in it, not a phone quietly
/// claiming to be a Mac.
///
/// A plugin rather than a window built in Rust: the window is declared in
/// `tauri.conf.json` beside everything else that is true of it, and moving it
/// into code to add one line would leave its definition in two places.
///
/// It is said twice, to the two things that ask it differently. The global is
/// for the window's code; the attribute is for the token layer, which is CSS
/// and cannot read a variable. Both are set here rather than one of them from
/// React, and that is the whole point of this running at document start: the
/// exported HTML is painted before any of the window's own code runs, so an
/// attribute applied from an effect would arrive one frame after a phone had
/// already shown a Mac window's inset frame.
const DEVICE: &str = "\
window.__SYNC_DEVICE__ = 'phone';
var mark = function () {
  document.documentElement.setAttribute('data-device', 'phone');
};
// The parser has usually made the root element by now. Where it has not, the
// next state change is still before anything is painted.
if (document.documentElement) mark();
else document.addEventListener('readystatechange', mark, { once: true });
";

/// The application.
///
/// `mobile_entry_point` is what emits `start_app`, the symbol the generated
/// Xcode project calls into. Off iOS the same function is ordinary Rust, so a
/// `cargo check` for the host target still says something about it.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Said once, into the log the simulator and the device both keep. A build
    // installed weeks ago is the ordinary condition of a client that arrives
    // through a store, and the first question about one is always which channel
    // it was built to speak.
    //
    // Written whole rather than with `eprintln!`, and that is measured: iOS
    // reads the process's stderr in whatever pieces it arrives in and files
    // each as its own line, so a formatted message reached the log as
    // "host channel" and, a millisecond later, "1".
    let greeting = format!(
        "Sync for iOS {}, host channel {}\n",
        env!("CARGO_PKG_VERSION"),
        sync_memory::CHANNEL_VERSION
    );
    let _ = std::io::Write::write_all(&mut std::io::stderr(), greeting.as_bytes());

    // The runtime is named rather than inferred: a plugin that registers no
    // command gives the compiler nothing to read it off.
    let builder = tauri::Builder::default().plugin(
        tauri::plugin::Builder::<tauri::Wry>::new("device")
            .js_init_script(DEVICE)
            .build(),
    );

    // Gated by the same words the dependency is, in `Cargo.toml`: the plugin
    // has no half that builds for this machine, so the two gates saying
    // different things is a host build that cannot find the crate it is
    // calling.
    #[cfg(target_os = "ios")]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    builder
        // An extension's own files, fetched from the computer that holds them.
        // The same scheme the window's code already asks for: the computer
        // builds the URL, and what differs is only where the bytes come from.
        .register_asynchronous_uri_scheme_protocol(packages::SCHEME, packages::serve)
        .manage(Channel::default())
        .manage(packages::Served::default())
        .setup(|app| {
            // Dialling takes a relay, a handshake and whatever the network is
            // doing, so it happens beside the window rather than in front of
            // it: the screen comes up saying it is not connected and corrects
            // itself, instead of holding still until it knows.
            // Which computer this is, said before anything is dialled. The
            // window comes up in the same moment and asks it for the projects
            // there are, and a phone that has a computer it has not reached yet
            // must not answer that it has none — that reads as a pairing lost
            // rather than as a connection being made.
            if let Some(pairing) = remembered() {
                app.state::<Channel>().hold(&pairing);
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    drop(handle.state::<Channel>().open(&pairing));
                });
            }
            Ok(())
        })
        // Two lists, because they are two different kinds of command. These
        // four are about *this phone* — whether it has a computer and which one
        // — and are the only ones that can be answered before it has one. The
        // rest are the window's own, and every one of them is a question for
        // the computer.
        .invoke_handler(window::commands![
            channel_status,
            channel_pair,
            channel_pair_by_hand,
            channel_forget,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not be opened");
}

/// Whether this phone has a computer, and whether it is talking to it.
///
/// Two answers rather than one, because they fail differently: a phone that
/// was never paired needs the code on somebody's screen, and a paired phone
/// that cannot reach its computer needs the computer woken up.
///
/// **Every command here is `async`, and none of them is an async function.**
/// That is what the word does in Tauri: the body stays ordinary blocking Rust
/// and runs on a worker instead of on the thread drawing the screen. All of
/// them block — on a keychain, on a network, on a lock — and one of them
/// blocks for as long as a call is given. Without it the window froze for
/// thirty seconds on a computer that did not answer, which iOS itself reported
/// as a gesture recogniser held for 30.6 seconds.
#[tauri::command(async)]
fn channel_status(channel: State<'_, Channel>) -> Value {
    let paired = channel.pairing().or_else(remembered);
    json!({
        "paired": paired.is_some(),
        "endpoint": paired.map(|pairing| pairing.endpoint),
        "connected": channel.open_now(),
    })
}

/// Take what the camera read and, if it is one of ours, dial with it.
#[tauri::command(async)]
fn channel_pair(payload: String, channel: State<'_, Channel>) -> Result<Value, String> {
    pair(payload, channel)
}

/// The same, from the two halves the computer also shows as text.
///
/// The payload is composed here rather than in the window, by the function the
/// computer composes it with: the format is one crate's, and a second speller
/// of it is a format that can disagree with itself. What the window sends is
/// what a person read off the other screen, and nothing about its shape.
#[tauri::command(async)]
fn channel_pair_by_hand(
    endpoint: String,
    secret: String,
    channel: State<'_, Channel>,
) -> Result<Value, String> {
    pair(
        sync_memory::pairing::pairing(endpoint.trim(), secret.trim()),
        channel,
    )
}

/// Dial, and keep the pairing only once the computer has admitted this phone.
///
/// A code that was refused is not a computer this phone has, and remembering it
/// would be remembering a failure to try again at every launch.
fn pair(payload: String, channel: State<'_, Channel>) -> Result<Value, String> {
    let (endpoint, secret) = sync_memory::pairing::paired(&payload)
        .ok_or_else(|| "that code is not a Sync pairing code".to_owned())?;
    let pairing = Pairing { endpoint, secret };
    channel.open(&pairing).map_err(|trouble| trouble.0)?;
    remember(&payload).map_err(|trouble| trouble.0)?;
    Ok(channel_status(channel))
}

/// Forget the computer entirely: the connection and the key with it.
#[tauri::command(async)]
fn channel_forget(channel: State<'_, Channel>) -> Value {
    channel.close();
    if let Ok(vault) = Vault::system()
        && let Ok(slot) = Slot::new(KEEPER, PAIRING)
    {
        drop(vault.forget(&slot));
    }
    channel_status(channel)
}

/// The pairing this phone was given, read back from the keychain.
fn remembered() -> Option<Pairing> {
    let vault = Vault::system().ok()?;
    let slot = Slot::new(KEEPER, PAIRING).ok()?;
    let payload = vault.read(&slot).ok()?;
    let (endpoint, secret) = sync_memory::pairing::paired(&payload)?;
    Some(Pairing { endpoint, secret })
}

/// Keep it where the operating system keeps secrets, and nowhere else.
///
/// Not a file beside the application and not the webview's storage: both are
/// readable by anything that gets at the device's backup, and what is in here
/// is the right to speak to somebody's computer.
fn remember(payload: &str) -> Result<(), Trouble> {
    let vault = Vault::system().map_err(|refusal| Trouble(refusal.to_string()))?;
    let slot = Slot::new(KEEPER, PAIRING).map_err(|refusal| Trouble(refusal.to_string()))?;
    vault
        .write(&slot, payload)
        .map_err(|refusal| Trouble(refusal.to_string()))
}
