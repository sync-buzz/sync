//! Where installed extensions live on a machine.
//!
//! Two directories and one rule.
//!
//! ```text
//! extensions/
//!   artefacts/<sha256>/…   unpacked, immutable, shared by every project
//!   refs/<id>.json         which artefact serves that id right now
//! ```
//!
//! **An artefact is never modified in place.** Updating unpacks beside the old
//! one and moves a pointer; the previous version stays exactly as it was until
//! something deliberately collects it. That is what makes an update reversible
//! by writing four bytes rather than by downloading anything again, and it is
//! why the directory is named after the content rather than after the version:
//! two projects on one machine can want different versions of the same id, and
//! a version-named directory would make that a conflict instead of two entries.
//!
//! Artefacts are the machine's; the dependency is the repository's. Only
//! `{id, version, integrity}` is written into a project's own record, so a
//! colleague who clones it resolves the same versions on their own machine
//! rather than being handed a path from somebody else's.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError, SignatureState};
use crate::manifest::{Manifest, ManifestError};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("the pointer for \"{0}\" is not readable")]
    UnreadablePointer(String),
    #[error("nothing is installed under \"{0}\"")]
    Unknown(String),
}

/// Where a package came from, and therefore how much it is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Downloaded from the registry: the ordinary way.
    Registry,
    /// A `.syncext` somebody chose in the open panel.
    File,
    /// A folder on this machine, being written. Never signed, always marked.
    Folder,
    /// Unpacked from the archives this build ships with, on a machine that had
    /// nothing under that id yet.
    ///
    /// **Read, never written.** Builds through v0.9.0 shipped the recommended
    /// archives inside the bundle and unpacked them on a first launch; nothing
    /// does now, and a package arrives from the registry or from a folder. The
    /// word stays because pointers written by those builds are still on disk,
    /// and a variant removed from this enum is a pointer `serde` cannot read —
    /// an installed extension disappearing from the window with no account of
    /// why.
    Seeded,
}

/// The pointer file: what serves an id, and what is known about it.
///
/// Deliberately small. Everything else about an extension is in the manifest
/// inside the artefact, which is hashed — so it cannot drift from what was
/// verified, the way a copy here would.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pointer {
    pub id: String,
    pub version: String,
    /// The artefact's sha256, which is also its directory name. Absent for a
    /// folder, which has no fixed content to hash.
    #[serde(default)]
    pub integrity: Option<String>,
    pub source: Source,
    /// For a folder: where it is. Machine-local, and never written into a
    /// project — a path from somebody else's disk is worse than no answer.
    #[serde(default)]
    pub path: Option<PathBuf>,
    pub signature: SignatureState,
}

/// One extension this machine can load.
#[derive(Debug, Clone)]
pub struct Installed {
    pub manifest: Manifest,
    pub pointer: Pointer,
    /// The directory its files are under, whether artefact or working folder.
    pub root: PathBuf,
}

/// The artefact directory, and the operations over it.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Names the directory. Nothing is created until something is installed.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    fn artefacts(&self) -> PathBuf {
        self.root.join("artefacts")
    }

    fn pointers(&self) -> PathBuf {
        self.root.join("refs")
    }

    fn pointer_path(&self, id: &str) -> PathBuf {
        self.pointers().join(format!("{id}.json"))
    }

    /// Unpacks an archive and points its id at it.
    ///
    /// Two phases, and the order is the whole safety of it: the artefact is
    /// complete on disk before any pointer names it, so a failure halfway
    /// leaves an unreferenced directory — collectable, harmless — rather than a
    /// pointer to half an extension.
    ///
    /// Installing the same archive twice costs one hash and no writes: the
    /// directory is named after the content, so it is already there.
    ///
    /// # Errors
    ///
    /// When the artefact cannot be unpacked or the pointer cannot be written.
    pub fn install(&self, archive: &Archive, source: Source) -> Result<Installed, StoreError> {
        let digest = archive.digest().to_string();
        let destination = self.artefacts().join(&digest);

        if !destination.join(crate::archive::MANIFEST_PATH).exists() {
            // Unpack beside the final name and move it into place. A directory
            // that appears fully formed cannot be read half-written by a window
            // opening at the wrong moment.
            let staging = self.artefacts().join(format!(".{digest}.partial"));
            if staging.exists() {
                std::fs::remove_dir_all(&staging)?;
            }
            std::fs::create_dir_all(&staging)?;
            archive.unpack(&staging)?;
            std::fs::create_dir_all(self.artefacts())?;
            // Another window may have won the race and put the same content
            // there. Identical content, so the loser simply drops its copy.
            if destination.exists() {
                std::fs::remove_dir_all(&staging)?;
            } else {
                std::fs::rename(&staging, &destination)?;
            }
        }

        let pointer = Pointer {
            id: archive.manifest().id.clone(),
            version: archive.manifest().version.clone(),
            integrity: Some(digest),
            source,
            path: None,
            signature: archive.signature(),
        };
        self.write_pointer(&pointer)?;

        Ok(Installed {
            manifest: archive.manifest().clone(),
            pointer,
            root: destination,
        })
    }

    /// Points an id at a folder somebody is writing in.
    ///
    /// Nothing is copied and nothing is hashed: the files are read where they
    /// lie, so an author's next build is what the window loads. That is the
    /// whole feature, and it is also why this source is marked everywhere it
    /// appears.
    ///
    /// # Errors
    ///
    /// When the folder holds no readable manifest, or the pointer cannot be
    /// written.
    pub fn install_folder(&self, folder: &Path) -> Result<Installed, StoreError> {
        let manifest =
            Manifest::parse(&std::fs::read(folder.join(crate::archive::MANIFEST_PATH))?)?;

        let pointer = Pointer {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            integrity: None,
            source: Source::Folder,
            path: Some(folder.to_path_buf()),
            signature: SignatureState::Absent,
        };
        self.write_pointer(&pointer)?;

        Ok(Installed {
            manifest,
            pointer,
            root: folder.to_path_buf(),
        })
    }

    /// The pointer is written whole and moved into place, never edited.
    ///
    /// An update is exactly this: a new pointer over an old one, atomically. A
    /// half-written pointer would be an id that resolves to nothing on the next
    /// launch, which is an extension that vanished for no reason a person could
    /// see.
    fn write_pointer(&self, pointer: &Pointer) -> Result<(), StoreError> {
        std::fs::create_dir_all(self.pointers())?;
        let final_path = self.pointer_path(&pointer.id);
        let staging = final_path.with_extension("json.partial");
        std::fs::write(
            &staging,
            serde_json::to_vec_pretty(pointer).unwrap_or_default(),
        )?;
        std::fs::rename(&staging, &final_path)?;
        Ok(())
    }

    /// What is installed under one id, or `None`.
    ///
    /// # Errors
    ///
    /// When the pointer exists and cannot be read, or names neither an artefact
    /// nor a folder. A pointer whose artefact is gone reads as `None` rather
    /// than as an error: the catalogue's job is to offer to install it again.
    pub fn resolve(&self, id: &str) -> Result<Option<Installed>, StoreError> {
        let path = self.pointer_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let pointer: Pointer = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|_| StoreError::UnreadablePointer(id.to_string()))?;

        let root = match (&pointer.integrity, &pointer.path) {
            (Some(digest), _) => self.artefacts().join(digest),
            (None, Some(folder)) => folder.clone(),
            (None, None) => return Err(StoreError::UnreadablePointer(id.to_string())),
        };

        let manifest_path = root.join(crate::archive::MANIFEST_PATH);
        if !manifest_path.exists() {
            // The pointer outlived what it pointed at: a folder that moved, or
            // an artefact somebody deleted. Reported as absent rather than as an
            // error, because the catalogue's job is to say so and offer to
            // install it again.
            return Ok(None);
        }

        Ok(Some(Installed {
            manifest: Manifest::parse(&std::fs::read(manifest_path)?)?,
            pointer,
            root,
        }))
    }

    /// Everything this machine can load, in no particular order.
    ///
    /// A pointer that no longer resolves is skipped rather than raised: one
    /// missing artefact must not make the whole catalogue unreadable.
    ///
    /// # Errors
    ///
    /// When the pointer directory itself cannot be read.
    pub fn list(&self) -> Result<Vec<Installed>, StoreError> {
        let directory = self.pointers();
        if !directory.exists() {
            return Ok(Vec::new());
        }

        let mut installed = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().and_then(|it| it.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|it| it.to_str()) else {
                continue;
            };
            if let Ok(Some(one)) = self.resolve(id) {
                installed.push(one);
            }
        }
        Ok(installed)
    }

    /// Points an id back at an artefact that is already unpacked.
    ///
    /// What an update's last step undoes. Applying one is more than moving this
    /// pointer — the type definitions are published into the project's memory
    /// and the version is written into its record, and both of those happen in
    /// the window, after the pointer has already moved. A failure there leaves
    /// a project declaring one version and a machine serving another, so the
    /// pointer is put back exactly as it was.
    ///
    /// Exactly as it was is why this takes a whole [`Pointer`] rather than a
    /// digest: the source and the signature state are what the archive said
    /// when it was verified, and the archive is deleted once it is unpacked.
    /// Rebuilding either from what is on the disk would be this build inventing
    /// the provenance of somebody else's package.
    ///
    /// # Errors
    ///
    /// When the pointer names nothing this machine holds — the artefact was
    /// collected, or the folder moved — or when it cannot be written.
    pub fn repoint(&self, pointer: &Pointer) -> Result<Installed, StoreError> {
        let root = match (&pointer.integrity, &pointer.path) {
            (Some(digest), _) => self.artefacts().join(digest),
            (None, Some(folder)) => folder.clone(),
            (None, None) => return Err(StoreError::UnreadablePointer(pointer.id.clone())),
        };

        // Read before it is written, so an artefact that is no longer there is
        // refused rather than pointed at: a pointer to nothing resolves as
        // absent, which is an extension that vanished for no reason a person
        // could see — and it would be this rollback that did it.
        let manifest = Manifest::parse(&std::fs::read(root.join(crate::archive::MANIFEST_PATH))?)?;
        self.write_pointer(pointer)?;

        Ok(Installed {
            manifest,
            pointer: pointer.clone(),
            root,
        })
    }

    /// Stops serving an id. The artefact stays.
    ///
    /// Deleting the files as well would be wrong twice over: another project on
    /// this machine may declare the same version, and re-installing what is
    /// already unpacked should cost nothing. Collecting unreferenced artefacts
    /// is a separate decision, made when there is a reason to make it.
    ///
    /// # Errors
    ///
    /// When nothing is installed under that id, or the pointer cannot be
    /// removed.
    pub fn forget(&self, id: &str) -> Result<(), StoreError> {
        let path = self.pointer_path(id);
        if path.exists() {
            std::fs::remove_file(path)?;
            Ok(())
        } else {
            Err(StoreError::Unknown(id.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot set itself up has failed, and panicking is the
    // shortest true way to say so.
    #![allow(clippy::expect_used)]

    use super::*;
    use sha2::{Digest as _, Sha256};
    use std::io::Write as _;

    const MANIFEST: &str = r#"{
      "manifestVersion": 1,
      "id": "probe",
      "version": "1.0.0",
      "name": "Probe",
      "engines": { "syncApi": "^1.0" }
    }"#;

    fn pack(body: &str) -> tempfile::NamedTempFile {
        let files = [
            (crate::archive::MANIFEST_PATH, MANIFEST),
            ("ui/index.js", body),
        ];
        let mut hashes = crate::manifest::Hashes::new();
        for (path, content) in files {
            hashes.insert(
                path.to_string(),
                hex::encode(Sha256::digest(content.as_bytes())),
            );
        }

        let file = tempfile::NamedTempFile::new().expect("a temporary file");
        let mut zip = zip::ZipWriter::new(file.reopen().expect("reopen"));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (path, content) in files {
            zip.start_file(path, options).expect("entry");
            zip.write_all(content.as_bytes()).expect("write");
        }
        zip.start_file(crate::archive::HASHES_PATH, options)
            .expect("entry");
        zip.write_all(&serde_json::to_vec(&hashes).expect("json"))
            .expect("write");
        zip.finish().expect("finish");
        file
    }

    #[test]
    fn installing_unpacks_and_points_at_it() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());
        let file = pack("export default () => ({})");
        let archive = Archive::open(file.path(), None).expect("opens");

        let installed = store.install(&archive, Source::File).expect("installs");
        assert_eq!(installed.manifest.id, "probe");
        assert!(installed.root.join("ui/index.js").exists());

        let resolved = store.resolve("probe").expect("reads").expect("is there");
        assert_eq!(resolved.pointer.version, "1.0.0");
        assert_eq!(resolved.pointer.source, Source::File);
    }

    #[test]
    fn a_second_version_moves_the_pointer_and_leaves_the_first() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());

        let first = pack("export default () => ({ first: true })");
        let first = Archive::open(first.path(), None).expect("opens");
        let one = store.install(&first, Source::File).expect("installs");

        let second = pack("export default () => ({ second: true })");
        let second = Archive::open(second.path(), None).expect("opens");
        let two = store.install(&second, Source::File).expect("installs");

        assert_ne!(one.root, two.root, "different content, different artefact");
        assert!(one.root.exists(), "the previous artefact is left alone");
        assert_eq!(
            store
                .resolve("probe")
                .expect("reads")
                .expect("is there")
                .root,
            two.root,
            "the pointer names the new one"
        );
    }

    /// The rollback an update's last step needs.
    ///
    /// The two artefacts are both on the disk — installing never touched the
    /// first — so putting the pointer back is a write of a few bytes and the
    /// version that was serving is serving again.
    #[test]
    fn a_pointer_goes_back_to_the_artefact_it_left() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());

        let first = pack("export default () => ({ first: true })");
        let first = Archive::open(first.path(), None).expect("opens");
        let before = store.install(&first, Source::Registry).expect("installs");

        let second = pack("export default () => ({ second: true })");
        let second = Archive::open(second.path(), None).expect("opens");
        store.install(&second, Source::Registry).expect("installs");

        let back = store.repoint(&before.pointer).expect("points back");
        assert_eq!(back.root, before.root);
        assert_eq!(
            store
                .resolve("probe")
                .expect("reads")
                .expect("is there")
                .pointer
                .integrity,
            before.pointer.integrity,
        );
    }

    /// And it refuses rather than pointing an id at something that is gone.
    ///
    /// A pointer to nothing reads as absent, which is an extension that
    /// vanished — the one outcome a rollback must not produce.
    #[test]
    fn a_rollback_onto_a_collected_artefact_is_refused() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());
        let file = pack("export default () => ({})");
        let archive = Archive::open(file.path(), None).expect("opens");
        let installed = store.install(&archive, Source::Registry).expect("installs");

        std::fs::remove_dir_all(&installed.root).expect("removes");
        assert!(store.repoint(&installed.pointer).is_err());
    }

    #[test]
    fn forgetting_leaves_the_files_where_they_are() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());
        let file = pack("export default () => ({})");
        let archive = Archive::open(file.path(), None).expect("opens");
        let installed = store.install(&archive, Source::File).expect("installs");

        store.forget("probe").expect("forgets");
        assert!(store.resolve("probe").expect("reads").is_none());
        assert!(installed.root.exists(), "the artefact is not deleted");
    }

    #[test]
    fn a_pointer_to_something_that_is_gone_reads_as_absent() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());
        let file = pack("export default () => ({})");
        let archive = Archive::open(file.path(), None).expect("opens");
        let installed = store.install(&archive, Source::File).expect("installs");

        std::fs::remove_dir_all(&installed.root).expect("removes");
        assert!(store.resolve("probe").expect("reads").is_none());
        assert!(store.list().expect("lists").is_empty());
    }

    #[test]
    fn a_folder_is_read_where_it_lies() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());
        let folder = tempfile::tempdir().expect("a directory");
        std::fs::write(folder.path().join("manifest.json"), MANIFEST).expect("writes");

        let installed = store.install_folder(folder.path()).expect("installs");
        assert_eq!(installed.pointer.source, Source::Folder);
        assert_eq!(installed.root, folder.path());
        assert!(installed.pointer.integrity.is_none());
    }

    #[test]
    fn listing_skips_what_no_longer_resolves() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::at(home.path().to_path_buf());
        let file = pack("export default () => ({})");
        let archive = Archive::open(file.path(), None).expect("opens");
        store.install(&archive, Source::File).expect("installs");

        std::fs::write(home.path().join("refs/ghost.json"), "{ not json").expect("writes");
        assert_eq!(store.list().expect("lists").len(), 1);
    }
}
