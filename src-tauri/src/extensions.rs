//! Extensions, as the window reaches them.
//!
//! A thin adapter over `sync-extensions`, in the shape every command layer in
//! this crate has: parse input, call a domain function, map the result. The
//! decisions — what a manifest may say, what makes an archive believable, what
//! an artefact directory is — are all in the crate, which knows nothing about
//! Tauri and is tested without it.
//!
//! Two things live here because they are the desktop's rather than the domain's:
//! the URI scheme an extension's files are served under, and where the artefact
//! directory is on this machine.
//!
//! This grew out of `extension_probe.rs`, whose one question — will a packaged
//! build execute a module from outside the bundle — was answered on 2026-08-24.
//! The answer and its surprise are recorded in `docs/extensions.md` §5: the
//! Content-Security-Policy is not the obstacle, CORS is, and the response has to
//! carry `Access-Control-Allow-Origin` or the webview discards a body it has
//! already received.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sync_extensions::{
    Answer, Archive, Artefact, Fetched, Index, Installed, Ledger, Manifest, NET_CAPABILITY,
    Pointer, Registry, Source, Store, TypeDefinition, read_prompt, read_types,
};
use tauri::http::{Request, Response, StatusCode};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime, UriSchemeContext};

/// The scheme an unpacked extension's files are served under.
///
/// Not `asset:` — that one is the asset protocol's, scoped by a capability and
/// meant for files a person chose. This serves code, from a directory the
/// application owns, and keeping it separate is what lets the policy name it:
/// `script-src` in `tauri.conf.json` names this scheme and nothing else new.
pub const SCHEME: &str = "syncext";

/// The origins this window is served under.
///
/// A module fetched from another origin is a CORS request, and the webview
/// refuses it unless the response says who may read it. `*` would say "anyone",
/// which is a larger answer than the question: nothing but this window is meant
/// to reach an artefact, so the two origins Tauri serves a window under are
/// named and everything else is refused.
const WINDOW_ORIGINS: [&str; 2] = ["tauri://localhost", "http://tauri.localhost"];

/// Whether an origin is this window's own.
///
/// In a packaged build that is the two strings above and nothing else. `tauri
/// dev` does not serve the window from the bundle at all — it points the
/// webview at a dev server — so the origin is whatever `devUrl` says, and none
/// of the two match it.
///
/// **That gap is why §5 of `docs/extensions.md` was measured true and was false
/// where it mattered.** The probe ran against a packaged build, where the
/// origin is `tauri://localhost` and a module loads; every extension loaded in
/// `tauri dev` was refused with *"Cross-origin script load denied"* — the very
/// sentence that document warns names neither the policy nor the scheme. The
/// one loop an extension author actually works in was the one loop the
/// mechanism had never been run in.
///
/// The dev origin is read from the configuration rather than written down here,
/// because a port kept in two files is a port that will come to disagree with
/// itself. It is honoured **only in a debug build**: `devUrl` survives into a
/// release configuration, and trusting it there would let anything served from
/// that port read artefacts out of the app data directory.
fn is_window_origin<R: Runtime>(app: &AppHandle<R>, origin: &str) -> bool {
    #[cfg(debug_assertions)]
    let dev = app
        .config()
        .build
        .dev_url
        .as_ref()
        .map(|url| url.origin().ascii_serialization());

    #[cfg(not(debug_assertions))]
    let dev = {
        // The configuration still carries `devUrl` in a release build, and it
        // is deliberately not read: this is where the widening would be.
        let _ = app;
        None::<String>
    };

    origin_is_the_window(origin, dev.as_deref())
}

/// The rule itself, with the one thing that varies passed in.
///
/// Split out so it can be stated as a test rather than reasoned about: the
/// whole defect was a rule that was only ever exercised in one of the two
/// builds it has to hold in.
fn origin_is_the_window(origin: &str, dev: Option<&str>) -> bool {
    WINDOW_ORIGINS.contains(&origin) || dev == Some(origin)
}

/// The key `.syncext` signatures are checked against, once there is one.
///
/// `None` in v0, and the format is what matters rather than the gate: hashes
/// always gate, a signature is reported and does not, and turning the gate on
/// later changes a policy rather than an archive format. The updater's key is
/// deliberately not reused — packages and application updates are signed by
/// different pipelines, and sharing a key would make them one blast radius.
const SIGNING_KEY: Option<&str> = None;

/// Where unpacked artefacts live: the machine's, shared by every project.
/// `pub(crate)` rather than private because `handlers.rs` resolves a package to
/// read its service module out of the artefact, which is the same question this
/// answers for every command here — and a second way of finding an artefact
/// directory is a second place the layout could be got wrong.
pub(crate) fn store<R: Runtime>(app: &AppHandle<R>) -> Result<Store, String> {
    extensions_dir(app).map(Store::at)
}

/// The registry, and the one cached file it keeps beside the artefacts.
fn registry<R: Runtime>(app: &AppHandle<R>) -> Result<Registry, String> {
    extensions_dir(app).map(|dir| Registry::at(dir.join("registry")))
}

fn extensions_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("extensions"))
        .map_err(|error| error.to_string())
}

/// Where the archives this build ships with live inside the bundle.
///
/// Filled by `pnpm extensions:seed` before a release, from the published
/// registry, and empty in a checkout where nobody has run it. Empty is not an
/// error: the archives are release artefacts of another repository, and a
/// developer's build is allowed to be without them.
const SEEDED: &str = "resources/extensions";

/// Unpack the extensions this build ships with, on a machine that lacks them.
///
/// The reason this exists is a first launch with no network. Nothing here is
/// built into the application — the code is not in this tree, the archives were
/// compiled by the registry's CI, and they install through the ordinary path
/// and update from the registry afterwards. Seeding is only about *when* the
/// bytes arrive, which is why the archives are resources rather than modules.
///
/// An id something already serves is left alone; see `Store::seed`. Answers
/// with what was unpacked, which is empty on every launch after the first.
///
/// # Errors
///
/// When the artefact directory cannot be reached, or an archive this build
/// ships with is not readable — which is a defect in the build rather than
/// anything a person did, and is why it is not swallowed.
pub fn seed<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<String>, String> {
    let directory = app
        .path()
        .resolve(SEEDED, BaseDirectory::Resource)
        .map_err(|error| error.to_string())?;
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };

    let store = store(app)?;
    let mut seeded = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("syncext") {
            continue;
        }
        let archive = Archive::open(&path, SIGNING_KEY)
            .map_err(|error| format!("{} is not a readable package: {error}", path.display()))?;
        let id = archive.manifest().id.clone();
        if store
            .seed(&archive)
            .map_err(|error| format!("{id} could not be unpacked: {error}"))?
            .is_some()
        {
            seeded.push(id);
        }
    }
    Ok(seeded)
}

// ---------------------------------------------------------------------------
// Serving an extension's own files.
// ---------------------------------------------------------------------------

fn refused(status: StatusCode, why: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(why.as_bytes().to_vec())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// What a file is, as the webview needs to be told.
///
/// A module served as `application/octet-stream` is fetched and then refused by
/// the module loader, which reports it as a network error and sends whoever is
/// reading the console looking in the wrong place.
fn media_type(path: &Path) -> &'static str {
    match path.extension().and_then(|it| it.to_str()) {
        Some("js" | "mjs") => "text/javascript",
        Some("json") => "application/json",
        Some("css") => "text/css",
        Some("md") => "text/markdown",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// Whether a request's path stays inside the extension it names.
///
/// Rejecting the components outright rather than canonicalising and comparing
/// means there is no window between the check and the read for a symlink to be
/// swapped in.
fn within(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path.trim_start_matches('/'));
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(relative))
}

/// Serves one file of one installed extension.
///
/// `syncext://<id>/<path>`, where the id is resolved through the pointer rather
/// than trusted: an extension that is not installed serves nothing, whatever is
/// on the disk under that name.
pub fn serve<R: Runtime>(
    context: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let uri = request.uri();
    let app = context.app_handle();

    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    // An absent Origin is not a cross-origin request and needs no permission; a
    // present one that is not this window's is somebody else asking.
    let allowed = if origin.is_empty() {
        None
    } else if is_window_origin(app, origin) {
        Some(origin.to_string())
    } else {
        return refused(StatusCode::FORBIDDEN, "that origin may not read artefacts");
    };

    let Some(id) = uri.host().filter(|host| !host.is_empty()) else {
        return refused(StatusCode::BAD_REQUEST, "no extension named in the URI");
    };

    let installed = match store(app).map(|store| store.resolve(id)) {
        Ok(Ok(Some(installed))) => installed,
        Ok(Ok(None)) => return refused(StatusCode::NOT_FOUND, "no such extension is installed"),
        Ok(Err(error)) => return refused(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => return refused(StatusCode::INTERNAL_SERVER_ERROR, &error),
    };

    let Some(file) = within(&installed.root, uri.path()) else {
        return refused(StatusCode::FORBIDDEN, "that path leaves the extension");
    };

    match std::fs::read(&file) {
        Ok(bytes) => {
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", media_type(&file))
                // An artefact directory is named after its content, so a URL
                // names exactly one immutable file. A folder being written is
                // the exception, and it is the one that must not be cached —
                // so nothing is, and the cost is a disk read.
                .header("cache-control", "no-store");
            if let Some(origin) = allowed {
                response = response.header("access-control-allow-origin", origin);
            }
            response
                .body(bytes)
                .unwrap_or_else(|_| refused(StatusCode::INTERNAL_SERVER_ERROR, "unreadable"))
        }
        Err(error) => refused(StatusCode::NOT_FOUND, &error.to_string()),
    }
}

// ---------------------------------------------------------------------------
// What the window is told about a package.
// ---------------------------------------------------------------------------

/// One installed extension, as the catalogue reads it.
///
/// The manifest whole, rather than a summary of it: the window decides
/// compatibility, draws the card and mounts the areas, and every one of those
/// needs a different part of it. A curated subset here would be a third place
/// the manifest's shape is written down.
///
/// The vocabulary and the prompt are read out of the artefact and carried here
/// because the window cannot read them itself: a file inside an artefact is
/// reachable only over `syncext://`, and fetching one would mean widening the
/// webview's `connect-src`. It also has to work for a package with no code at
/// all — an extension that publishes only a vocabulary never loads a module,
/// and its types still have to reach the project.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub manifest: Manifest,
    pub pointer: Pointer,
    /// Where its UI is, ready to be imported, and `None` for a package that
    /// ships none. The window never builds this string itself — the scheme is
    /// the desktop's business.
    pub ui: Option<String>,
    /// Where its stylesheet is, and `None` for a package that ships none.
    ///
    /// Served over the same scheme and resolved the same way, because it is the
    /// same kind of thing: a file inside an artefact that the window has to
    /// fetch. The window adds it to the document when it loads the module and
    /// takes it away with it.
    pub styles: Option<String>,
    /// The types installing it would publish, as the engine asks for them.
    pub types: Vec<TypeDefinition>,
    /// What it tells an agent, whole.
    pub prompt: Option<String>,
    /// Why this package cannot be used, when it cannot.
    ///
    /// A package whose manifest parsed and whose type definitions did not is
    /// still a package a person can see and remove, so it is listed with the
    /// reason rather than dropped from the list. Dropping it would present a
    /// broken package as one that was never installed, and the two are
    /// different problems with different answers.
    pub defect: Option<String>,
}

impl InstalledExtension {
    fn of(installed: Installed) -> Self {
        let served = |path: &String| format!("{SCHEME}://{}/{path}", installed.manifest.id);
        let ui = installed.manifest.ui.as_ref().map(&served);
        let styles = installed.manifest.styles.as_ref().map(&served);

        let (types, prompt, defect) = match read_types(&installed.root, &installed.manifest)
            .and_then(|types| {
                read_prompt(&installed.root, &installed.manifest).map(|prompt| (types, prompt))
            }) {
            Ok((types, prompt)) => (types, prompt, None),
            Err(error) => (Vec::new(), None, Some(error.to_string())),
        };

        Self {
            manifest: installed.manifest,
            pointer: installed.pointer,
            ui,
            styles,
            types,
            prompt,
            defect,
        }
    }
}

// ---------------------------------------------------------------------------
// Commands.
// ---------------------------------------------------------------------------

/// Installs a `.syncext` a person chose in the open panel.
#[tauri::command]
pub fn extension_install_file<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> Result<InstalledExtension, String> {
    let archive =
        Archive::open(Path::new(&path), SIGNING_KEY).map_err(|error| error.to_string())?;
    store(&app)?
        .install(&archive, Source::File)
        .map(InstalledExtension::of)
        .map_err(|error| error.to_string())
}

/// Points an id at a folder somebody is writing in.
///
/// The path is not written into any project: a project declares `{id, version}`
/// and that declaration travels with the repository, where an absolute path
/// from one machine is noise at best. Which folder serves an id is a fact about
/// this machine and stays in the artefact directory.
#[tauri::command]
pub fn extension_install_folder<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> Result<InstalledExtension, String> {
    store(&app)?
        .install_folder(Path::new(&path))
        .map(InstalledExtension::of)
        .map_err(|error| error.to_string())
}

/// Everything this machine can load, whatever any project declares.
#[tauri::command]
pub fn extension_list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<InstalledExtension>, String> {
    store(&app)?
        .list()
        .map(|all| all.into_iter().map(InstalledExtension::of).collect())
        .map_err(|error| error.to_string())
}

/// Stops serving an id on this machine. The artefact and its records stay.
#[tauri::command]
pub fn extension_forget<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    store(&app)?.forget(&id).map_err(|error| error.to_string())
}

/// What the registry says exists, from the network or from what was cached.
///
/// Off the main thread, and off the async one as well.
///
/// `#[tauri::command(async)]` was the first answer and it was wrong in a way
/// that only showed at runtime: it means `tokio::spawn`, so the body runs *in*
/// an async context rather than beside one. The HTTP client here is
/// `reqwest::blocking`, which carries its own tokio runtime and drops it when
/// it goes — and dropping a runtime inside an async context is what tokio
/// refuses, loudly, on a thread called `tokio-rt-worker`. The panic did not
/// take the window with it; it took the catalogue, silently, which is worse.
///
/// `spawn_blocking` is the pool that exists for exactly this. Nothing else
/// changes: the body is the same ordinary blocking code, and the window is
/// still not held while a request is in flight.
#[tauri::command]
pub async fn registry_index<R: Runtime>(app: AppHandle<R>) -> Result<Fetched<Index>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        registry(&app)?.index().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("reading the registry did not finish: {error}"))?
}

/// What the last fetch left on the disk, and nothing over the network.
///
/// The window asks this when a project opens, so that the pinned Extensions row
/// can carry a mark without every launch becoming a request. The distinction is
/// the whole point of the command existing beside [`registry_index`]: that one
/// is somebody opening the catalogue and asking what exists, this one is the
/// window reading what it already knows.
///
/// `None` on a machine that has never fetched an index. Not an error — there is
/// nothing to say about updates yet — and the row draws nothing.
#[tauri::command]
pub fn registry_cached_index<R: Runtime>(app: AppHandle<R>) -> Result<Option<Index>, String> {
    registry(&app)?
        .cached_index()
        .map_err(|error| error.to_string())
}

/// Every version one extension has published, with what changed in each.
///
/// Fetched when a page is opened rather than with the index: the changelog of
/// every version of everything ever published would make the one file every
/// marketplace fetches grow without limit. On the blocking pool, for the reason
/// [`registry_index`] is.
#[tauri::command]
pub async fn registry_ledger<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Ledger, String> {
    tauri::async_runtime::spawn_blocking(move || {
        registry(&app)?
            .ledger(&id)
            .map(|fetched| fetched.answer)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("reading the extension's versions did not finish: {error}"))?
}

/// Reads one URL on behalf of one package, or says why it did not.
///
/// **The permission is read here, from the manifest on this machine, and never
/// taken from the caller.** That is the whole of what this command adds over
/// [`sync_extensions::net::read`]: what a package may reach is a sentence in
/// the manifest a person installed, so the id is resolved against the store
/// and the list comes off the artefact. A request that arrived carrying its own
/// allow-list would be an extension granting itself the permission.
///
/// The capability is checked beside it rather than assumed from the list. A
/// manifest cannot have one without the other — the crate refuses that pair
/// when it parses — but the two say different things and an artefact on this
/// disk was verified before this build ever ran, so both are asked for.
///
/// On the blocking pool, for the reason [`registry_index`] is: the client here
/// is `reqwest::blocking` and dropping its runtime inside an async context is
/// what tokio refuses, loudly and in the wrong place.
#[tauri::command]
pub async fn extension_fetch<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    url: String,
) -> Result<Answer, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installed = store(&app)?
            .resolve(&id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Nothing on this machine serves \"{id}\"."))?;

        if !installed
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == NET_CAPABILITY)
        {
            return Err(format!(
                "\"{id}\" did not ask for the network, so it does not have it."
            ));
        }

        sync_extensions::net::read(&id, &url, &installed.manifest.net)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("the request did not finish: {error}"))?
}

/// Points an id back at the artefact it was serving before an update.
///
/// The last step of applying one, and only when a later step failed. What
/// follows the pointer moving happens in the window — the types are published
/// into the project's memory and the version is written into its record — and a
/// failure there would otherwise leave a project declaring one version while
/// this machine serves another.
///
/// The whole pointer rather than an id and a version, because the source and
/// the signature state were established when the archive was verified and the
/// archive is deleted once it is unpacked. The window hands back exactly what
/// it was given.
#[tauri::command]
pub fn extension_repoint<R: Runtime>(
    app: AppHandle<R>,
    pointer: Pointer,
) -> Result<InstalledExtension, String> {
    store(&app)?
        .repoint(&pointer)
        .map(InstalledExtension::of)
        .map_err(|error| error.to_string())
}

/// Downloads an artefact the index named, and installs it.
///
/// One command rather than two, because there is nothing a person could do
/// between them and a downloaded file nobody installed is litter with no owner.
/// The three checks happen in the order that makes each of them mean something:
/// the bytes are what the registry named, then the archive's own hashes cover
/// its own files, then — before this is ever called — the window has already
/// refused a package this build cannot run, which is why a card for one offers
/// no button at all.
///
/// The download is deleted whether or not the install worked. It has served its
/// purpose either way: what survives is the unpacked artefact, and keeping the
/// zip beside it would be a second copy of every extension on the machine.
/// On the blocking pool, for the reason [`registry_index`] is.
#[tauri::command]
pub async fn extension_install_registry<R: Runtime>(
    app: AppHandle<R>,
    artefact: Artefact,
) -> Result<InstalledExtension, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let downloads = extensions_dir(&app)?.join("downloads");
        let file = registry(&app)?
            .download(&artefact, &downloads)
            .map_err(|error| error.to_string())?;

        let installed = Archive::open(&file, SIGNING_KEY)
            .map_err(|error| error.to_string())
            .and_then(|archive| {
                store(&app)?
                    .install(&archive, Source::Registry)
                    .map(InstalledExtension::of)
                    .map_err(|error| error.to_string())
            });

        drop(std::fs::remove_file(&file));
        installed
    })
    .await
    .map_err(|error| format!("installing from the registry did not finish: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The archives in the tree, without Tauri's idea of where resources are.
    ///
    /// [`seed`] answers that question at runtime and cannot be called without an
    /// application; what is checked here is the half that can be wrong in the
    /// repository — an archive committed corrupt, truncated, or built by a
    /// packer this build no longer reads.
    fn shipped() -> Vec<PathBuf> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(SEEDED);
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("syncext"))
            .collect()
    }

    /// Every archive this build ships with is one this build can open.
    ///
    /// The failure this catches is a release that seeds nothing on every machine
    /// it installs on, which no other test would notice: seeding is allowed to
    /// find no archives, so an unreadable one and an absent one look the same
    /// from inside [`seed`].
    #[test]
    fn the_archives_this_build_ships_with_are_readable_and_seed_once() {
        let shipped = shipped();
        assert!(
            !shipped.is_empty(),
            "no archives are shipped — run `pnpm extensions:seed`"
        );

        let root = tempfile::tempdir().expect("a directory to seed into");
        let store = Store::at(root.path().to_path_buf());

        for path in &shipped {
            let archive = Archive::open(path, SIGNING_KEY)
                .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
            let seeded = store
                .seed(&archive)
                .unwrap_or_else(|error| panic!("{} did not seed: {error}", path.display()));
            assert!(
                seeded.is_some(),
                "{} was skipped on a machine holding nothing",
                path.display()
            );
        }

        // The second pass is the guard that keeps a launch from undoing an
        // update: everything is already served, so nothing is seeded again.
        for path in &shipped {
            let archive = Archive::open(path, SIGNING_KEY).expect("readable a second time");
            assert!(
                store.seed(&archive).expect("seeded").is_none(),
                "{} seeded over what was already there",
                path.display()
            );
        }
    }

    /// The origin a packaged window is served under is the window's.
    #[test]
    fn the_bundle_origins_are_the_window() {
        for origin in WINDOW_ORIGINS {
            assert!(origin_is_the_window(origin, None));
        }
    }

    /// And nothing else is, whatever a dev server is serving.
    #[test]
    fn another_origin_is_not_the_window() {
        assert!(!origin_is_the_window("http://evil.example", None));
        assert!(!origin_is_the_window(
            "http://evil.example",
            Some("http://localhost:1420"),
        ));
        assert!(!origin_is_the_window("null", None));
    }

    /// A dev server's origin counts only while there is one.
    ///
    /// The second half is the release build: `devUrl` is in the configuration
    /// either way, and refusing to read it there is what keeps a development
    /// port from being a way into the artefact directory of a shipped app.
    #[test]
    fn the_dev_origin_counts_only_when_there_is_one() {
        assert!(origin_is_the_window(
            "http://localhost:1420",
            Some("http://localhost:1420"),
        ));
        assert!(!origin_is_the_window("http://localhost:1420", None));
    }

    /// The origin `tauri dev` actually serves this window under is admitted.
    ///
    /// This is the test with teeth, and the one whose absence cost the loop an
    /// extension author works in: every module loaded in development was
    /// refused for a month while a packaged build loaded them fine. It reads
    /// the port out of the configuration instead of repeating it, so moving
    /// `devUrl` moves the test with it — and dropping the development branch
    /// from [`is_window_origin`] fails here rather than in front of somebody
    /// writing an extension.
    #[test]
    #[cfg(debug_assertions)]
    fn the_configured_dev_server_is_the_window_in_development() {
        let config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
                .expect("the configuration is beside the crate"),
        )
        .expect("the configuration is JSON");

        let dev_url = config["build"]["devUrl"]
            .as_str()
            .expect("`build.devUrl` names where development serves the window");
        let origin = dev_url.trim_end_matches('/');

        assert!(
            origin_is_the_window(origin, Some(origin)),
            "{origin} is where `tauri dev` serves the window and must reach artefacts",
        );
        assert!(
            !WINDOW_ORIGINS.contains(&origin),
            "{origin} is not a bundle origin — if it were, this rule would be moot",
        );
    }

    #[test]
    fn a_path_that_climbs_out_is_refused() {
        let root = Path::new("/artefacts/abc");
        assert!(within(root, "/../../etc/passwd").is_none());
        assert!(within(root, "/ui/../../escape.js").is_none());
    }

    #[test]
    fn an_ordinary_path_resolves_under_the_artefact() {
        let root = Path::new("/artefacts/abc");
        assert_eq!(
            within(root, "/ui/index.js"),
            Some(PathBuf::from("/artefacts/abc/ui/index.js")),
        );
    }

    #[test]
    fn a_module_is_served_as_javascript() {
        assert_eq!(media_type(Path::new("ui/index.js")), "text/javascript");
        assert_eq!(media_type(Path::new("manifest.json")), "application/json");
    }
}
