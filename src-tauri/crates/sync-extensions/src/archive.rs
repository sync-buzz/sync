//! Reading a `.syncext`, and deciding whether to believe it.
//!
//! Three questions, in an order that matters:
//!
//! 1. **Is it the archive it says it is?** Every file named in `META/hashes.json`
//!    is hashed and compared. This always gates: an archive whose contents do
//!    not match its own manifest of hashes is not installed, signed or not.
//! 2. **Is it from whom it says?** The signature over the canonical hashes file.
//!    Verified and reported, and in v0 it does **not** gate — the format is
//!    final so that turning the gate on later changes a policy rather than a
//!    format.
//! 3. **Does it contain what it promised?** Every path the manifest points at
//!    has to be in the archive. A missing UI bundle discovered at load time is
//!    a blank panel; discovered here it is a refusal naming the file.
//!
//! What is deliberately not here: the API range and the capabilities. Those are
//! the host's, checked against `SYNC_API_VERSION`, which is declared on the
//! surface it describes and must not have a second copy in Rust.

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::manifest::{Hashes, Manifest, ManifestError};

/// Where the manifest lives inside an archive.
pub const MANIFEST_PATH: &str = "manifest.json";
/// The sha256 of some bytes, hex — the one spelling of it in this crate.
///
/// Named rather than inlined because two different things ask it: an archive
/// hashing its own files, and a download checking that it is the file the
/// registry said it would be. Two spellings of one hash is how they come to
/// disagree about a case neither author thought of.
#[must_use]
pub fn digest_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Path → sha256, for every file the archive carries.
pub const HASHES_PATH: &str = "META/hashes.json";
/// minisign over the bytes of [`HASHES_PATH`], exactly as they are stored.
pub const SIGNATURE_PATH: &str = "META/signature";

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("the file could not be read: {0}")]
    Unreadable(#[from] std::io::Error),
    #[error("this is not a readable .syncext archive: {0}")]
    NotAnArchive(#[from] zip::result::ZipError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("the archive has no {0}")]
    Missing(String),
    #[error("{path} is not what the archive says it is")]
    Tampered { path: String },
    #[error("the archive carries {path}, which no manifest of hashes covers")]
    Uncovered { path: String },
    #[error("\"{0}\" is not a path an archive may contain")]
    Escaping(String),
    #[error("the hashes file is not readable JSON: {0}")]
    UnreadableHashes(serde_json::Error),
}

/// What is known about who produced an archive.
///
/// Three states rather than a boolean, because "no signature" and "a signature
/// that does not check out" are different facts about the same file and only
/// one of them is suspicious.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureState {
    /// Signed, and the signature is the key's.
    Valid,
    /// Signed, and it is not. Never install this without saying so loudly.
    Invalid,
    /// Not signed at all — a local build, or a package from a folder.
    Absent,
}

/// A `.syncext`, read and checked.
#[derive(Debug)]
pub struct Archive {
    manifest: Manifest,
    hashes: Hashes,
    signature: SignatureState,
    /// The sha256 of the archive file itself: what a project records as
    /// `integrity`, and what the artefact directory is named after.
    digest: String,
    path: PathBuf,
}

impl Archive {
    /// Opens an archive and answers the three questions above.
    ///
    /// `public_key` is minisign's, base64 as minisign writes it, or `None` where
    /// this build has none yet. Absent, signatures are read and reported but
    /// nothing is checked against anything.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or is not a zip, when the manifest is
    /// refused, when any file's hash does not match, when the archive carries a
    /// file no hash covers, or when something the manifest promised is missing.
    /// A bad signature is not an error here — it is a state, reported.
    pub fn open(path: &Path, public_key: Option<&str>) -> Result<Self, ArchiveError> {
        let bytes = std::fs::read(path)?;
        let digest = digest_of(&bytes);

        let cursor = std::io::Cursor::new(bytes);
        let mut zip = zip::ZipArchive::new(cursor)?;

        let manifest = Manifest::parse(&read_entry(&mut zip, MANIFEST_PATH)?)?;
        let hashes: Hashes = serde_json::from_slice(&read_entry(&mut zip, HASHES_PATH)?)
            .map_err(ArchiveError::UnreadableHashes)?;

        // Every file, against its stated hash. Done over the whole archive
        // rather than over the manifest's list: a file nobody declared is
        // exactly how something extra rides along, and it is refused below.
        let names: Vec<String> = zip.file_names().map(ToString::to_string).collect();
        for name in &names {
            if name.ends_with('/') {
                continue;
            }
            if !is_inside(name) {
                return Err(ArchiveError::Escaping(name.clone()));
            }
            if name == HASHES_PATH || name == SIGNATURE_PATH {
                continue;
            }
            let Some(expected) = hashes.get(name) else {
                return Err(ArchiveError::Uncovered { path: name.clone() });
            };
            let actual = digest_of(&read_entry(&mut zip, name)?);
            if &actual != expected {
                return Err(ArchiveError::Tampered { path: name.clone() });
            }
        }

        // And the other direction: a hash for a file that is not there means
        // the archive is missing something it accounted for.
        for path in hashes.keys() {
            if !names.contains(path) {
                return Err(ArchiveError::Missing(path.clone()));
            }
        }

        for wanted in manifest.files() {
            if !names.iter().any(|name| name == wanted) {
                return Err(ArchiveError::Missing(wanted.to_string()));
            }
        }

        let signature = verify_signature(&mut zip, public_key);

        Ok(Self {
            manifest,
            hashes,
            signature,
            digest,
            path: path.to_path_buf(),
        })
    }

    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    #[must_use]
    pub fn signature(&self) -> SignatureState {
        self.signature
    }

    /// The archive's own sha256, hex.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Writes the archive's contents into a directory.
    ///
    /// Called only after [`Archive::open`] has checked every hash, so this is
    /// extraction rather than trust: what it writes is what was verified.
    /// Directories are created as needed; nothing outside `into` is touched,
    /// because a climbing path was refused while opening.
    ///
    /// # Errors
    ///
    /// When the archive can no longer be read, or a file cannot be written.
    pub fn unpack(&self, into: &Path) -> Result<(), ArchiveError> {
        let file = std::fs::File::open(&self.path)?;
        let mut zip = zip::ZipArchive::new(file)?;

        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let name = entry.name().to_string();
            if name.ends_with('/') {
                continue;
            }
            if !is_inside(&name) {
                return Err(ArchiveError::Escaping(name));
            }
            let destination = into.join(&name);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            std::fs::write(&destination, bytes)?;
        }
        Ok(())
    }

    /// What the archive said each file should hash to.
    #[must_use]
    pub fn hashes(&self) -> &Hashes {
        &self.hashes
    }
}

/// Reads one entry whole, or says which one was missing.
fn read_entry<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    path: &str,
) -> Result<Vec<u8>, ArchiveError> {
    let mut entry = zip
        .by_name(path)
        .map_err(|_| ArchiveError::Missing(path.to_string()))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The signature over the hashes file, checked if there is a key to check it with.
///
/// The signature covers `META/hashes.json`, which covers every other file —
/// including `manifest.json`, and therefore the id and the version. Signing the
/// files alone would let a signed archive be republished under another
/// identifier, which is the attack this shape exists to prevent.
fn verify_signature<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    public_key: Option<&str>,
) -> SignatureState {
    let (Ok(signature), Ok(hashes)) = (
        read_entry(zip, SIGNATURE_PATH),
        read_entry(zip, HASHES_PATH),
    ) else {
        return SignatureState::Absent;
    };

    let Some(key) = public_key else {
        // Signed, and this build cannot say by whom. Reported as absent rather
        // than invalid: the archive is not suspect, the build is uninformed.
        return SignatureState::Absent;
    };

    let checked = minisign_verify::PublicKey::from_base64(key).and_then(|key| {
        let signature = minisign_verify::Signature::decode(&String::from_utf8_lossy(&signature))?;
        key.verify(&hashes, &signature, false)
    });

    if checked.is_ok() {
        SignatureState::Valid
    } else {
        SignatureState::Invalid
    }
}

/// Whether an entry name stays inside the directory it is unpacked into.
fn is_inside(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    // A test that cannot set itself up has failed, and panicking is the
    // shortest true way to say so.
    #![allow(clippy::expect_used)]

    use super::*;
    use std::io::Write as _;

    /// Builds an archive in memory, the way the packer will.
    fn pack(files: &[(&str, &str)], tamper: bool) -> tempfile::NamedTempFile {
        let mut hashes = Hashes::new();
        for (path, body) in files {
            hashes.insert(
                (*path).to_string(),
                hex::encode(Sha256::digest(body.as_bytes())),
            );
        }
        if tamper {
            // A hash for content the archive does not carry.
            hashes.insert(
                "ui/index.js".to_string(),
                hex::encode(Sha256::digest(b"something else")),
            );
        }

        let file = tempfile::NamedTempFile::new().expect("a temporary file");
        let mut zip = zip::ZipWriter::new(file.reopen().expect("reopen"));
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        for (path, body) in files {
            zip.start_file(*path, options).expect("entry");
            zip.write_all(body.as_bytes()).expect("write");
        }
        zip.start_file(HASHES_PATH, options).expect("entry");
        zip.write_all(&serde_json::to_vec(&hashes).expect("json"))
            .expect("write");
        zip.finish().expect("finish");
        file
    }

    const MANIFEST: &str = r#"{
      "manifestVersion": 1,
      "id": "probe",
      "version": "1.0.0",
      "name": "Probe",
      "engines": { "syncApi": "^1.0" },
      "ui": "ui/index.js"
    }"#;

    #[test]
    fn a_well_formed_archive_opens() {
        let file = pack(
            &[
                (MANIFEST_PATH, MANIFEST),
                ("ui/index.js", "export default () => ({})"),
            ],
            false,
        );
        let archive = Archive::open(file.path(), None).expect("opens");
        assert_eq!(archive.manifest().id, "probe");
        assert_eq!(archive.signature(), SignatureState::Absent);
        assert_eq!(archive.digest().len(), 64);
    }

    #[test]
    fn a_file_that_does_not_match_its_hash_is_refused() {
        let file = pack(
            &[
                (MANIFEST_PATH, MANIFEST),
                ("ui/index.js", "export default () => ({})"),
            ],
            true,
        );
        let error = Archive::open(file.path(), None).expect_err("refused");
        assert!(matches!(error, ArchiveError::Tampered { .. }), "{error}");
    }

    #[test]
    fn a_file_no_hash_covers_is_refused() {
        // The extra file is in the archive and not in `hashes`, which is how
        // something rides along beside what was signed.
        let mut hashes = Hashes::new();
        hashes.insert(
            MANIFEST_PATH.to_string(),
            hex::encode(Sha256::digest(MANIFEST.as_bytes())),
        );

        let file = tempfile::NamedTempFile::new().expect("a temporary file");
        let mut zip = zip::ZipWriter::new(file.reopen().expect("reopen"));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file(MANIFEST_PATH, options).expect("entry");
        zip.write_all(MANIFEST.as_bytes()).expect("write");
        zip.start_file("ui/index.js", options).expect("entry");
        zip.write_all(b"whatever").expect("write");
        zip.start_file(HASHES_PATH, options).expect("entry");
        zip.write_all(&serde_json::to_vec(&hashes).expect("json"))
            .expect("write");
        zip.finish().expect("finish");

        let error = Archive::open(file.path(), None).expect_err("refused");
        assert!(matches!(error, ArchiveError::Uncovered { .. }), "{error}");
    }

    #[test]
    fn an_archive_missing_the_ui_it_promised_is_refused() {
        let file = pack(&[(MANIFEST_PATH, MANIFEST)], false);
        let error = Archive::open(file.path(), None).expect_err("refused");
        assert!(
            error.to_string().contains("ui/index.js"),
            "the refusal names the file: {error}"
        );
    }

    #[test]
    fn unpacking_writes_what_was_verified() {
        let file = pack(
            &[
                (MANIFEST_PATH, MANIFEST),
                ("ui/index.js", "export default () => ({})"),
            ],
            false,
        );
        let archive = Archive::open(file.path(), None).expect("opens");
        let into = tempfile::tempdir().expect("a directory");
        archive.unpack(into.path()).expect("unpacks");

        let written = std::fs::read_to_string(into.path().join("ui/index.js")).expect("read");
        assert_eq!(written, "export default () => ({})");
    }

    #[test]
    fn a_climbing_entry_never_reaches_the_disk() {
        assert!(!is_inside("../escape.js"));
        assert!(!is_inside("/etc/passwd"));
        assert!(is_inside("ui/index.js"));
    }
}
