//! The Dock icon's menu, which is where a second window is asked for.
//!
//! Holding the Dock icon is how a person on this system opens another window of
//! something — Safari, Finder and Mail all answer there — and Sync has to be in
//! that list because the menu bar is not always an option: the icon can be held
//! from any application, without Sync being the active one first.
//!
//! macOS asks the application delegate for that menu and nothing else can
//! supply it: there is no `setDockMenu`, and the delegate belongs to Tao, which
//! does not implement `applicationDockMenu:`. So the method is added to the
//! delegate's class at launch. Adding rather than replacing is the whole safety
//! of it — `class_addMethod` refuses a selector the class already implements,
//! so the day Tao grows a Dock menu of its own this quietly does nothing rather
//! than overwriting it.
//!
//! The item's target is the delegate too, for the same reason: an object of our
//! own would have to be kept alive for the process's life and reached from a C
//! function anyway, and the delegate already is both.

use std::ffi::CStr;
use std::sync::OnceLock;

use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::ns_string;
use tauri::{AppHandle, Runtime};

/// What the item does, held here because the menu is reached through a C
/// function that can carry nothing with it.
///
/// A closure rather than an `AppHandle`, so that nothing below is generic over
/// the runtime: the handle is the application's own, captured once at launch.
static A_NEW_WINDOW: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Put `New Window` in the Dock icon's menu.
///
/// Called once, at launch and after the event loop exists — the delegate is
/// created with it, and there is nothing to teach before that.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let held = A_NEW_WINDOW.set(Box::new(move || {
        let app = handle.clone();
        // Handed to the event loop rather than built here. This runs inside
        // AppKit's menu tracking, and creating a window from there is asking
        // the main thread for something while standing on it.
        let _ = handle.run_on_main_thread(move || {
            if let Err(error) = crate::windows::open(&app) {
                eprintln!("the Dock could not open a window: {error}");
            }
        });
    }));

    // Installed twice, which is a call that should not have happened rather
    // than a state to recover from.
    if held.is_err() {
        return;
    }

    unsafe { teach_the_delegate() };
}

/// Give the application delegate the two methods the Dock menu is made of.
///
/// # Safety
///
/// Adds methods to the delegate's class, so the implementations below have to
/// match the signatures their type encodings claim.
unsafe fn teach_the_delegate() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("the Dock menu was asked for off the main thread");
        return;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        eprintln!("the Dock menu has nowhere to live: the application has no delegate");
        return;
    };

    let class: *const AnyClass = AsRef::<AnyObject>::as_ref(&*delegate).class();

    // `@@:@` — returns an object, takes self, the selector and the sender;
    // `v@:@` — the same, returning nothing. The encodings are what the runtime
    // dispatches on, so they describe the functions below exactly.
    //
    // SAFETY: each implementation is transmuted from a function with the
    // signature its encoding states.
    let added = unsafe {
        add(
            class,
            sel!(applicationDockMenu:),
            std::mem::transmute::<DockMenu, Imp>(dock_menu),
            c"@@:@",
        ) && add(
            class,
            sel!(syncOpenANewWindow:),
            std::mem::transmute::<Chosen, Imp>(open_a_window),
            c"v@:@",
        )
    };
    if !added {
        eprintln!("the Dock menu could not be added to the application delegate");
    }
}

/// `- (NSMenu *)applicationDockMenu:(NSApplication *)sender`.
type DockMenu = extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> *mut NSMenu;

/// `- (void)syncOpenANewWindow:(id)sender`.
type Chosen = extern "C-unwind" fn(&AnyObject, Sel, &AnyObject);

/// One method onto a class that is already registered.
///
/// # Safety
///
/// `implementation` has to have the signature `types` describes.
unsafe fn add(class: *const AnyClass, selector: Sel, implementation: Imp, types: &CStr) -> bool {
    unsafe { class_addMethod(class.cast_mut(), selector, implementation, types.as_ptr()) }.as_bool()
}

/// The menu itself, built fresh each time the Dock asks for it.
///
/// One item, because the system supplies the rest: the windows that are open,
/// `Show All Windows`, `Options`, `Quit`. What is missing from that list is the
/// one thing an application has to say for itself.
extern "C-unwind" fn dock_menu(this: &AnyObject, _cmd: Sel, _sender: &AnyObject) -> *mut NSMenu {
    // SAFETY: the Dock asks the delegate for its menu on the main thread.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    let menu = NSMenu::new(mtm);
    let item = NSMenuItem::new(mtm);
    item.setTitle(ns_string!("New Window"));
    // SAFETY: the selector is the one added to this class beside this method,
    // and the target is the object it was added to.
    unsafe {
        item.setAction(Some(sel!(syncOpenANewWindow:)));
        item.setTarget(Some(this));
    }
    menu.addItem(&item);

    // Autoreleased: the Dock reads the menu and lets it go, and a menu returned
    // owned would be one leaked on every click.
    Retained::autorelease_return(menu)
}

/// What the item does when it is chosen.
extern "C-unwind" fn open_a_window(_this: &AnyObject, _cmd: Sel, _sender: &AnyObject) {
    if let Some(open) = A_NEW_WINDOW.get() {
        open();
    }
}
