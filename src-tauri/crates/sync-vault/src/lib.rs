//! Where a secret is kept, and the one door to it.
//!
//! A secret is not a fact about the project, so it is not in the project's
//! memory: that memory travels on a Git remote, and ciphertext that has left
//! this machine cannot be called back, while a token that has been revoked has
//! to be gone. It is not in the webview's storage either, which is a file any
//! process running as this person can read. It is in the system's own secure
//! storage, and this crate is how it gets there.
//!
//! **The name is composed here and never taken from the caller.** The service
//! is one string for the whole application; the item's name is the package the
//! secret belongs to and whatever that package calls it. So a caller supplies
//! two halves of a name and cannot supply the whole of one, which is what makes
//! a namespace a namespace rather than a convention. A package that wants a
//! different secret per project is free to put the project in its own half —
//! that is its decision to make, and nothing here makes it for it.
//!
//! **Every search states the service, and there is one search.** A search
//! without it matches every generic password the person has: on the machine
//! this was measured on, 126 of them, most of them theirs rather than Sync's.
//! That is why [`spec`] takes no arguments and why [`Vault::list`] drops
//! anything that came back under another service — a store whose matching is
//! looser than this one's would otherwise decide what Sync lists.
//!
//! **Every call has a deadline.** The system may put a dialog up and ask a
//! person for permission, and a dialog nobody is there to answer is a process
//! that waits for ever rather than a call that fails. An agent working while
//! its owner is asleep needs the second, so [`Vault`] gives it: a refusal in
//! words, after a bounded wait.
//!
//! What this crate does not do is decide *who may ask*. It is handed an owner
//! and believes it, exactly as the network door is handed a URL — the caller's
//! right to that owner is settled above, where the application knows who is
//! calling.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::Duration;

use keyring_core::api::CredentialPersistence;
use keyring_core::{CredentialStore, Error as KeyringError};
use serde::Serialize;

/// The service every entry Sync writes carries, and the only one it reads.
///
/// One string for the whole application rather than one per package: the store
/// matches a service exactly, so a service per package would mean a search per
/// package and no way to answer *what is Sync holding* at all.
pub const SERVICE: &str = "Sync";

/// What divides the owner from the name inside one item's name.
///
/// The owner may not contain it and the name may. That asymmetry is what makes
/// reading the name backwards unambiguous: the owner is everything before the
/// first one, so a package writing `staging/token` gets two names of its own
/// rather than a way into somebody else's.
const SEPARATOR: char = '/';

/// How long a call may wait on a person who may not be there.
///
/// The same twenty seconds the network door holds a column for, and for the
/// same reason: it is long enough for somebody sitting in front of the dialog
/// to type their password, and short enough that a caller with nobody in front
/// of it gets an answer rather than a hang.
const DEADLINE: Duration = Duration::from_secs(20);

/// Why a secret could not be stored, read or forgotten.
///
/// [`Waiting`](VaultError::Waiting) is the one worth reading twice. It is not
/// *the person said no* — that is [`Refused`](VaultError::Refused), which is an
/// answer. It means the system asked and the question is still on the screen.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("{0} has no name, and nothing in the keychain can be found without one")]
    Unnamed(&'static str),
    #[error(
        "\"{0}\" cannot name a package: a \"/\" in it would leave the entry belonging to two of them"
    )]
    Ambiguous(String),
    #[error("there is no secret stored under that name")]
    Missing,
    #[error("the system did not let Sync at the keychain: {0}")]
    Refused(String),
    #[error(
        "the keychain is asking somebody for permission and nobody answered within {seconds} seconds"
    )]
    Waiting { seconds: u64 },
    #[error("the keychain was asked and never answered")]
    Lost,
    #[error("this build has nowhere to keep a secret: it is macOS that Sync stores one on")]
    Nowhere,
    #[error("what is stored under that name is not text")]
    NotText,
    #[error("the keychain refused: {0}")]
    Unusable(String),
}

/// How long the store holds what it is given, as the store itself states it.
///
/// Asked rather than assumed. A store that keeps an entry until a person
/// deletes it and a store that loses it at the next reboot are both correct
/// implementations of the same trait, and the difference is the whole of what a
/// person needs to know before they type a token into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Persistence {
    /// Until somebody deletes it.
    UntilDeleted,
    /// Until this person logs out.
    UntilLogout,
    /// Until this machine reboots.
    UntilReboot,
    /// Only while something is running: nothing survives on its own.
    WhileRunning,
    /// The store did not say.
    Unknown,
}

/// One secret's address: whose it is, and what they call it.
///
/// Constructed rather than declared, because the rules that make the two halves
/// safe to join are the point of the type. There is no way to build one that
/// names a service, which is what stops a caller reaching outside Sync's own
/// entries at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    owner: String,
    name: String,
}

impl Slot {
    /// The address of a secret belonging to `owner` and called `name`.
    ///
    /// # Errors
    ///
    /// When either half is empty, or when the owner carries the separator: an
    /// entry whose owner cannot be read back is one nothing can attribute.
    pub fn new(owner: &str, name: &str) -> Result<Self, VaultError> {
        let owner = owner.trim();
        let name = name.trim();
        if owner.is_empty() {
            return Err(VaultError::Unnamed("the package a secret belongs to"));
        }
        if name.is_empty() {
            return Err(VaultError::Unnamed("the secret"));
        }
        if owner.contains(SEPARATOR) {
            return Err(VaultError::Ambiguous(owner.to_string()));
        }
        Ok(Self {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }

    /// The package this secret belongs to.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// What that package calls it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The item's name in the store: the two halves, joined here and nowhere
    /// else.
    fn account(&self) -> String {
        format!("{}{SEPARATOR}{}", self.owner, self.name)
    }

    /// The same join, read backwards, for a name that came out of the store.
    ///
    /// `None` for anything that does not split, which is how an entry under
    /// Sync's service that Sync did not write stays out of the list rather than
    /// being attributed to a package that does not exist.
    fn parse(account: &str) -> Option<Self> {
        let (owner, name) = account.split_once(SEPARATOR)?;
        Self::new(owner, name).ok()
    }
}

/// The keychain, opened once and reached through nothing else.
pub struct Vault {
    store: Arc<CredentialStore>,
}

impl Vault {
    /// Open the store this platform keeps secrets in.
    ///
    /// **The one place a store is chosen.** Adding a platform means adding a
    /// dependency and an arm here; it does not mean a second module that knows
    /// what a keychain is.
    ///
    /// # Errors
    ///
    /// When the platform has no store this build was compiled with, or when the
    /// store will not open.
    pub fn system() -> Result<Self, VaultError> {
        Ok(Self {
            store: open_store()?,
        })
    }

    /// A vault over a store somebody else built, for tests.
    #[cfg(test)]
    fn over(store: Arc<CredentialStore>) -> Self {
        Self { store }
    }

    /// Store a secret under `slot`, replacing whatever was there.
    ///
    /// # Errors
    ///
    /// When the store refuses, or when nobody answered the system's dialog
    /// before the deadline.
    pub fn write(&self, slot: &Slot, secret: &str) -> Result<(), VaultError> {
        let store = Arc::clone(&self.store);
        let account = slot.account();
        let secret = secret.to_string();
        within(DEADLINE, move || {
            entry(&store, &account)?
                .set_password(&secret)
                .map_err(translate)
        })
    }

    /// Read the secret stored under `slot`.
    ///
    /// # Errors
    ///
    /// When nothing is stored under it, when the store refuses, or when nobody
    /// answered the system's dialog before the deadline.
    pub fn read(&self, slot: &Slot) -> Result<String, VaultError> {
        let store = Arc::clone(&self.store);
        let account = slot.account();
        within(DEADLINE, move || {
            entry(&store, &account)?.get_password().map_err(translate)
        })
    }

    /// Forget the secret stored under `slot`.
    ///
    /// # Errors
    ///
    /// When nothing is stored under it, when the store refuses, or when nobody
    /// answered the system's dialog before the deadline.
    pub fn forget(&self, slot: &Slot) -> Result<(), VaultError> {
        let store = Arc::clone(&self.store);
        let account = slot.account();
        within(DEADLINE, move || {
            entry(&store, &account)?
                .delete_credential()
                .map_err(translate)
        })
    }

    /// Every secret Sync is holding, whoever it belongs to.
    ///
    /// The store is the only record of what exists. Nothing is kept beside it:
    /// a pointer file listing the names would be a second answer to the same
    /// question, going wrong the first time somebody deletes an entry in
    /// Keychain Access — which they are entitled to do, and which shows up here
    /// as the entry simply being gone.
    ///
    /// # Errors
    ///
    /// When the store refuses, or when nobody answered the system's dialog
    /// before the deadline.
    pub fn list(&self) -> Result<Vec<Slot>, VaultError> {
        let store = Arc::clone(&self.store);
        within(DEADLINE, move || {
            let found = store.search(&spec()).map_err(translate)?;
            let mut slots: Vec<Slot> = found
                .iter()
                .filter_map(keyring_core::Entry::get_specifiers)
                // The service is asked for again on the way out. It is stated
                // in the search, and a store that matched it loosely would
                // still be answering with somebody else's passwords — which is
                // too expensive a thing to hold by trusting the store alone.
                .filter(|(service, _)| service == SERVICE)
                .filter_map(|(_, account)| Slot::parse(&account))
                .collect();
            slots.sort();
            Ok(slots)
        })
    }

    /// How long this store holds what it is given.
    ///
    /// No deadline on it: it is the store describing itself and reaches nothing
    /// that could ask a person anything.
    #[must_use]
    pub fn persistence(&self) -> Persistence {
        match self.store.persistence() {
            CredentialPersistence::UntilDelete => Persistence::UntilDeleted,
            CredentialPersistence::UntilLogout => Persistence::UntilLogout,
            CredentialPersistence::UntilReboot => Persistence::UntilReboot,
            CredentialPersistence::ProcessOnly | CredentialPersistence::EntryOnly => {
                Persistence::WhileRunning
            }
            _ => Persistence::Unknown,
        }
    }
}

/// The search this crate makes, and the only place a spec is built.
///
/// It takes no arguments, and that is deliberate rather than incidental: a
/// service parameter is a service somebody can leave out, and a spec without
/// one matches the person's whole login keychain.
fn spec() -> HashMap<&'static str, &'static str> {
    HashMap::from([("service", SERVICE)])
}

/// The one place the service is handed to the store.
fn entry(store: &Arc<CredentialStore>, account: &str) -> Result<keyring_core::Entry, VaultError> {
    store.build(SERVICE, account, None).map_err(translate)
}

/// Run the work, and give up on it after `deadline`.
///
/// The thread is not joined and cannot be stopped. When the system has a dialog
/// up, the call inside it is blocked in the platform's own code until somebody
/// answers or the process ends — so what a deadline can buy is the *caller*
/// getting an answer, and that is what this buys. The thread ends when the
/// dialog does, and its result goes nowhere, which is the ordinary end of a
/// call that ran past its deadline.
fn within<T>(
    deadline: Duration,
    work: impl FnOnce() -> Result<T, VaultError> + Send + 'static,
) -> Result<T, VaultError>
where
    T: Send + 'static,
{
    let (tell, hear) = sync_channel(1);
    std::thread::spawn(move || {
        let _ = tell.send(work());
    });
    match hear.recv_timeout(deadline) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => Err(VaultError::Waiting {
            seconds: deadline.as_secs(),
        }),
        Err(RecvTimeoutError::Disconnected) => Err(VaultError::Lost),
    }
}

/// What the store said, in words that name what a person or an agent should do.
fn translate(error: KeyringError) -> VaultError {
    match error {
        KeyringError::NoEntry => VaultError::Missing,
        KeyringError::NoStorageAccess(reason) => VaultError::Refused(reason.to_string()),
        KeyringError::BadEncoding(_) => VaultError::NotText,
        other => VaultError::Unusable(other.to_string()),
    }
}

#[cfg(target_os = "macos")]
fn open_store() -> Result<Arc<CredentialStore>, VaultError> {
    let store: Arc<CredentialStore> =
        apple_native_keyring_store::keychain::Store::new().map_err(translate)?;
    Ok(store)
}

/// The phone's, and the only store it has.
///
/// Unconfigured on purpose: with no access group stated, an item goes to the
/// application's own, which is the one thing on a phone that no other
/// application can read. Naming a group here would widen that to whoever else
/// is signed into the same group, and nothing asks for it.
#[cfg(target_os = "ios")]
fn open_store() -> Result<Arc<CredentialStore>, VaultError> {
    let store: Arc<CredentialStore> =
        apple_native_keyring_store::protected::Store::new().map_err(translate)?;
    Ok(store)
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn open_store() -> Result<Arc<CredentialStore>, VaultError> {
    Err(VaultError::Nowhere)
}

#[cfg(test)]
mod tests {
    // A test that cannot set itself up has failed, and panicking is the
    // shortest true way to say so.
    #![allow(clippy::expect_used)]

    use super::*;
    use keyring_core::mock;

    fn vault() -> Vault {
        let store: Arc<CredentialStore> = mock::Store::new().expect("the mock store builds");
        Vault::over(store)
    }

    /// The invariant this whole crate is arranged around. A spec with no
    /// service matches every generic password the person has, so the check is
    /// on the one function that builds one rather than on remembering to pass
    /// it at four call sites.
    #[test]
    fn a_search_is_never_made_without_the_service() {
        let spec = spec();
        assert_eq!(
            spec.get("service"),
            Some(&SERVICE),
            "the search names the service"
        );
        assert_eq!(
            spec.len(),
            1,
            "the service is the whole spec: {spec:?} narrows it further, which would hide Sync's own entries"
        );
    }

    /// The other half of it, on the way back. A store that matched the service
    /// loosely would hand back somebody else's credentials, and the list drops
    /// them rather than believing what it was given.
    #[test]
    fn an_entry_under_another_service_is_not_listed() {
        let vault = vault();
        vault
            .write(&Slot::new("a-package", "token").expect("a slot"), "kept")
            .expect("the mock store writes");
        vault
            .store
            .build("Synchronised backups", "a-package/token", None)
            .expect("the mock store builds an entry")
            .set_password("not Sync's")
            .expect("the mock store writes");

        let listed = vault.list().expect("the list comes back");

        assert_eq!(
            listed,
            vec![Slot::new("a-package", "token").expect("a slot")],
            "only what Sync wrote is listed"
        );
    }

    /// The name is two halves joined here, so the caller cannot spell the whole
    /// of one. Whatever it puts in its own half stays in its own half.
    #[test]
    fn a_name_cannot_reach_out_of_its_owner() {
        let escaping = Slot::new("a-package", "../another-package/token").expect("a slot");

        assert_eq!(escaping.account(), "a-package/../another-package/token");
        assert_eq!(
            Slot::parse(&escaping.account()).map(|slot| slot.owner().to_string()),
            Some("a-package".to_string()),
            "the owner is still the owner when the name is read back"
        );
    }

    /// And the caller cannot spell an owner either, because an owner carrying
    /// the separator would be read back as a different one.
    #[test]
    fn an_owner_carrying_the_separator_is_refused() {
        let error = Slot::new("a-package/another-package", "token")
            .expect_err("an owner with a separator in it is refused");

        assert!(
            matches!(error, VaultError::Ambiguous(_)),
            "the refusal says the name would belong to two packages: {error}"
        );
    }

    /// A package writing under its own name cannot read one written under
    /// another, whatever it asks for.
    #[test]
    fn one_package_does_not_read_another_package_s_secret() {
        let vault = vault();
        vault
            .write(&Slot::new("a-package", "token").expect("a slot"), "kept")
            .expect("the mock store writes");

        let reaching = vault.read(&Slot::new("another-package", "token").expect("a slot"));

        assert!(
            matches!(reaching, Err(VaultError::Missing)),
            "another package's name finds nothing"
        );
    }

    /// The whole point of a deadline: a call the system is holding open ends in
    /// words rather than in waiting. The work here outlives the deadline by a
    /// long way, exactly as a dialog nobody is answering does.
    #[test]
    fn a_call_the_system_will_not_answer_ends_in_a_refusal() {
        let answer: Result<(), VaultError> = within(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(5));
            Ok(())
        });

        assert!(
            matches!(answer, Err(VaultError::Waiting { .. })),
            "a call that ran past its deadline is refused in words"
        );
    }

    /// And the deadline does not cost anything to a call that answers.
    #[test]
    fn a_call_that_answers_in_time_is_not_refused() {
        let vault = vault();
        let slot = Slot::new("a-package", "token").expect("a slot");

        vault.write(&slot, "kept").expect("the mock store writes");

        assert_eq!(vault.read(&slot).expect("it comes back"), "kept");
        vault.forget(&slot).expect("the mock store forgets");
        assert!(
            matches!(vault.read(&slot), Err(VaultError::Missing)),
            "what was forgotten is gone"
        );
    }

    /// The window is told how long an entry lasts rather than deciding. This
    /// store keeps nothing past the process, and says so.
    #[test]
    fn the_store_says_how_long_it_holds_what_it_is_given() {
        assert_eq!(vault().persistence(), Persistence::WhileRunning);
    }
}
