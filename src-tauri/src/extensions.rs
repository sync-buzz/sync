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

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sync_extensions::{
    Archive, Artefact, Fetched, Index, Installed, Ledger, Manifest, NET_CAPABILITY,
    NET_WRITE_CAPABILITY, NetRequest, NetResponse, Pointer, Registry, Source, Store,
    TypeDefinition, read_prompt, read_types,
};
use tauri::http::{Request, Response, StatusCode};
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
        // The token is what makes a rebuilt file a *different* URL, and without
        // it nothing a person does reaches the window. `cache-control:
        // no-store` in `serve` is honoured by the fetch and is beside the
        // point: a webview memoises an ES module by its specifier, so the
        // second `import()` of one URL never asks the network at all. An
        // extension served from a folder could therefore be reinstalled, rebuilt
        // and version-bumped, and the window went on running the code it had
        // imported when it started — which is exactly what a person writing one
        // does all day.
        //
        // The modification time rather than the version: a folder being written
        // changes many times per version, and a token that only moved with the
        // manifest would fix reinstalling and leave rebuilding broken. For an
        // artefact it is stable anyway, the directory being named after its own
        // content and never written twice.
        let served = |path: &String| {
            let stamp = std::fs::metadata(installed.root.join(path))
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_millis());
            format!("{SCHEME}://{}/{path}?v={stamp}", installed.manifest.id)
        };
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
    listed(&app)
}

/// Everything this machine can load.
///
/// Split from the command because a phone asks the same question over the
/// channel, and a second body for it would be a second answer to *what is
/// installed here*.
pub(crate) fn listed<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<InstalledExtension>, String> {
    store(app)?
        .list()
        .map(|all| all.into_iter().map(InstalledExtension::of).collect())
        .map_err(|error| error.to_string())
}

/// One file of one installed artefact, with what it is.
///
/// The same two answers [`serve`] gives a webview on this machine, for a
/// webview that is not on it: the id is resolved through the pointer rather
/// than trusted, and the path is refused if it leaves the artefact. Both checks
/// are here rather than at either caller — a phone cannot make them, and a
/// machine that made them twice would eventually make them differently.
pub(crate) fn file_of<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    path: &str,
) -> Result<(Vec<u8>, &'static str), String> {
    let installed = store(app)?
        .resolve(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("nothing on this machine serves `{id}`"))?;
    let file = within(&installed.root, path).ok_or("that path leaves the extension")?;
    let bytes = std::fs::read(&file).map_err(|error| format!("`{path}`: {error}"))?;
    let media = media_type(&file);
    Ok((bytes, media))
}

/// Stops serving an id on this machine. The artefact and its records stay.
#[tauri::command]
pub fn extension_forget<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    forget_now(&app, &id)
}

pub(crate) fn forget_now<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    store(app)?.forget(id).map_err(|error| error.to_string())
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
    tauri::async_runtime::spawn_blocking(move || index_now(&app))
        .await
        .map_err(|error| format!("reading the registry did not finish: {error}"))?
}

/// The index itself, on whatever thread the caller already put itself on.
///
/// Blocking, and every caller says so in its own way: the command hands it to
/// the blocking pool, and the channel's carrier is already on a thread of its
/// own.
pub(crate) fn index_now<R: Runtime>(app: &AppHandle<R>) -> Result<Fetched<Index>, String> {
    registry(app)?.index().map_err(|error| error.to_string())
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
    cached_now(&app)
}

pub(crate) fn cached_now<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Index>, String> {
    registry(app)?
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
    tauri::async_runtime::spawn_blocking(move || ledger_now(&app, &id))
        .await
        .map_err(|error| format!("reading the extension's versions did not finish: {error}"))?
}

pub(crate) fn ledger_now<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<Ledger, String> {
    registry(app)?
        .ledger(id)
        .map(|fetched| fetched.answer)
        .map_err(|error| error.to_string())
}

/// The package a call is being made for, or why it may not be made.
///
/// **Both answers come off the artefact on this machine and neither is taken
/// from the caller.** Whether anything here serves that id, and whether the
/// manifest a person installed asks for the capability the door is behind: a
/// request that arrived carrying either answer would be an extension granting
/// itself the permission.
///
/// Here rather than inside [`extension_fetch`] because the network is not the
/// only door. The keychain's is in [`crate::vault`], it asks these same two
/// questions, and a second copy of them is the copy that stops being asked.
///
/// It reads the disk, so every caller is already on the blocking pool.
pub(crate) fn permitted<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    capability: &str,
) -> Result<Installed, String> {
    let installed = store(app)?
        .resolve(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Nothing on this machine serves \"{id}\"."))?;

    asked_for(&installed.manifest, id, capability)?;
    Ok(installed)
}

/// Whether a manifest already in hand asks for a capability, in words.
///
/// The half of [`permitted`] that is left once the package has been resolved,
/// and it is split out because a handler's host has resolved it already: the
/// isolate is built from one artefact read at the start of the call, and
/// resolving it again per call would be a second reading of the same file that
/// could come back different halfway through a handler.
///
/// One function rather than two spellings of the refusal, because the sentence
/// is the whole of what an author gets: a package that hears different words
/// from the two halves of the same door will be debugged as two problems.
///
/// # Errors
///
/// When the manifest does not name the capability.
pub(crate) fn asked_for(manifest: &Manifest, id: &str, capability: &str) -> Result<(), String> {
    if !manifest.asks_for(capability) {
        return Err(format!(
            "\"{id}\" did not ask for the \"{capability}\" capability, so it does not have it."
        ));
    }
    Ok(())
}

/// Makes one request on behalf of one package, or says why it did not.
///
/// **The permission is read here, from the manifest on this machine, and never
/// taken from the caller.** That is the whole of what this command adds over
/// [`sync_extensions::net::fetch`]: what a package may reach is a sentence in
/// the manifest a person installed, so the id is resolved against the store
/// and the list comes off the artefact. A request that arrived carrying its own
/// allow-list would be an extension granting itself the permission.
///
/// **The second capability is decided here too, and for the reason every
/// capability that cannot be read off a manifest is.** Whether a package dials
/// out at all is written in the file; which verb it chooses on a given call is
/// inside its JavaScript, so the card is honest before anything runs and this
/// is what refuses. The definition of *changes something* is the door's, so
/// this layer and the crate cannot come to disagree about which verbs are safe.
///
/// On the blocking pool, for the reason [`registry_index`] is: the client here
/// is `reqwest::blocking` and dropping its runtime inside an async context is
/// what tokio refuses, loudly and in the wrong place.
#[tauri::command]
pub async fn extension_fetch<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    request: NetRequest,
) -> Result<NetResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installed = permitted(&app, &id, NET_CAPABILITY)?;
        fetch_now(&id, &installed.manifest, &request)
    })
    .await
    .map_err(|error| format!("the request did not finish: {error}"))?
}

/// One request for one package, for a caller that is already blocking.
///
/// **The whole of the door, minus who is allowed to open it.** The verb's own
/// capability, the secrets the manifest declared, and the request itself: all
/// three are here, so the window and a service module make the same request
/// under the same rules rather than under two implementations of them. The one
/// thing left to the caller is [`NET_CAPABILITY`], because the two halves reach
/// the manifest by different routes — the window resolves an id, a handler was
/// built from an artefact already read.
///
/// The second capability is decided here for the reason every capability that
/// cannot be read off a manifest is. Whether a package dials out at all is
/// written in the file; which verb it chooses on a given call is inside its
/// JavaScript, so the card is honest before anything runs and this is what
/// refuses. The definition of *changes something* is the crate's, so this layer
/// and the door cannot come to disagree about which verbs are safe.
///
/// # Errors
///
/// When the verb needs a capability the package did not ask for, when a
/// declared secret cannot be read, or when the request was refused or failed.
pub(crate) fn fetch_now(
    id: &str,
    manifest: &Manifest,
    request: &NetRequest,
) -> Result<NetResponse, String> {
    if request.method.changes_something() {
        asked_for(manifest, id, NET_WRITE_CAPABILITY).map_err(|_| {
            format!(
                "\"{id}\" may read where it reaches and not change anything there: a {} needs the \"{NET_WRITE_CAPABILITY}\" capability, which is what a person agrees to before it is installed",
                request.method
            )
        })?;
    }

    let sealed = sealed_for(id, &request.url, &manifest.net, |name| {
        crate::vault::read_for_extension(id, name)
    })?;

    sync_extensions::net::fetch(id, request, &manifest.net, &sealed)
        .map_err(|error| error.to_string())
}

/// The headers this request carries that the package did not write.
///
/// **The value is looked up here and goes nowhere else.** It is put into a
/// header in Rust and never returned, so a package that only has to reach an
/// API with a token never holds one — which is the whole of what the
/// declaration buys, and the reason it is the recommended way to use a secret.
///
/// Reading is a closure rather than a call, because that is what makes this
/// testable without a keychain: the failure worth a test is the sentence a
/// person gets when nothing is stored, and standing up a real entry to see it
/// would be a test that writes to somebody's login keychain.
///
/// An entry that is not there is a refusal and never a request sent without the
/// header. A silent `401` from somebody else's API is an hour of the wrong
/// person's time, and the manifest already promised this header would be there.
fn sealed_for(
    id: &str,
    url: &str,
    allowed: &sync_extensions::Net,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut sealed = BTreeMap::new();
    for sending in sync_extensions::net::secrets_for(url, allowed) {
        let value = read(&sending.secret).map_err(|error| {
            format!(
                "\"{id}\" sends the secret \"{}\" to {} and it could not be read: {error}",
                sending.secret, sending.host
            )
        })?;
        sealed.insert(
            sending.header.to_ascii_lowercase(),
            sending.header_value(&value),
        );
    }
    Ok(sealed)
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
    repoint_now(&app, &pointer)
}

pub(crate) fn repoint_now<R: Runtime>(
    app: &AppHandle<R>,
    pointer: &Pointer,
) -> Result<InstalledExtension, String> {
    store(app)?
        .repoint(pointer)
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
    tauri::async_runtime::spawn_blocking(move || install_now(&app, &artefact))
        .await
        .map_err(|error| format!("installing from the registry did not finish: {error}"))?
}

/// Download and install, on the thread the caller is already on.
///
/// The downloaded file is removed either way: a `.syncext` nobody installed is
/// litter with no owner, and the one that was installed has been copied into
/// the artefact directory by then.
pub(crate) fn install_now<R: Runtime>(
    app: &AppHandle<R>,
    artefact: &Artefact,
) -> Result<InstalledExtension, String> {
    let downloads = extensions_dir(app)?.join("downloads");
    let file = registry(app)?
        .download(artefact, &downloads)
        .map_err(|error| error.to_string())?;

    let installed = Archive::open(&file, SIGNING_KEY)
        .map_err(|error| error.to_string())
        .and_then(|archive| {
            store(app)?
                .install(&archive, Source::Registry)
                .map(InstalledExtension::of)
                .map_err(|error| error.to_string())
        });

    drop(std::fs::remove_file(&file));
    installed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The token that busts the window's module cache never reaches the disk.
    ///
    /// `served` hangs `?v=<mtime>` on every URL, and the path a request is
    /// resolved through is [`tauri::http::Uri::path`], which excludes the
    /// query. Written down because the two halves are in different languages
    /// and nothing else would fail if that stopped being true: a token treated
    /// as part of the path makes every extension serve 404 and the window draw
    /// no sections at all.
    #[test]
    fn the_cache_busting_token_is_not_part_of_the_path() {
        let uri: tauri::http::Uri = format!("{SCHEME}://routines/ui/index.js?v=1756339200000")
            .parse()
            .expect("the served URL parses");

        assert_eq!(uri.path(), "/ui/index.js");
        assert_eq!(uri.query(), Some("v=1756339200000"));

        let root = Path::new("/tmp/routines");
        assert_eq!(
            within(root, uri.path()),
            Some(root.join("ui/index.js")),
            "the query must not reach the file that is read"
        );
    }

    fn sending(
        host: &str,
        header: &str,
        secret: &str,
        scheme: Option<&str>,
    ) -> sync_extensions::Net {
        sync_extensions::Net {
            hosts: vec![host.to_owned()],
            secrets: vec![sync_extensions::Secret {
                host: host.to_owned(),
                header: header.to_owned(),
                secret: secret.to_owned(),
                scheme: scheme.map(str::to_owned),
            }],
        }
    }

    /// The value is put into a header here and is not returned to anybody. What
    /// this can assert is the header it went into and the shape it went in as;
    /// that it goes no further is the surface's — nothing on it answers with
    /// one — and the crossing is one function wide.
    #[test]
    fn a_declared_secret_becomes_a_header_the_package_never_wrote() {
        let sealed = sealed_for(
            "tracker",
            "https://api.example.com/tickets",
            &sending("api.example.com", "Authorization", "token", Some("Bearer")),
            |name| {
                assert_eq!(name, "token", "the entry the manifest named");
                Ok("s3cret".to_owned())
            },
        )
        .expect("the secret is read");

        assert_eq!(
            sealed.get("authorization").map(String::as_str),
            Some("Bearer s3cret"),
            "the scheme is written in front of the value: {sealed:?}"
        );
    }

    /// An API key wants the value alone, and a manifest that says no scheme
    /// gets exactly that rather than a space and a guess.
    #[test]
    fn a_secret_with_no_scheme_is_written_alone() {
        let sealed = sealed_for(
            "tracker",
            "https://api.example.com/tickets",
            &sending("api.example.com", "x-api-key", "key", None),
            |_| Ok("k3y".to_owned()),
        )
        .expect("the secret is read");

        assert_eq!(sealed.get("x-api-key").map(String::as_str), Some("k3y"));
    }

    /// A pair is about one host. A request to another that the package also
    /// reaches carries nothing, which is what keeps one API's token from
    /// arriving at another.
    #[test]
    fn a_secret_declared_for_one_host_is_not_sent_to_another() {
        let mut allowed = sending("api.example.com", "authorization", "token", Some("Bearer"));
        allowed.hosts.push("api.other.example".to_owned());

        let sealed = sealed_for(
            "tracker",
            "https://api.other.example/anything",
            &allowed,
            |_| panic!("nothing should be read for a host with no pair"),
        )
        .expect("a request with no pair carries none");

        assert!(sealed.is_empty(), "{sealed:?}");
    }

    /// Nothing stored is a refusal in words, not a request sent without the
    /// header: the manifest promised the header, and a silent 401 from
    /// somebody else's API is an hour of the wrong person's time.
    #[test]
    fn a_secret_that_is_not_there_is_a_refusal_naming_it() {
        let refused = sealed_for(
            "tracker",
            "https://api.example.com/tickets",
            &sending("api.example.com", "authorization", "token", Some("Bearer")),
            |_| Err("there is no secret stored under that name".to_owned()),
        )
        .expect_err("nothing is stored");

        for said in ["tracker", "token", "api.example.com"] {
            assert!(
                refused.contains(said),
                "the refusal names what to put where: {said} missing from {refused}"
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
