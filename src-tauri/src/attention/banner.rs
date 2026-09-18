//! Putting a banner on the screen, and hearing that somebody clicked it.
//!
//! [`super`] decides what is said and when; this is the platform. The division
//! is the one the repository keeps everywhere else — *who may ask* apart from
//! *how it is done* — and here it earns itself twice over: everything below is
//! unsafe, and everything above it is a rule about people that can be read and
//! tested without a window, a bundle or a notification centre.
//!
//! # Why not the plugin
//!
//! `tauri-plugin-notification` shows a banner and forgets it. Its desktop
//! implementation spawns `notify-rust`'s `show()` and drops the handle, and the
//! handle is the only thing on that path that a click is ever delivered to — so
//! a banner raised through the plugin cannot answer *where does this go*. The
//! payload members its builder carries (`extra`, `action_type_id`) are read on
//! mobile and nowhere else.
//!
//! So the notification is built here instead. `UNUserNotificationCenter` is the
//! interface that carries a dictionary out with the notification and hands it
//! back on the way in, which is the whole of what makes a banner a place rather
//! than a sentence.
//!
//! # It needs a bundle, so a build from source does not have one
//!
//! `UNUserNotificationCenter` reads the running process's bundle identifier and
//! raises an Objective-C exception when there is none — and a `tauri dev` build
//! is a bare executable in `target/debug`. There is nothing to work around: the
//! notification centre is a thing an *application* has. So the identifier is
//! read first, and a process without one falls back to the plugin: banners in
//! development still appear, and clicking one still does nothing, which is the
//! same trap `docs/architecture.md` records for the CSP — the packaged build is
//! where this is verified, never `pnpm tauri dev`.
//!
//! # What is kept alive, and what is not
//!
//! `setDelegate:` is a weak property, so the delegate is deliberately leaked:
//! one object, once, for the life of the process. The alternative is a static
//! holding a `Retained` of a class that is not `Sync`, which buys nothing —
//! nothing ever takes it back.
//!
//! Where it routes to is a closure in a `OnceLock` for the reason
//! [`crate::dock`]'s is: a delegate method is reached by the runtime and can
//! carry nothing of ours with it, and a closure keeps the concrete `AppHandle`
//! out of the signatures below.

use std::path::PathBuf;
use std::sync::OnceLock;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationRequest,
    UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::{Address, Banner};

/// What a click does, set once at launch.
static OPEN: OnceLock<Box<dyn Fn(Address) + Send + Sync>> = OnceLock::new();

/// The members of the dictionary a banner carries and gives back.
///
/// Spelled once and read twice, because the two halves of this file are the
/// only two places in the product that agree on them: a key changed in one and
/// not the other is a click that silently lands nowhere.
const PROJECT: &str = "project";
const CONVERSATION: &str = "conversation";
const RECORD: &str = "record";
const RECORD_KIND: &str = "recordKind";

/// Take the notification centre, and say what a click on one means.
///
/// Called once at launch, before any banner is raised — and before the first
/// one a person clicks, which matters more than it looks: macOS delivers the
/// response of a banner clicked while Sync was closed to the delegate of the
/// application it just launched, and a delegate installed later would be
/// installed after that delivery had already been dropped.
///
/// Nothing is reported when the platform refuses permission. That refusal is a
/// person having turned Sync off in System Settings, which is an answer.
pub fn attend(open: impl Fn(Address) + Send + Sync + 'static) {
    if OPEN.set(Box::new(open)).is_err() {
        // Installed twice, which is a call that should not have been made
        // rather than a state to recover from.
        return;
    }
    let Some(centre) = centre() else {
        return;
    };

    let delegate = Attendant::new();
    centre.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // Weak property, see the module's head.
    std::mem::forget(delegate);

    let answered = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
    centre.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &answered,
    );
}

/// Put one on the screen, and answer whether it went.
///
/// `false` is a process with no bundle to be a notification centre's client,
/// which is the case [`super`] answers with the plugin.
pub fn raise(banner: &Banner, address: &Address) -> bool {
    let Some(centre) = centre() else {
        return false;
    };

    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&banner.title));
    // Left unset rather than set to nothing: a subtitle of no characters is
    // still a subtitle, and the system lays a line out for one.
    if !banner.subtitle.is_empty() {
        content.setSubtitle(&NSString::from_str(&banner.subtitle));
    }
    content.setBody(&NSString::from_str(&banner.body));
    // Grouped by project, which is how Notification Center already draws a
    // stack of anything: three conversations in one project read as that
    // project's pile rather than as three unrelated interruptions.
    content.setThreadIdentifier(&NSString::from_str(&address.project.to_string_lossy()));
    // SAFETY: every value in it is an `NSString`, which is what a notification's
    // payload is required to hold — it is archived, and an object that could not
    // be would be refused at delivery.
    unsafe { content.setUserInfo(&carried(address)) };

    // The conversation's own name, so the second banner about a conversation
    // replaces the first rather than joining it. A person who left the room
    // while an agent was asking and came back after it had finished should find
    // where it got to, not the two states in the order they happened.
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&address.conversation),
        &content,
        None,
    );
    centre.addNotificationRequest_withCompletionHandler(&request, None);
    true
}

/// The dictionary a banner takes with it.
fn carried(address: &Address) -> Retained<NSDictionary> {
    let mut pairs = vec![
        (PROJECT, address.project.to_string_lossy().into_owned()),
        (CONVERSATION, address.conversation.clone()),
    ];
    if let Some(record) = &address.record {
        pairs.push((RECORD, record.key.clone()));
        pairs.push((RECORD_KIND, record.kind.clone()));
    }

    let keys: Vec<_> = pairs
        .iter()
        .map(|(key, _)| NSString::from_str(key))
        .collect();
    let values: Vec<_> = pairs
        .iter()
        .map(|(_, value)| NSString::from_str(value))
        .collect();
    let dictionary = NSDictionary::from_retained_objects(
        &keys.iter().map(|key| &**key).collect::<Vec<_>>(),
        &values,
    );
    // SAFETY: the same dictionary, described by what a notification's payload
    // is declared to hold rather than by what was put in it. Every value is an
    // `NSString`, so nothing here is a claim the objects do not answer.
    unsafe { Retained::cast_unchecked(dictionary) }
}

/// This process's notification centre, or nothing where it has no bundle to be
/// one for. See the module's head.
fn centre() -> Option<Retained<UNUserNotificationCenter>> {
    NSBundle::mainBundle().bundleIdentifier()?;
    Some(UNUserNotificationCenter::currentNotificationCenter())
}

/// Where a click was addressed, read back out of what the banner carried.
fn addressed(response: &UNNotificationResponse) -> Option<Address> {
    let notification: Retained<UNNotification> = response.notification();
    let carried = notification.request().content().userInfo();
    let text = |key: &str| {
        let key = NSString::from_str(key);
        carried
            .objectForKey(AsRef::<AnyObject>::as_ref(&*key))
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|value| value.to_string())
    };

    let project = text(PROJECT)?;
    let conversation = text(CONVERSATION)?;
    Some(Address {
        project: PathBuf::from(project),
        conversation,
        // A conversation about nothing in particular is the ordinary case, so
        // one half of the pair missing is not a banner to refuse: it is one
        // that was never about a record.
        record: text(RECORD)
            .zip(text(RECORD_KIND))
            .map(|(key, kind)| super::AboutRecord { key, kind }),
    })
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - `Attendant` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "SyncNotificationAttendant"]
    #[ivars = ()]
    struct Attendant;

    unsafe impl NSObjectProtocol for Attendant {}

    unsafe impl UNUserNotificationCenterDelegate for Attendant {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn responded(
            &self,
            _centre: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            if let (Some(open), Some(address)) = (OPEN.get(), addressed(response)) {
                open(address);
            }
            // Always, and before anything can return early: the system waits on
            // this block to know the response was handled, and one never called
            // is a delegate the system stops trusting with the next one.
            done.call(());
        }
    }
);

impl Attendant {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}
