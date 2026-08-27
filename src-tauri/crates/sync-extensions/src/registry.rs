//! What exists anywhere, and the only part of this application that dials out.
//!
//! **The network lives here rather than in the webview, and that is the whole
//! design.** The window's `connect-src` names `'self'` and the IPC endpoint and
//! nothing else, so a record's body cannot ask this application to fetch
//! anything. What can be reached is a property of the build — the hosts are in
//! [`ALLOWED_HOSTS`], compiled in — rather than a property of a page. That is
//! the one place the "no network in v1" decision is reversed, and keeping the
//! reversal in Rust is what keeps it small.
//!
//! **Every hop is checked, not only the first.** A release download redirects,
//! so a policy that admitted the starting host and then followed wherever it
//! was sent would be an allow-list with a door in it. The redirect policy
//! refuses a hop to a host that is not on the list, which is why it is written
//! out rather than left at the default.
//!
//! **The bytes come from one organisation's repositories.** A host is not an
//! author — `github.com` is every repository anybody has — so an artefact's URL
//! is pinned to a release of an [`ORGANISATION`] repository before the first
//! request. Without it a tampered index could name any repository on GitHub and
//! be believed, because the digest beside the URL would agree: the URL and the
//! digest are the same file's word.
//!
//! **The index is one file.** Fetched with its `ETag` and cached beside the
//! artefacts, so opening the catalogue usually costs a 304 and a read from
//! disk: no GitHub API, therefore no rate limit, no token, and nothing to
//! authenticate. A fetch that fails leaves whatever was cached — a marketplace
//! is worth showing from yesterday's index, and a person with no network is
//! better served by a list that is a day old than by a column that says the
//! network is down.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

/// The index format this build reads.
///
/// Read before anything else in the file, so an index from a newer registry is
/// answered with "this Sync is too old" rather than with a field name nobody
/// asked about — the same order [`crate::manifest`] reads a manifest in.
pub const SUPPORTED_REGISTRY_FORMAT: u32 = 1;

/// Where the index is, and it is not configurable.
///
/// A settings field pointing this somewhere else would make "which extensions
/// exist" a per-machine answer, and the first thing anybody would do with it is
/// point a colleague's Sync at something. Sideloading already has a door — a
/// `.syncext` file, or a folder — and it is a door that says on the card what
/// came through it.
pub const INDEX_URL: &str =
    "https://raw.githubusercontent.com/sync-buzz/sync-extensions/main/registry.json";

/// Everything this build may talk to, on any hop of any request.
///
/// Each is one leg of fetching from GitHub: the index is a raw file, a release
/// asset is a `github.com` URL, and that URL redirects to storage. Storage is
/// two names because GitHub moved release assets to `release-assets` and kept
/// the older name for other content. A build that knows only one of them stops
/// its own download halfway and blames the server's `302` for it.
const ALLOWED_HOSTS: &[&str] = &[
    "raw.githubusercontent.com",
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

/// The organisation whose repositories an artefact may come from.
///
/// [`ALLOWED_HOSTS`] says the bytes come from GitHub; this says which GitHub,
/// and the two together are the whole door. Alone, the host list would admit
/// `github.com/anybody/anything` — and the digest beside such a URL would match
/// it, since both come from the same index. An index that has been tampered
/// with agrees with itself; what it cannot do is name somebody else's release.
const ORGANISATION: &str = "sync-buzz";

/// Nothing here is worth waiting on for long: the catalogue has a cached answer.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The largest artefact this will accept.
///
/// A ceiling rather than trust in `Content-Length`, because the header is the
/// server's claim and the disk is ours. An extension is a few tens of kilobytes;
/// eight megabytes is far above anything plausible and far below anything that
/// fills a disk while somebody watches a spinner.
const LARGEST_ARTEFACT: u64 = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the registry could not be reached: {0}")]
    Unreachable(String),
    #[error("the registry answered {0}")]
    Refused(u16),
    #[error("the registry's index is not readable: {0}")]
    Unreadable(#[from] serde_json::Error),
    #[error(
        "this extension registry needs a newer Sync: its index is written in format {found}, and this build reads {SUPPORTED_REGISTRY_FORMAT}"
    )]
    Newer { found: u32 },
    #[error("\"{0}\" is not a host this build downloads from")]
    Elsewhere(String),
    #[error("\"{0}\" is not a release of a {ORGANISATION} repository")]
    NotOurs(String),
    #[error("\"{0}\" is not an extension identifier")]
    Nameless(String),
    #[error("the download is larger than this build accepts")]
    TooLarge,
    #[error("the download is not the file the registry named: expected {expected}, got {found}")]
    Mismatched { expected: String, found: String },
}

/// Where an artefact can be fetched from, and what it must hash to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artefact {
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

/// One extension, at the length a card needs and a search matches on.
///
/// Deliberately not the manifest. What a card says is a subset of what a
/// package says, and carrying the whole manifest here would make the file every
/// window fetches grow with everything anybody had ever published. The
/// description, the changelog and the older versions are the extension's own
/// ledger, fetched when a page is opened.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub version: String,
    /// The range of Sync's extension API it was written for.
    pub sync_api: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    /// The kinds it publishes, which is also what somebody searching for a word
    /// like "decision" is looking for.
    #[serde(default)]
    pub publishes: Vec<String>,
    #[serde(default)]
    pub areas: Vec<ListedArea>,
    /// Whether it tells an agent anything at all.
    #[serde(default)]
    pub prompt: bool,
    #[serde(default)]
    pub npm: Vec<String>,
    #[serde(default)]
    pub author: Option<serde_json::Value>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    pub artefact: Artefact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListedArea {
    pub id: String,
    pub label: String,
}

/// The index, as the registry publishes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub format_version: u32,
    pub extensions: Vec<Listed>,
}

/// Every version one extension has published, newest first.
///
/// A file of its own rather than part of the index, and that division is what
/// keeps the index small: the changelog of every version of everything anybody
/// ever published would be fetched by every window that opened a marketplace.
/// This is fetched when a page is opened, about the one extension it is about.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    pub format_version: u32,
    pub id: String,
    pub versions: Vec<Release>,
}

/// One published version, at the length a page needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    /// The range of Sync's extension API this version was written for.
    ///
    /// Per version rather than per extension, because it is what decides
    /// whether *this* version may be offered: an extension whose newest release
    /// needs a Sync this build is below is one whose older release may still be
    /// perfectly installable.
    pub sync_api: String,
    #[serde(default)]
    pub description: String,
    /// What changed, in the author's words, and empty for a first release.
    #[serde(default)]
    pub changelog: String,
    pub artefact: Artefact,
}

/// Something read from the registry, and where it came from.
///
/// One type over both files because the answer to "did the network say this, or
/// is this what the disk remembered" is the same answer for an index and for a
/// ledger, and two spellings of it is how one of them comes to be forgotten at
/// a call site.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fetched<T> {
    pub answer: T,
    /// True when this is what was on disk rather than what the network answered.
    ///
    /// Said out loud because it is the difference between "these are the
    /// extensions there are" and "these are the extensions there were when this
    /// machine last had a network". A catalogue that could not tell them apart
    /// would present the second as the first.
    pub cached: bool,
}

/// The registry, and the directory it keeps its one cached file in.
pub struct Registry {
    root: PathBuf,
    index_url: String,
}

impl Registry {
    /// Names the directory. Nothing is created until something is fetched.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self {
            root,
            index_url: INDEX_URL.to_owned(),
        }
    }

    /// Points this at another index. For tests, and reachable from nowhere else.
    #[must_use]
    #[doc(hidden)]
    pub fn from(mut self, index_url: String) -> Self {
        self.index_url = index_url;
        self
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    fn etag_path(&self) -> PathBuf {
        self.root.join("index.etag")
    }

    /// Where one extension's ledger is fetched from, and cached.
    ///
    /// Derived from the index's own URL rather than written out a second time,
    /// so an index pointed somewhere else — which is what a test does — takes
    /// its ledgers with it. An index and the ledgers beside it are one
    /// publication, and two constants would be two ways to point at half of it.
    fn ledger_url(&self, id: &str) -> String {
        let directory = self
            .index_url
            .rsplit_once('/')
            .map_or("", |(before, _)| before);
        format!("{directory}/registry/{id}.json")
    }

    fn ledger_path(&self, id: &str) -> PathBuf {
        self.root.join("ledgers").join(format!("{id}.json"))
    }

    fn ledger_etag_path(&self, id: &str) -> PathBuf {
        self.root.join("ledgers").join(format!("{id}.etag"))
    }

    /// The index, from the network when it has moved and from disk when it has
    /// not.
    ///
    /// # Errors
    ///
    /// When there is no network *and* nothing cached, when the answer is not an
    /// index this build reads, or when the registry refuses.
    pub fn index(&self) -> Result<Fetched<Index>, RegistryError> {
        read(&self.index_url, &self.index_path(), &self.etag_path())
    }

    /// What the last fetch left on the disk, without asking anybody anything.
    ///
    /// The window reads this when a project opens, and it is deliberately not
    /// [`Registry::index`]: a person who has never opened the catalogue has
    /// never asked this application to dial out, and every launch turning into
    /// a request would make that promise false for the sake of a mark on one
    /// row. What is on the disk was fetched because somebody did ask, so
    /// reading it costs nothing and claims nothing new.
    ///
    /// `None` is the machine that has never fetched one — the first launch, and
    /// not an error: there is nothing to say about updates yet, and saying
    /// nothing is the honest form of that.
    ///
    /// # Errors
    ///
    /// When a cached index exists and cannot be read as one.
    pub fn cached_index(&self) -> Result<Option<Index>, RegistryError> {
        match std::fs::read(self.index_path()) {
            Ok(body) => parse(&body).map(Some),
            Err(absent) if absent.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(unreadable) => Err(unreadable.into()),
        }
    }

    /// Every version one extension has published.
    ///
    /// Fetched when a page is opened rather than with the index, because it is
    /// about one extension and the index is about all of them. Cached the same
    /// way, so a page opened twice costs a 304.
    ///
    /// # Errors
    ///
    /// When `id` is not an extension identifier, when there is no network *and*
    /// nothing cached, or when the answer is not a ledger this build reads.
    pub fn ledger(&self, id: &str) -> Result<Fetched<Ledger>, RegistryError> {
        // The id came out of the index, which came off the network, and it is
        // about to become a path segment in a URL and a file name on the disk.
        // Held to the manifest's own rule rather than trusted for either.
        if !crate::manifest::is_identifier(id) {
            return Err(RegistryError::Nameless(id.to_owned()));
        }
        read(
            &self.ledger_url(id),
            &self.ledger_path(id),
            &self.ledger_etag_path(id),
        )
    }

    /// Downloads one artefact and answers with the file it wrote.
    ///
    /// The digest is checked here rather than left to the archive reader,
    /// because these are two different questions: the archive asks whether its
    /// own hashes cover its own files, and this asks whether the bytes are the
    /// ones the registry named. A package rebuilt and re-tagged under a version
    /// somebody already has passes the first and fails this one.
    ///
    /// # Errors
    ///
    /// When the URL is not a release of an [`ORGANISATION`] repository, when a
    /// redirect leaves [`ALLOWED_HOSTS`], when the download fails or is larger
    /// than [`LARGEST_ARTEFACT`], or when the bytes are not what was named.
    pub fn download(&self, artefact: &Artefact, into: &Path) -> Result<PathBuf, RegistryError> {
        refuse_another_repository(&artefact.url)?;

        let response = client()?
            .get(&artefact.url)
            .send()
            .map_err(|error| RegistryError::Unreachable(error.to_string()))?;
        if let Some(refusal) = refused_hop(&response) {
            return Err(refusal);
        }
        if !response.status().is_success() {
            return Err(RegistryError::Refused(response.status().as_u16()));
        }

        // Read under a ceiling rather than trusted to `Content-Length`: the
        // header is what the server says and the disk is ours.
        let mut body = Vec::new();
        response
            .take(LARGEST_ARTEFACT + 1)
            .read_to_end(&mut body)
            .map_err(|error| RegistryError::Unreachable(error.to_string()))?;
        if body.len() as u64 > LARGEST_ARTEFACT {
            return Err(RegistryError::TooLarge);
        }

        let found = crate::archive::digest_of(&body);
        if found != artefact.sha256 {
            return Err(RegistryError::Mismatched {
                expected: artefact.sha256.clone(),
                found,
            });
        }

        // Named after the digest, so two downloads of the same artefact are one
        // file and a half-written one is never mistaken for the whole.
        std::fs::create_dir_all(into)?;
        let at = into.join(format!("{found}.syncext"));
        std::fs::write(&at, &body)?;
        Ok(at)
    }
}

enum Answer {
    Unchanged,
    Fresh { body: Vec<u8>, etag: Option<String> },
}

/// One file, fetched with what is held of it and cached by what came back.
///
/// The index and a ledger differ in what they say and in nothing about how they
/// are read, so this is written once: ask with the held `ETag`, take a 304 as
/// "what is on the disk is current", and fall back to the disk when there is no
/// network at all.
fn read<T: serde::de::DeserializeOwned>(
    url: &str,
    cached_at: &Path,
    etag_at: &Path,
) -> Result<Fetched<T>, RegistryError> {
    let held = std::fs::read_to_string(etag_at).ok();
    let from_disk = || -> Result<T, RegistryError> { parse(&std::fs::read(cached_at)?) };

    match ask(url, held.as_deref()) {
        Ok(Answer::Unchanged) => Ok(Fetched {
            answer: from_disk()?,
            cached: true,
        }),
        Ok(Answer::Fresh { body, etag }) => {
            let answer = parse(&body)?;
            // Written only once the body has parsed as something this build
            // reads. Caching something unreadable would replace a working
            // answer with one that fails identically on every launch.
            if let Some(directory) = cached_at.parent() {
                std::fs::create_dir_all(directory)?;
            }
            std::fs::write(cached_at, &body)?;
            match etag {
                Some(tag) => std::fs::write(etag_at, tag)?,
                // No ETag is not a failure; it is a server that will be
                // asked in full next time. The stale tag has to go, or the
                // next request would claim to hold something it does not.
                None => drop(std::fs::remove_file(etag_at)),
            }
            Ok(Fetched {
                answer,
                cached: false,
            })
        }
        Err(unreachable) => {
            // A marketplace is worth showing from yesterday's index. Only
            // when there is nothing at all does the failure reach anybody,
            // and then it is the network's own words.
            from_disk().map_or(Err(unreachable), |answer| {
                Ok(Fetched {
                    answer,
                    cached: true,
                })
            })
        }
    }
}

/// Asks for one file, with the `ETag` held of it if there is one.
///
/// Free of the registry it is asked through, because it needs nothing from it:
/// which file is a URL, and what is held of that file is a string. Both are the
/// caller's, and the two files this reads are told apart nowhere else.
fn ask(url: &str, etag: Option<&str>) -> Result<Answer, RegistryError> {
    let mut request = client()?.get(url);
    if let Some(tag) = etag {
        request = request.header("If-None-Match", tag);
    }

    let response = request
        .send()
        .map_err(|error| RegistryError::Unreachable(error.to_string()))?;

    if response.status() == 304 {
        return Ok(Answer::Unchanged);
    }
    if let Some(refusal) = refused_hop(&response) {
        return Err(refusal);
    }
    if !response.status().is_success() {
        return Err(RegistryError::Refused(response.status().as_u16()));
    }

    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response
        .bytes()
        .map_err(|error| RegistryError::Unreachable(error.to_string()))?
        .to_vec();

    Ok(Answer::Fresh { body, etag })
}

/// The format is read before the shape, whichever file this is.
///
/// Both files the registry publishes carry `formatVersion`, and both are
/// answered the same way: a number this build is below is "this Sync is too
/// old", said before anything else, rather than a complaint about a field
/// nobody asked about. The same order [`crate::manifest`] reads a manifest in.
fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, RegistryError> {
    #[derive(Deserialize)]
    struct JustTheVersion {
        #[serde(rename = "formatVersion")]
        format_version: u32,
    }

    let probe: JustTheVersion = serde_json::from_slice(body)?;
    if probe.format_version > SUPPORTED_REGISTRY_FORMAT {
        return Err(RegistryError::Newer {
            found: probe.format_version,
        });
    }
    Ok(serde_json::from_slice(body)?)
}

/// Whether a URL names a host this build talks to.
fn refuse_elsewhere(url: &str) -> Result<(), RegistryError> {
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        // A URL may carry credentials and a port, and neither is the host.
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if ALLOWED_HOSTS.contains(&host.as_str()) {
        Ok(())
    } else {
        Err(RegistryError::Elsewhere(host))
    }
}

/// Whether a URL is a release of a repository this build installs from.
///
/// Parsed rather than split, because every shape that makes a URL read as one
/// place and resolve to another is the parser's business: a `..` segment is
/// resolved before the path is looked at, credentials are not the host, and a
/// default port is not part of a name. Fails closed — anything that does not
/// read as `https://github.com/<organisation>/<repository>/releases/download/…`
/// is refused, a URL that does not parse at all included.
fn refuse_another_repository(url: &str) -> Result<(), RegistryError> {
    let refuse = || RegistryError::NotOurs(url.to_owned());

    let parsed = Url::parse(url).map_err(|_| refuse())?;
    if parsed.scheme() != "https"
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || !parsed
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
    {
        return Err(refuse());
    }

    // `path` is normalised and always begins with the separator, so the first
    // piece a split answers with is empty and the organisation is the second.
    let mut path = parsed.path().split('/').skip(1);
    let ours = path.next() == Some(ORGANISATION)
        && path.next().is_some_and(|repository| !repository.is_empty())
        && path.next() == Some("releases")
        && path.next() == Some("download")
        && path.next().is_some_and(|tag| !tag.is_empty())
        && path.next().is_some_and(|file| !file.is_empty());

    if ours { Ok(()) } else { Err(refuse()) }
}

/// The refusal behind a redirect that reached the caller.
///
/// A 3xx arriving here is [`client`]'s policy having stopped, not a server
/// having answered: every hop that is allowed is followed, so the only redirect
/// left standing is one this build declined to take. Reporting the status
/// number instead would name the wrong culprit — GitHub redirecting a download
/// is ordinary, and the refusal is ours.
fn refused_hop(response: &reqwest::blocking::Response) -> Option<RegistryError> {
    let next = response.headers().get("location")?.to_str().ok()?;
    refuse_elsewhere(next).err()
}

/// One client, refusing a redirect off the list as firmly as a first request.
fn client() -> Result<reqwest::blocking::Client, RegistryError> {
    reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("Sync/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 {
                attempt.error("too many redirects")
            } else if refuse_elsewhere(attempt.url().as_str()).is_ok() {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| RegistryError::Unreachable(error.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn only_the_hosts_this_build_downloads_from_are_reachable() {
        for allowed in ALLOWED_HOSTS {
            assert!(refuse_elsewhere(&format!("https://{allowed}/a/b")).is_ok());
        }
        // The shapes a URL uses to look like one host and be another.
        for elsewhere in [
            "https://evil.example/a",
            "https://github.com.evil.example/a",
            "https://raw.githubusercontent.com.evil.example/a",
            "https://evil.example/github.com/a",
            "https://github.com@evil.example/a",
            "https://evil.example:443/a",
        ] {
            assert!(
                refuse_elsewhere(elsewhere).is_err(),
                "{elsewhere} was admitted"
            );
        }
    }

    #[test]
    fn a_port_and_a_case_do_not_make_another_host() {
        assert!(refuse_elsewhere("https://GitHub.com/a").is_ok());
        assert!(refuse_elsewhere("https://github.com:443/a").is_ok());
    }

    /// The regression that answered every install with `302`: GitHub moved
    /// release assets to another storage name, and a hop off the list is
    /// stopped rather than followed.
    #[test]
    fn githubs_release_storage_is_a_hop_this_build_takes() {
        assert!(
            refuse_elsewhere(
                "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=x"
            )
            .is_ok()
        );
    }

    #[test]
    fn only_a_release_of_this_organisation_is_downloaded() {
        assert!(
            refuse_another_repository(
                "https://github.com/sync-buzz/sync-extensions/releases/download/v1.0.0/chat-1.0.0.syncext"
            )
            .is_ok()
        );
        for elsewhere in [
            // Somebody else's release, and the shapes used to read as ours.
            "https://github.com/somebody/theirs/releases/download/v1/x.syncext",
            "https://github.com/sync-buzz-evil/theirs/releases/download/v1/x.syncext",
            "https://github.com/sync-buzz/x/../../somebody/theirs/releases/download/v1/x.syncext",
            "https://github.com.evil.example/sync-buzz/x/releases/download/v1/x.syncext",
            "https://sync-buzz@evil.example/sync-buzz/x/releases/download/v1/x.syncext",
            "https://raw.githubusercontent.com/sync-buzz/x/releases/download/v1/x.syncext",
            "http://github.com/sync-buzz/x/releases/download/v1/x.syncext",
            // Ours, but not a release: the door is one shape wide.
            "https://github.com/sync-buzz/x/archive/refs/heads/main.zip",
            "https://github.com/sync-buzz/x/releases/download/v1/",
            "not a url at all",
        ] {
            assert!(
                refuse_another_repository(elsewhere).is_err(),
                "{elsewhere} was admitted"
            );
        }
    }

    /// A `..` that resolves back inside the organisation is ours, and saying so
    /// is the point of parsing: the answer is where the URL goes, not how it
    /// reads.
    #[test]
    fn a_path_that_climbs_back_into_the_organisation_is_ours() {
        assert!(
            refuse_another_repository(
                "https://github.com/somebody/theirs/../../sync-buzz/x/releases/download/v1/x.syncext"
            )
            .is_ok()
        );
    }

    /// A ledger is fetched from beside the index it was named in.
    #[test]
    fn a_ledger_sits_beside_the_index_it_belongs_to() {
        let registry = Registry::at(PathBuf::from("/nowhere"));
        assert_eq!(
            registry.ledger_url("project-memory"),
            "https://raw.githubusercontent.com/sync-buzz/sync-extensions/main/registry/project-memory.json",
        );

        // An index pointed elsewhere takes its ledgers with it, which is what
        // makes the pair testable without the published registry.
        let elsewhere = Registry::at(PathBuf::from("/nowhere"))
            .from("https://example.test/some/where/registry.json".to_owned());
        assert_eq!(
            elsewhere.ledger_url("records"),
            "https://example.test/some/where/registry/records.json",
        );
    }

    /// An id off the network becomes a path segment and a file name, so it is
    /// held to the manifest's own rule before either is built out of it.
    #[test]
    fn an_id_that_could_be_a_path_is_never_asked_about() {
        let registry = Registry::at(PathBuf::from("/nowhere"));
        for nonsense in ["../../etc/passwd", "a/b", "", "Records", "-leading"] {
            assert!(
                matches!(registry.ledger(nonsense), Err(RegistryError::Nameless(_))),
                "{nonsense} was asked about",
            );
        }
    }

    /// A machine that has never fetched an index has nothing to say about
    /// updates, and says it with `None` rather than with a failure.
    #[test]
    fn a_cached_index_nobody_has_fetched_is_absent_rather_than_an_error() {
        let root = tempfile::tempdir().expect("a directory");
        let registry = Registry::at(root.path().to_path_buf());
        assert!(registry.cached_index().expect("reads").is_none());
    }

    #[test]
    fn an_index_from_a_newer_registry_says_so_rather_than_naming_a_field() {
        let error =
            parse::<Index>(br#"{"formatVersion": 2, "whatever": true}"#).expect_err("refused");
        assert!(matches!(error, RegistryError::Newer { found: 2 }));
    }

    #[test]
    fn an_index_reads_the_shape_the_generator_writes() {
        let index: Index = parse(
            br#"{
              "formatVersion": 1,
              "extensions": [{
                "id": "records", "name": "Records", "summary": "s", "icon": "shapes",
                "version": "1.0.0", "syncApi": "^2.0", "capabilities": ["records"],
                "requires": [], "publishes": [], "areas": [{"id": "records", "label": "Records"}],
                "prompt": false, "npm": [], "author": null, "license": "MIT",
                "repository": null,
                "artefact": {"url": "https://github.com/a/b/c.syncext", "sha256": "ab", "bytes": 12}
              }]
            }"#,
        )
        .expect("valid");
        assert_eq!(index.extensions.len(), 1);
        assert_eq!(index.extensions[0].sync_api, "^2.0");
        assert_eq!(index.extensions[0].areas[0].label, "Records");
    }

    /// A field a newer registry added is not a reason to refuse the whole index.
    ///
    /// The opposite of the manifest's rule, and deliberately: a manifest is one
    /// package describing itself to a build that has to run it, while this is a
    /// list, and refusing the list because one entry gained a field would take
    /// away every other entry too. `formatVersion` is what says the shape has
    /// changed in a way that matters.
    #[test]
    fn a_field_this_build_has_no_reading_for_is_ignored() {
        let index: Index = parse(
            br#"{"formatVersion": 1, "sponsored": [], "extensions": [{
              "id": "a", "name": "A", "version": "1.0.0", "syncApi": "^2.0",
              "featured": true,
              "artefact": {"url": "u", "sha256": "s", "bytes": 1}
            }]}"#,
        )
        .expect("valid");
        assert_eq!(index.extensions[0].id, "a");
    }
}
