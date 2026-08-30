//! Who may ask the keychain for what.
//!
//! The crate beside this one knows how a secret is stored; this decides who is
//! allowed to say so, which is the division the rest of the application already
//! draws between `src-tauri/src/` and a crate. There is one door and this is
//! it: nothing else in the tree opens the store, and the dependency that can is
//! named in one manifest.
//!
//! **Two halves, told apart by whose entry is being asked about.** The person's
//! half is the settings window: listing what is held, putting a secret in,
//! replacing one, taking one out, about entries that may belong to any package
//! at all. It has no read, and that absence is the design — the window never
//! shows a secret, so a command that handed one to it would exist only to be
//! misused.
//!
//! The package's half does read, because a package is what a secret is *for*:
//! it signs a request with one, exchanges one, replaces one when it expires. It
//! reaches a single namespace — the owner is the id resolved against the
//! extension store, never a string the call carried — and it is behind a
//! capability a person was shown on the card before they installed anything.
//! What a package may not do with a value once it holds it is not something
//! this file can enforce, and it is said where an author reads instead:
//! `docs/extensions.md` §4 and the doc comment on `ExtensionVault`.
//!
//! **The store is opened per call rather than held.** Opening it touches
//! nothing — it is a struct describing which keychain to talk to — so a handle
//! kept in application state would buy nothing and would have to be built
//! before the first window, on a platform that may have nowhere to put a
//! secret at all.
//!
//! Every one of these goes to the blocking pool, for the reason the network
//! door does: the work underneath is a synchronous platform call, and one that
//! may sit for twenty seconds behind a dialog is exactly the thing that must
//! not be sitting on an async worker.

use sync_vault::{Persistence, Slot, Vault};
use tauri::{AppHandle, Runtime};

use crate::extensions::permitted;

/// What a package asks for before this build opens the keychain for it.
///
/// Named here rather than beside the manifest's own capabilities because there
/// is no rule about it a manifest reader could apply — the same reason
/// `work.agent` is named where it is enforced. A package that reads a secret
/// does so inside its built JavaScript, and the file a person installs says
/// nothing about the call, so the honest place to refuse it is the moment it is
/// made.
pub(crate) const VAULT_CAPABILITY: &str = "vault";

/// Every secret Sync holds, whoever it belongs to.
///
/// The keychain is the only record of what exists, so this is a search and not
/// a read of anything Sync keeps: an entry somebody deleted in Keychain Access
/// is simply not in the answer.
#[tauri::command]
pub async fn vault_entries() -> Result<Vec<Slot>, String> {
    on_the_pool(|vault| vault.list()).await
}

/// Put a secret in, or replace the one that is there.
///
/// The owner is whatever the person typed, and it is deliberately not checked
/// against what is installed: somebody may quite reasonably store a token
/// before installing the package that will read it, and a refusal at that
/// moment would be the window inventing a rule nobody asked for.
#[tauri::command]
pub async fn vault_write(owner: String, name: String, secret: String) -> Result<(), String> {
    let slot = Slot::new(&owner, &name).map_err(|error| error.to_string())?;
    on_the_pool(move |vault| vault.write(&slot, &secret)).await
}

/// Take a secret out.
#[tauri::command]
pub async fn vault_forget(owner: String, name: String) -> Result<(), String> {
    let slot = Slot::new(&owner, &name).map_err(|error| error.to_string())?;
    on_the_pool(move |vault| vault.forget(&slot)).await
}

/// How long this machine's store holds what it is given.
///
/// Asked rather than assumed, because it is not the same everywhere and the
/// window has to be able to say so: a store that loses an entry at the next
/// reboot is a store somebody should know about before they type a token into
/// it. No pool for this one — it reaches nothing.
#[tauri::command]
pub fn vault_storage() -> Result<Persistence, String> {
    Vault::system()
        .map(|vault| vault.persistence())
        .map_err(|error| error.to_string())
}

/// Read the secret a package keeps under a name of its own choosing.
///
/// The one command in this file that hands a value to the webview, and the one
/// participant it is handed to is the package the entry belongs to. What it may
/// then do with it is its own code's business — the honest paragraph about that
/// is in `docs/extensions.md` §4, because a check here could only pretend.
#[tauri::command]
pub async fn extension_secret_read<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    name: String,
) -> Result<String, String> {
    for_package(app, id, name, |vault, slot| vault.read(slot)).await
}

/// Put a secret in the package's own namespace, or replace the one there.
///
/// Reading and writing are one agreement rather than two, because the flow that
/// needs either needs both: a package that signs somebody in ends up holding a
/// token nobody could have typed, and the same package refreshes it before it
/// expires. A choice every author makes the same way is not a choice.
#[tauri::command]
pub async fn extension_secret_write<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    name: String,
    secret: String,
) -> Result<(), String> {
    for_package(app, id, name, move |vault, slot| vault.write(slot, &secret)).await
}

/// Take one of the package's own secrets out.
///
/// Here so that signing out is something a package can finish. Without it the
/// only way to be rid of a revoked token is the settings window, which is a
/// person clearing up after code that knew perfectly well it was done with it.
#[tauri::command]
pub async fn extension_secret_forget<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    name: String,
) -> Result<(), String> {
    for_package(app, id, name, |vault, slot| vault.forget(slot)).await
}

/// Read one of a package's secrets, for a caller that is already blocking.
///
/// The network door's, and the one place a value is read for something other
/// than a package asking for it: what it is for is a header the package named
/// in its manifest and never sees the value of. No capability is asked for,
/// deliberately — this is the path that exists so that an author who only has
/// to reach an API with a token does not have to hold one, and requiring
/// `vault` for it would price the safe road at the same rate as the other.
///
/// # Errors
///
/// When nothing is stored under that name, or the store refuses or is not
/// answered before the deadline.
pub(crate) fn read_for_extension(id: &str, name: &str) -> Result<String, String> {
    for_package_now(&address(id, name)?, |vault, slot| vault.read(slot))
}

/// Do one thing with one of a package's secrets, for a caller that is already
/// blocking and has already decided the package may.
///
/// **The permission is not asked for here, and that is the one thing to know
/// about this function.** Its callers ask in two different ways — the window's
/// half resolves an id against the store, a handler's host was built from an
/// artefact read when the call began, and the header-sealing path deliberately
/// asks for nothing at all — so a check inside this would be a third answer to
/// a question already answered, and the kind that gets answered *differently*.
///
/// What it does hold is the address: [`address`] joins the caller's id to the
/// name, so no route to this can name another package's namespace.
///
/// # Errors
///
/// When the name is not one, when the store cannot be opened, or when the
/// operation was refused or not answered before the deadline.
pub(crate) fn for_package_now<T>(
    slot: &Slot,
    work: impl FnOnce(&Vault, &Slot) -> Result<T, sync_vault::VaultError>,
) -> Result<T, String> {
    let vault = Vault::system().map_err(|error| error.to_string())?;
    work(&vault, slot).map_err(|error| error.to_string())
}

/// Which entry a package's call is about: its own id, and the name it chose.
///
/// **The owner is not an argument.** It is the id the store resolved, so the
/// two halves of the address come from two different places and a package
/// supplies only the one that is its own. Whatever it puts in its half stays in
/// its half — `Slot` joins them and reads the owner back from before the first
/// separator — so a name spelling a path out of the namespace is a name with
/// slashes in it and nothing more.
pub(crate) fn address(id: &str, name: &str) -> Result<Slot, String> {
    Slot::new(id, name).map_err(|error| error.to_string())
}

/// Check the permission, open the store, and do one thing for one package.
///
/// The check and the work are in one blocking task because the check reads the
/// artefact off the disk. Both refusals it can produce — nothing serves that
/// id, or that package never asked for the capability — arrive before the
/// keychain is opened at all, so a package without the agreement never causes
/// somebody's system to ask them anything.
async fn for_package<R, T, F>(
    app: AppHandle<R>,
    id: String,
    name: String,
    work: F,
) -> Result<T, String>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(&Vault, &Slot) -> Result<T, sync_vault::VaultError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        permitted(&app, &id, VAULT_CAPABILITY)?;
        for_package_now(&address(&id, &name)?, work)
    })
    .await
    .map_err(|error| format!("the keychain was not reached: {error}"))?
}

/// Open the store, do one thing with it, and answer in words.
async fn on_the_pool<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Vault) -> Result<T, sync_vault::VaultError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let vault = Vault::system().map_err(|error| error.to_string())?;
        work(&vault).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("the keychain was not reached: {error}"))?
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A package addresses its own namespace whatever it calls its secret.
    ///
    /// The owner comes from the store and the name comes from the call, and
    /// this is the join between them. A name that looks like a way out of the
    /// namespace — a path, another package's id, a leading separator — is a
    /// name, and the entry it addresses is still the caller's own.
    #[test]
    fn a_name_that_looks_like_another_package_addresses_the_caller_s_own() {
        for name in [
            "../another-package/token",
            "another-package/token",
            "/token",
            "token",
        ] {
            let slot = address("a-package", name).expect("a package addresses a secret");
            assert_eq!(
                slot.owner(),
                "a-package",
                "\"{name}\" is a name, not a way into another namespace"
            );
        }
    }

    /// And it cannot spell an owner at all, so there is no name to try.
    #[test]
    fn a_package_has_nothing_to_pass_that_would_name_another_owner() {
        let refused = address("a-package", "").expect_err("a nameless secret is refused");

        assert!(
            refused.contains("has no name"),
            "the refusal says what was missing: {refused}"
        );
    }
}
