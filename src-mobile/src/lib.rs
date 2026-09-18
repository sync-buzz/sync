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
use tauri::{Emitter as _, Manager as _, State};

use channel::{Channel, Pairing, Reach, Trouble, kept};
use sync_vault::{Slot, Vault};

/// Where this phone stands with its computer has moved.
///
/// Pushed rather than polled, and the reason is the keeper: a phone now dials
/// on its own, so the state changes at moments the window had no reason to ask
/// about. A screen that only asked when it was opened would show *cannot reach
/// your computer* over a connection that had come back thirty seconds ago.
///
/// **It carries nothing, the way `attention://shown` carries nothing.** A
/// payload would be a second place the state arrives from, and the window would
/// have to decide which of the two is newer every time a command's answer and
/// an event crossed — a question with no answer at this end, since neither
/// carries when it was true. Told only that something moved, the window asks,
/// and what it is holding is always one whole answer from one moment.
const REACHED: &str = "channel://reach";

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

/// Where the project this phone was last looking at is written down.
///
/// **Beside the pairing because the webview reloads without being asked to.**
/// On a Mac the window is the application and a reload is somebody pressing a
/// key; here the system does it — a phone comes back from the background, the
/// content process is reclaimed — and everything React was holding goes with
/// it. Which project a person had open is the one piece of that they would
/// notice, because losing it puts them back at a list they chose from an hour
/// ago.
///
/// In the same store as the pairing, and the store is the keychain. Not because
/// a project's key is a secret — it is not — but because a phone has one place
/// to write anything durably, and a second mechanism for one short string is a
/// second thing to keep working across an upgrade. What it costs is a line in
/// the keychain; what a file beside the application would cost is finding out,
/// in a year, that the two are backed up under different rules.
const PLACE: &str = "place";

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

    // Which cryptography this process uses, said before anything can ask.
    //
    // `rustls` takes a provider from the process rather than from the caller,
    // and the one in this tree is built with none installed: the desktop gets
    // one for free because something in its dependencies asks for `aws-lc-rs`,
    // and nothing here does. The first HTTPS client built then panics instead
    // of failing — and it is built deep inside the transport, on a thread with
    // nothing above it to report, so what a person sees is the application
    // disappearing at launch with the system log holding the only sentence
    // about why.
    //
    // Worse, *when* it is built depends on the network: a phone that reaches
    // its computer directly never needs a relay and never builds one. So this
    // is not a line that can be left until it is observed to be missing — it
    // is missing on somebody else's network, not on this desk.
    //
    // The result is deliberately dropped. It is an error only if a provider
    // was already installed, which is the state this line wants.
    let _ = rustls::crypto::ring::default_provider().install_default();

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
            // The screen the window was given, in full. See `fill_the_screen`.
            #[cfg(target_os = "ios")]
            keep_the_screen(app.handle());

            // Said before anything can change, so that the first transition is
            // not the one nobody hears.
            let telling = app.handle().clone();
            app.state::<Channel>().tell(move |_: &Reach| {
                drop(telling.emit(REACHED, ()));
            });

            if let Some(pairing) = remembered() {
                app.state::<Channel>().hold(&pairing);
            }

            // The keeper, and it runs whether or not this phone is paired: the
            // pairing may arrive a minute from now from somebody's camera, and
            // a keeper started only for a phone that already had one would
            // leave that phone with no connection of its own after the first
            // one dropped.
            //
            // It never returns, which is why the handle is moved into it rather
            // than borrowed: the state it holds lives exactly as long as the
            // loop reading it.
            let keeping = app.handle().clone();
            std::thread::spawn(move || {
                kept(&keeping.state::<Channel>());
            });
            Ok(())
        })
        // Two lists, because they are two different kinds of command. These
        // four are about *this phone* — whether it has a computer and which one
        // — and are the only ones that can be answered before it has one. The
        // rest are the window's own, and every one of them is a question for
        // the computer.
        .invoke_handler(window::commands![
            channel_status,
            channel_reach_now,
            channel_pair,
            channel_pair_by_hand,
            channel_forget,
            place_held,
            place_hold,
            haptic,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not be opened");
}

/// Say it once now and again every time the system lays the window out.
///
/// Once is not enough, and the failure is silent in both directions. At setup
/// the view may not be in a window yet, and `fill_the_screen` answers that by
/// doing nothing — there is no screen to be measured against. Later, UIKit
/// lays the window out again whenever it changes size, and a frame set by hand
/// is a frame the next layout pass is free to put back inside the safe area.
///
/// What that looked like is worth writing down, because it reads as a design
/// decision rather than as a fault: a bar at the foot of a screen standing a
/// finger's width above the bottom of the phone, over a band of nothing, with
/// the interface looking like a page that had been cut off short.
///
/// Resizing the *webview* is not resizing the window, so this cannot chase its
/// own tail — nothing here emits the event that brings it back.
#[cfg(target_os = "ios")]
fn keep_the_screen<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    fill_the_screen(app);
    silence_the_keyboard_bar(app);

    // And again on the next turn of the main loop, which is the first moment
    // the window is certainly on the screen. Setup runs while it is being
    // built, and a view with no window has no bounds to be given: that call is
    // the one that quietly does nothing, and this is the one that lands.
    let soon = app.clone();
    let _ = app.run_on_main_thread(move || {
        fill_the_screen(&soon);
        silence_the_keyboard_bar(&soon);
    });

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Resized(_)) {
            fill_the_screen(&handle);
            silence_the_keyboard_bar(&handle);
        }
    });
}

/// Give the webview the whole screen, rather than the part inside the notch.
///
/// A phone's window is not a Mac's. `tao` reports an iOS window's inner size as
/// its safe area, and `wry` builds the webview from the view it is handed — so
/// the document is laid out inside a box that stops short of the hardware, and
/// what is left over is a band of empty grey along the foot that no CSS can
/// reach. `100dvh` measures the short box; `viewport-fit=cover` describes a
/// viewport the webview does not have.
///
/// So it is said in UIKit, once, where the frame actually lives: the webview
/// takes the window's bounds and keeps them through a rotation. The insets
/// themselves are not lost — the document reads them through `env()` and keeps
/// its own head and foot clear, which is what makes this a correction of the
/// frame rather than a decision about the layout.
///
/// `contentInsetAdjustmentBehavior` goes with it: left alone, the scroll view
/// adds the same inset a second time, and the band comes back half as tall.
#[cfg(target_os = "ios")]
fn fill_the_screen<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_core_foundation::CGRect;
    use tauri::Manager as _;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    // Errors here are not worth a word to anybody: a window that could not be
    // reached is a window that is not on the screen, and this is about how one
    // that *is* on the screen is proportioned.
    drop(window.with_webview(|platform| unsafe {
        let webview: *mut AnyObject = platform.inner().cast();
        let screen: *mut AnyObject = msg_send![webview, window];
        if screen.is_null() {
            return;
        }
        let bounds: CGRect = msg_send![screen, bounds];
        let container: *mut AnyObject = msg_send![webview, superview];
        let frame: CGRect = if container.is_null() {
            bounds
        } else {
            msg_send![container, convertRect: bounds, fromView: screen]
        };
        let _: () = msg_send![webview, setFrame: frame];
        // Flexible width and height, so a rotation does not undo this.
        let _: () = msg_send![webview, setAutoresizingMask: 2_usize | 16_usize];
        let scroller: *mut AnyObject = msg_send![webview, scrollView];
        if !scroller.is_null() {
            // Never: the document already accounts for the hardware itself.
            let _: () = msg_send![scroller, setContentInsetAdjustmentBehavior: 2_isize];
        }
    }));
}

/// Take away the bar iOS hangs over the keyboard.
///
/// Any text field inside a `WKWebView` gets a strip above the keyboard with the
/// system's own controls on it — previous, next, and the words it is offering.
/// It belongs to the browser it was designed for: a page of forms somebody
/// tabs through. Here there is one field on the screen at a time, the window
/// is not a page, and the strip covers the bottom of the conversation while
/// somebody is typing into it.
///
/// It cannot be reached from the document. The strip is not part of the page —
/// no CSS describes it and no event announces it — so it is taken away in
/// UIKit, where it is owned.
///
/// **Done by giving one view a subclass rather than by patching a class.** The
/// view that owns the strip is `WKContentView`, which is private and shared by
/// every webview in the process; replacing its method would reach into any
/// other webview this application ever creates. A subclass built once and
/// given to *this* view says the same thing about this window alone, and a
/// second call finds the class already in place and leaves it there.
#[cfg(target_os = "ios")]
fn silence_the_keyboard_bar<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
    use objc2::{msg_send, sel};
    use tauri::Manager as _;

    /// What the view answers when it is asked for its bar.
    ///
    /// The receiver is a raw pointer rather than a reference, and that is the
    /// compiler's requirement rather than a preference: a reference carries a
    /// lifetime, and the trait that accepts a method implementation cannot be
    /// satisfied by a function that is generic over one.
    extern "C" fn nothing(_this: *mut AnyObject, _cmd: Sel) -> *mut AnyObject {
        std::ptr::null_mut()
    }

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    drop(window.with_webview(|platform| unsafe {
        let webview: *mut AnyObject = platform.inner().cast();
        let scroller: *mut AnyObject = msg_send![webview, scrollView];
        if scroller.is_null() {
            return;
        }

        // The content view is a subview of the scroller and is named for what
        // it is. Found by name rather than by position, because the order of
        // those subviews is the framework's business and has changed before;
        // found by *substring*, because the name has carried a suffix in some
        // releases.
        let subviews: *mut AnyObject = msg_send![scroller, subviews];
        let count: usize = msg_send![subviews, count];
        let mut content: *mut AnyObject = std::ptr::null_mut();
        for index in 0..count {
            let view: *mut AnyObject = msg_send![subviews, objectAtIndex: index];
            if view.is_null() {
                continue;
            }
            let name = (*view).class().name().to_string_lossy();
            if name.contains("ContentView") {
                content = view;
                break;
            }
        }
        if content.is_null() {
            return;
        }

        let held = (*content).class();
        // Already ours, from an earlier call: nothing to do, and re-deriving a
        // subclass from a subclass is how a chain of them ends up on one view.
        if held.name().to_bytes() == QUIET_CONTENT_VIEW.to_bytes() {
            return;
        }

        let class = match AnyClass::get(QUIET_CONTENT_VIEW) {
            Some(existing) => existing,
            None => {
                let Some(mut building) = ClassBuilder::new(QUIET_CONTENT_VIEW, held) else {
                    return;
                };
                building.add_method(
                    sel!(inputAccessoryView),
                    nothing as extern "C" fn(*mut AnyObject, Sel) -> *mut AnyObject,
                );
                building.register()
            }
        };

        objc2::ffi::object_setClass(content, class);
    }));
}

/// The name the subclass above is registered under, in both places that need it.
#[cfg(target_os = "ios")]
const QUIET_CONTENT_VIEW: &std::ffi::CStr = c"SyncContentViewWithoutAccessoryBar";

/// Whether this phone has a computer, and where it stands with it.
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
    standing(&channel)
}

/// Dial now rather than at the end of the wait the keeper is in.
///
/// Two presses, and neither of them is a person asking for a *connection* —
/// they have one or they do not, and the keeper is already working on it. What
/// this shortens is the wait: *Try again* is somebody saying they have fixed
/// whatever it was, and the window says the same thing when the system hands
/// this application back to somebody, because a phone coming out of a pocket
/// should not spend the rest of a half-minute rest showing a screen about a
/// network that came back while it was asleep.
///
/// It answers with the status rather than with nothing, so a window that
/// pressed it is not left waiting for an event to learn that the dial has
/// begun.
#[tauri::command(async)]
fn channel_reach_now(channel: State<'_, Channel>) -> Value {
    channel.reach_soon();
    standing(&channel)
}

/// What this phone has, and where it stands with it, as one answer.
///
/// The reach is spelled by [`Reach::told`] rather than here, because the same
/// two members go out on [`REACHED`] and two spellings of one shape is how the
/// pushed one comes to be missing a member the answered one has.
fn standing(channel: &Channel) -> Value {
    let paired = channel.pairing().or_else(remembered);
    let mut said = channel.reach_now().told();
    if let Some(members) = said.as_object_mut() {
        members.insert("paired".to_owned(), json!(paired.is_some()));
        members.insert(
            "endpoint".to_owned(),
            json!(paired.map(|pairing| pairing.endpoint)),
        );
    }
    said
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
    if let Ok(vault) = Vault::system() {
        // And where this phone was in it. A project key belongs to the computer
        // that holds it, so one kept across a re-pairing would send the next
        // launch looking for a project on a machine that may never have heard
        // of it.
        for named in [PAIRING, PLACE] {
            if let Ok(slot) = Slot::new(KEEPER, named) {
                drop(vault.forget(&slot));
            }
        }
    }
    channel_status(channel)
}

/// The project this phone was last looking at, if it was looking at one.
///
/// Answered before anything has been dialled, which is what makes it usable at
/// launch: it is a fact about this phone rather than about the computer, and a
/// window that had to reach a computer to find out where it was would show the
/// list of projects every time the network was slow.
///
/// The window still opens the project by asking the computer about it. This is
/// only the key — the name, what it declares and everything else is read the
/// same way it is read when somebody taps a row, so a project renamed or
/// removed while this phone was away is answered by the computer rather than by
/// a stale copy here.
#[tauri::command(async)]
fn place_held() -> Option<String> {
    let vault = Vault::system().ok()?;
    let slot = Slot::new(KEEPER, PLACE).ok()?;
    let held = vault.read(&slot).ok()?;
    (!held.is_empty()).then_some(held)
}

/// Write down where this phone is, or that it is nowhere.
///
/// Best effort in both directions, and deliberately: a keychain that would not
/// answer costs somebody one tap after a reload, and refusing to open a project
/// over it would cost them the project.
#[tauri::command(async)]
fn place_hold(project: Option<String>) {
    let Ok(vault) = Vault::system() else {
        return;
    };
    let Ok(slot) = Slot::new(KEEPER, PLACE) else {
        return;
    };
    match project {
        Some(project) => drop(vault.write(&slot, &project)),
        None => drop(vault.forget(&slot)),
    }
}

/// A tap the hardware makes, for a gesture the person made.
///
/// Nothing on a Mac, which has no haptic engine. On the phone this is the
/// platform's own feedback for selection — the same feel the system gives a
/// picker landing on a row — and one kind rather than two because the
/// difference between switching a section and moving between columns is not a
/// difference a finger asks for.
///
/// `run_on_main_thread` because UIKit's feedback generators answer on the main
/// thread, and a command's body runs on a worker.
#[tauri::command(async)]
fn haptic(app: tauri::AppHandle) {
    #[cfg(target_os = "ios")]
    {
        let _ = app.run_on_main_thread(selection_tap);
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = app;
    }
}

/// Fire a selection feedback generator.
///
/// `UISelectionFeedbackGenerator` is in UIKit, already linked, and is the one
/// the system itself uses for tab changes and picker landings. `Retained`
/// releases the generator when it drops, so nothing accumulates across the
/// calls a session makes.
#[cfg(target_os = "ios")]
fn selection_tap() {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};

    let Some(cls) = AnyClass::get(c"UISelectionFeedbackGenerator") else {
        return;
    };
    unsafe {
        let generator: Retained<AnyObject> = msg_send![cls, new];
        let _: () = msg_send![&generator, prepare];
        let _: () = msg_send![&generator, selectionChanged];
    }
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
