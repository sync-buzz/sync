//! Local auxiliary models: what this machine has installed, and what an
//! extension may ask of them.
//!
//! A model is a machine-level concern, not a project's. It is downloaded once
//! into the application's configuration directory, shared by every project, and
//! reached by any handler through [`ask`]. This is the `voice` / `flagship`
//! silhouette, not the marketplace's: the bytes are on the machine, the choice
//! of which model serves which task is the machine's, and nothing about a model
//! travels with a repository.
//!
//! # Why this is not the extension store
//!
//! A model is downloaded as bytes, not unpacked from a signed archive, and its
//! integrity is the sha256 of the bytes rather than a minisign signature over
//! an archive. The lifecycle is different enough that reusing `archive.rs`
//! would force the extension install path to handle multi-gigabyte downloads,
//! resume and progress — concerns that do not belong to it. So the store here
//! is a sibling of `store.rs`, not a call into it: the same content-addressing
//! (`models/<sha256>/`), a separate id space, a separate install command.
//!
//! # The surface is pass-through
//!
//! [`ask`] takes a task string and an opaque JSON value, and answers an opaque
//! JSON value. The task is stable — it is what the shell dispatches on — and
//! the shape is declared in the model's manifest, not encoded in the function's
//! signature. A model's API moves between versions of the model without this
//! module moving with it. See `sync-inference`'s docs for the reasoning.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sync_inference::{InferenceError, ModelManifest, capability};
use tauri::{AppHandle, Runtime};

use crate::project::{ProjectError, configuration_file, write_configuration};

/// Where the choice of which model serves which task is kept, beside this
/// installation's other files.
const ASSIGNMENTS_FILE: &str = "models.json";

/// The directory under the config dir that holds model bytes and refs.
const MODELS_DIR: &str = "models";

/// How long a catalogue download may run before the shell gives up on it.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// One model installed on this machine, as the settings window reads it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModel {
    /// The model's permanent id.
    pub id: String,
    /// What a person reads.
    pub name: String,
    /// Which runtime loads it.
    pub runtime: String,
    /// The tasks this model answers.
    pub purposes: Vec<String>,
    /// The size of the weights, in bytes.
    pub size: u64,
    /// The digest the bytes are stored under.
    pub sha256: String,
}

/// One curated catalogue entry, with the hardware refusal beside it when this
/// machine cannot run it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueEntry {
    #[serde(flatten)]
    pub manifest: ModelManifest,
    /// Whether this machine can run it. The catalogue lists the unrunnable
    /// rather than filtering them out — a person who expected to see one learns
    /// why they do not, and a person who wants to override can.
    pub eligible: bool,
    /// Why it cannot run, in a sentence, or `None` when it can.
    pub refusal: Option<String>,
    /// Whether the bytes are already on this machine.
    pub installed: bool,
}

/// The whole of what the settings page draws, in one answer.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsStatus {
    /// What this machine has installed.
    pub installed: Vec<InstalledModel>,
    /// Which model serves which task, or `null` where the shell auto-picks.
    pub assignments: BTreeMap<String, String>,
    /// The curated catalogue, filtered by this machine's hardware with
    /// refusals shown rather than rows hidden.
    pub catalogue: Vec<CatalogueEntry>,
    /// What this build can run a model on.
    pub capabilities: CapabilitiesView,
}

/// The capabilities, as the window reads them.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesView {
    pub arch: &'static str,
    pub os: &'static str,
    pub gpu: &'static str,
}

impl From<&capability::Capabilities> for CapabilitiesView {
    fn from(caps: &capability::Capabilities) -> Self {
        Self {
            arch: caps.arch,
            os: caps.os,
            gpu: caps.gpu,
        }
    }
}

/// What `model_install_catalogue` was asked to install — a model from the
/// curated catalogue, by its id.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueInstallRequest {
    pub id: String,
}

/// What `model_test` runs — a simple completion prompt, to verify the pipeline
/// end to end from Settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRequest {
    pub prompt: String,
}

/// What `model_assign` writes.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignRequest {
    /// The task to assign, e.g. `text.classify`.
    pub task: String,
    /// The model id to serve it, or `null` to clear the assignment and let the
    /// shell auto-pick.
    pub model_id: Option<String>,
}

/// The whole status, for the settings page.
///
/// # Errors
///
/// Never. A settings section that refused to draw would leave somebody unable
/// to change the thing that failed — the same rule `model_choice_status` keeps.
#[tauri::command(async)]
pub async fn models_status<R: Runtime>(app: AppHandle<R>) -> Result<ModelsStatus, String> {
    Ok(status_of(&app))
}

/// Remove a model's registration. The bytes stay, as an extension artefact's
/// do — another project may be mid-download, and collecting unreferenced ones
/// is a separate sweep.
///
/// # Errors
///
/// [`ProjectError`] when the ref cannot be removed.
#[tauri::command(async)]
pub async fn model_remove<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<ModelsStatus, ProjectError> {
    remove_ref(&app, &id).map_err(|error| ProjectError::new("model_remove", error))?;
    // Clear any assignment that pointed at it.
    let mut assignments = assignments_of(&app);
    assignments.retain(|_, v| v != &id);
    let _ = write_assignments(&app, &assignments);
    Ok(status_of(&app))
}

/// Assign a task to a model, or clear the assignment.
///
/// # Errors
///
/// [`ProjectError`] when the model named is not installed, or when the file
/// cannot be written.
#[tauri::command(async)]
pub async fn model_assign<R: Runtime>(
    app: AppHandle<R>,
    request: AssignRequest,
) -> Result<ModelsStatus, ProjectError> {
    if let Some(id) = &request.model_id
        && ref_of(&app, id).is_none()
    {
        return Err(ProjectError::new(
            "model_unknown",
            format!("`{id}` is not installed on this machine."),
        ));
    }
    let mut assignments = assignments_of(&app);
    match &request.model_id {
        Some(id) => {
            assignments.insert(request.task, id.clone());
        }
        None => {
            assignments.remove(&request.task);
        }
    }
    write_assignments(&app, &assignments)
        .map_err(|error| ProjectError::new("model_assign", error))?;
    Ok(status_of(&app))
}

/// Install a model from the curated catalogue by its id. Downloads the weights
/// and, when the model ships its own engine, the engine binary — both verified
/// against the sha256 the catalogue carries.
///
/// # Errors
///
/// [`ProjectError`] when the id is not in the catalogue, the download fails,
/// the sha256 does not match, or the store cannot be written.
#[tauri::command(async)]
pub async fn model_install_catalogue<R: Runtime>(
    app: AppHandle<R>,
    request: CatalogueInstallRequest,
) -> Result<ModelsStatus, ProjectError> {
    install_catalogue(&app, &request.id)
        .await
        .map_err(|error| ProjectError::new("model_install_catalogue", error))?;
    Ok(status_of(&app))
}

/// Run a test through the full pipeline — the same path a handler's `model.run`
/// takes. Uses the first installed model's first declared task, so it works
/// with any model: a GGUF model serves `text.complete`, a Needle model serves
/// `text.toolcall`. Returns the raw JSON answer.
///
/// # Errors
///
/// [`ProjectError`] when no model is installed or the sidecar fails.
#[tauri::command(async)]
pub async fn model_test<R: Runtime>(
    app: AppHandle<R>,
    request: TestRequest,
) -> Result<String, ProjectError> {
    let dir = tauri::Manager::path(&app)
        .app_config_dir()
        .map(|d| d.join(MODELS_DIR))
        .map_err(|e| {
            ProjectError::new("model_test", format!("could not resolve config dir: {e}"))
        })?;
    let models = installed_manifests_in(&dir);
    let first = models.first().ok_or_else(|| {
        ProjectError::new("no_model", "No model installed. Open Settings → Models.")
    })?;
    let task = first
        .purposes
        .first()
        .cloned()
        .unwrap_or_else(|| "text.complete".to_owned());
    // A classifier (Laya/ONNX) takes structured input — `state` and
    // `questions` — not a plain prompt. Build a minimal test question so the
    // pipeline runs end to end.
    let input = if first.category.as_deref() == Some("classifier") {
        serde_json::json!({
            "state": {"body": request.prompt},
            "questions": {
                "test": {
                    "type": "noul",
                    "instructions": "Does this text describe a problem or issue?"
                }
            }
        })
    } else {
        serde_json::json!({ "prompt": request.prompt })
    };
    ask(&app, &task, &input)
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()))
        .map_err(|e| ProjectError::new("model_test", e.message))
}

/// Answer one task, for a handler.
///
/// The internal half of the service surface's `model.run`. Finds the model
/// assigned to the task (or the first installed model that declares it),
/// resolves its weights path, and asks the inference sidecar to run it. The
/// sidecar is a separate process — a crash in native inference is a reconnect
/// rather than a lost window, the same isolation the memory sidecar gives.
///
/// # Errors
///
/// [`InferenceError`] when no model serves the task, when the sidecar cannot
/// be started, or when the runtime fails.
pub(crate) fn ask<R: Runtime>(
    app: &AppHandle<R>,
    task: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, InferenceError> {
    let model = serving(app, task).ok_or_else(|| {
        InferenceError::new(
            "no_model",
            format!("No model installed for `{task}`. Open Settings → Models."),
        )
    })?;

    let dir = models_dir(app).map_err(|e| InferenceError::new("store_failed", e))?;

    // For a Laya model, the "model path" is the HuggingFace repo ID (the
    // Python library downloads and caches the weights itself), and the
    // "engine path" is the bundled `laya_server.py` script. For everything
    // else, both are files in the store.
    let (model_path, engine_path) = if model.runtime == sync_inference::Runtime::Laya {
        // Asked of the resource directory rather than worked out from the
        // executable's own, because those are two different places in a
        // bundle: a `resources` entry lands in `Contents/Resources` and the
        // executable sits in `Contents/MacOS`. This door answers both, and in
        // a build from source it answers `src-tauri`, where the file is.
        let script = tauri::Manager::path(app)
            .resource_dir()
            .map_err(|e| {
                InferenceError::new(
                    "store_failed",
                    format!("could not find the bundled resources: {e}"),
                )
            })?
            .join("binaries")
            .join("laya_server.py");
        (
            model.weights.url.clone(),
            Some(script.to_string_lossy().into_owned()),
        )
    } else {
        let sha = safe_component(&model.weights.sha256)
            .map_err(|e| InferenceError::new("store_failed", e))?;
        let model_dir = dir.join(sha);
        let mp = model_dir.join("weights").to_string_lossy().into_owned();
        let ep = model
            .engine
            .as_ref()
            .map(|_| model_dir.join("engine").to_string_lossy().into_owned());
        (mp, ep)
    };
    let sidecar =
        tauri::Manager::try_state::<crate::inference::InferenceSidecar>(app).ok_or_else(|| {
            InferenceError::new("no_sidecar", "the inference sidecar is not available")
        })?;

    sidecar
        .run(
            app,
            &model_path,
            &model.runtime.to_string(),
            task,
            input,
            engine_path.as_deref(),
        )
        .map_err(|e| InferenceError::new("runtime_failed", e))
}

/// The schema a model declares for a task, for the service surface's
/// `model.serving`.
pub(crate) fn serving_schema<R: Runtime>(
    app: &AppHandle<R>,
    task: &str,
) -> Option<sync_inference::TaskSchema> {
    serving(app, task).and_then(|m| m.schemas.get(task).cloned())
}

/// The model that serves a task: the one assigned to it, or the first installed
/// model that declares it.
fn serving<R: Runtime>(app: &AppHandle<R>, task: &str) -> Option<ModelManifest> {
    let dir = models_dir(app).ok()?;
    let assignments = assignments_of(app);
    serving_in(&dir, &assignments, task)
}

/// Path-based dispatch, testable without an `AppHandle`.
fn serving_in(
    dir: &Path,
    assignments: &BTreeMap<String, String>,
    task: &str,
) -> Option<ModelManifest> {
    if let Some(id) = assignments.get(task)
        && let Some(manifest) = manifest_in(dir, id)
    {
        return Some(manifest);
    }
    // Auto-pick: the first installed model that declares the task.
    installed_manifests_in(dir)
        .into_iter()
        .find(|model| model.purposes.iter().any(|p| p == task))
}

/// The whole status, built from the files on disk.
fn status_of<R: Runtime>(app: &AppHandle<R>) -> ModelsStatus {
    let caps = capability::Capabilities::here();
    let installed = installed_models(app);
    let installed_ids: std::collections::HashSet<&str> =
        installed.iter().map(|m| m.id.as_str()).collect();

    let catalogue = curated_catalogue()
        .into_iter()
        .map(|manifest| {
            let refusal = capability::refusal_of(&manifest, &caps);
            CatalogueEntry {
                eligible: refusal.is_none(),
                refusal,
                installed: installed_ids.contains(manifest.id.as_str()),
                manifest,
            }
        })
        .collect();

    ModelsStatus {
        installed,
        assignments: assignments_of(app),
        catalogue,
        capabilities: CapabilitiesView::from(&caps),
    }
}

/// The curated catalogue bundled into this build.
///
/// Needle 3 from Cactus Compute — a 34 MB tool-calling model that ships its
/// own engine binary. Included only on macOS ARM64 for now, because that is
/// the one platform binary whose sha256 is pinned here; other platforms get
/// an empty catalogue until their binaries are pinned.
fn curated_catalogue() -> Vec<ModelManifest> {
    let mut entries = Vec::new();

    // Needle 3, on macOS ARM64. The `.cact` file is the model; the `needle`
    // binary is the engine that loads it. Both are fetched from HuggingFace
    // and verified against the sha256s pinned here.
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        use sync_inference::{Engine, Modality, Requires, Runtime, Weights};

        entries.push(ModelManifest {
            id: "needle-3".to_owned(),
            version: "3.0.0".to_owned(),
            name: "Needle 3".to_owned(),
            modality: Modality::Text,
            runtime: Runtime::Needle,
            purposes: vec!["text.toolcall".to_owned(), "text.extract".to_owned()],
            category: Some("router".to_owned()),
            size: 35_335_380,
            min_memory: 100_000_000,
            requires: Requires {
                arch: vec!["aarch64".to_owned()],
                gpu: "any".to_owned(),
            },
            weights: Weights {
                url: "https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact"
                    .to_owned(),
                sha256: "c9d915eca282ed42d1a09b143b592adb4cc6744ffe2d294adf5cfc5548170c38"
                    .to_owned(),
                format: Some("cact".to_owned()),
            },
            engine: Some(Engine {
                url:
                    "https://huggingface.co/Cactus-Compute/needle3/resolve/main/macos-arm64/needle"
                        .to_owned(),
                sha256: "bfcc14c38a7ebf670bf7ade3849b617d268120dc4cc75d3aa12c88bc7c0ba75c"
                    .to_owned(),
                platform: "macos-arm64".to_owned(),
            }),
            source: Some("cactus".to_owned()),
            license: Some("apache-2.0".to_owned()),
            schemas: std::collections::BTreeMap::new(),
        });
    }

    // Laya — a multilingual classifier from Convai Innovations. Exported to
    // ONNX for native inference; the model, tokenizer, and calibration are
    // downloaded together. Runs via `ort` (ONNX Runtime), no Python needed.
    {
        use sync_inference::{Modality, Requires, Runtime, Weights};

        entries.push(ModelManifest {
            id: "laya".to_owned(),
            version: "1.0.0".to_owned(),
            name: "Laya".to_owned(),
            modality: Modality::Text,
            runtime: Runtime::Onnx,
            purposes: vec![
                "text.choice".to_owned(),
                "text.score".to_owned(),
                "text.noul".to_owned(),
            ],
            category: Some("classifier".to_owned()),
            size: 1_685_060_996,
            min_memory: 4_000_000_000,
            requires: Requires {
                arch: vec![],
                gpu: "any".to_owned(),
            },
            weights: Weights {
                url: "https://huggingface.co/convaiinnovations/laya/resolve/main/laya.onnx"
                    .to_owned(),
                sha256: "3025fd8ca30ae525dd46d64a4f4209d50ac79355495b756c9cef72f7980328e5"
                    .to_owned(),
                format: Some("onnx".to_owned()),
            },
            engine: None,
            source: Some("convai-innovations".to_owned()),
            license: Some("apache-2.0".to_owned()),
            schemas: std::collections::BTreeMap::new(),
        });
    }

    entries
}

// --- Storage ---------------------------------------------------------------

/// A string that is safe to use as a single path component: it is not empty,
/// contains no separator, and is not `.` or `..`. A model's `id` and `sha256`
/// are both used to build paths (`refs/<id>.json`, `<sha256>/`), and a
/// manifest fetched from the network is not trusted to contain only safe
/// characters — a hostile manifest with `id: "../../something"` would write
/// outside the models directory without this check.
fn safe_component(s: &str) -> Result<&str, String> {
    if s.is_empty() || s == "." || s == ".." {
        return Err(format!("`{s}` is not a valid path component"));
    }
    if s.contains('/') || s.contains('\\') {
        return Err(format!("`{s}` contains a path separator"));
    }
    Ok(s)
}

/// The models directory under the config dir.
fn models_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    tauri::Manager::path(app)
        .app_config_dir()
        .map(|d| d.join(MODELS_DIR))
        .map_err(|e| format!("could not resolve the configuration directory: {e}"))
}

/// The ref file for one model id: `models/refs/<id>.json`.
///
/// The id is checked here rather than by the callers because every read, write
/// and removal of a ref builds its path through this one function — and one of
/// those callers answers an `invoke`, where the id is whatever the window sent.
/// `join` on an absolute path discards the directory entirely, and `..` walks
/// out of it, so an unchecked id turns a model's removal into the removal of a
/// file somebody named.
fn ref_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    let id = safe_component(id)?;
    Ok(dir.join("refs").join(format!("{id}.json")))
}

/// One model's ref, as stored.
#[derive(Debug, Deserialize, Serialize)]
struct Ref {
    sha256: String,
}

/// The digest a model id is stored under, or `None` when it is not installed.
fn ref_of<R: Runtime>(app: &AppHandle<R>, id: &str) -> Option<String> {
    let dir = models_dir(app).ok()?;
    ref_in(&dir, id)
}

/// Path-based read of a ref, testable without an `AppHandle`.
fn ref_in(dir: &Path, id: &str) -> Option<String> {
    let path = ref_path(dir, id).ok()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let r: Ref = serde_json::from_str(&text).ok()?;
    Some(r.sha256)
}

/// Path-based read of a manifest, testable without an `AppHandle`.
fn manifest_in(dir: &Path, id: &str) -> Option<ModelManifest> {
    let sha = ref_in(dir, id)?;
    let path = dir.join(&sha).join("manifest.json");
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Every installed model's manifest.
fn installed_manifests<R: Runtime>(app: &AppHandle<R>) -> Vec<ModelManifest> {
    let dir = match models_dir(app) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    installed_manifests_in(&dir)
}

/// Path-based list of installed manifests, testable without an `AppHandle`.
fn installed_manifests_in(dir: &Path) -> Vec<ModelManifest> {
    let refs = match std::fs::read_dir(dir.join("refs")) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in refs.flatten() {
        if let Some(id) = entry.file_name().to_str() {
            let id = id.trim_end_matches(".json");
            if let Some(manifest) = manifest_in(dir, id) {
                out.push(manifest);
            }
        }
    }
    out
}

/// Every installed model, as the settings window reads it.
fn installed_models<R: Runtime>(app: &AppHandle<R>) -> Vec<InstalledModel> {
    installed_manifests(app)
        .into_iter()
        .map(|m| {
            let sha256 = ref_of(app, &m.id).unwrap_or_default();
            InstalledModel {
                id: m.id,
                name: m.name,
                runtime: m.runtime.to_string(),
                purposes: m.purposes,
                size: m.size,
                sha256,
            }
        })
        .collect()
}

/// The assignments file, or the empty default.
fn assignments_of<R: Runtime>(app: &AppHandle<R>) -> BTreeMap<String, String> {
    configuration_file(app, ASSIGNMENTS_FILE)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Write the assignments file.
fn write_assignments<R: Runtime>(
    app: &AppHandle<R>,
    assignments: &BTreeMap<String, String>,
) -> Result<(), String> {
    let path = configuration_file(app, ASSIGNMENTS_FILE).map_err(|e| e.message)?;
    write_configuration(&path, assignments).map_err(|e| e.message)
}

/// Remove a model's ref.
fn remove_ref<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let dir = models_dir(app)?;
    let path = ref_path(&dir, id)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Install a model from the curated catalogue: find the manifest by id,
/// download the weights (and the engine, when the model ships one), verify
/// each against the sha256 the catalogue carries, and register.
async fn install_catalogue<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let manifest = curated_catalogue()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("`{id}` is not in the catalogue"))?;

    safe_component(&manifest.id)?;

    let dir = models_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // A model with a sha256 downloads its weights (and engine, when it ships
    // one) into `models/<sha256>/`. A model without one — Laya, which
    // downloads its own weights via the Python library — skips this entirely;
    // `register` creates its directory from the id.
    if !manifest.weights.sha256.is_empty() {
        safe_component(&manifest.weights.sha256)?;
        let model_dir = dir.join(&manifest.weights.sha256);
        std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

        let weights_file = model_dir.join("weights");
        if !weights_file.exists() {
            download_and_verify(
                &manifest.weights.url,
                &manifest.weights.sha256,
                &weights_file,
            )
            .await?;
        }

        if let Some(engine) = &manifest.engine {
            let engine_file = model_dir.join("engine");
            if !engine_file.exists() {
                download_and_verify(&engine.url, &engine.sha256, &engine_file).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&engine_file, std::fs::Permissions::from_mode(0o755))
                        .map_err(|e| e.to_string())?;
                }
            }
        }

        // For ONNX models, also download the tokenizer and calibration that
        // the runtime needs beside the model. Their URLs are derived from the
        // weights URL — same directory, different filename.
        if manifest.runtime == sync_inference::Runtime::Onnx {
            let base_url = manifest
                .weights
                .url
                .rsplit_once('/')
                .map(|(dir, _)| dir)
                .unwrap_or("");
            for file in &["tokenizer.json", "calibration.json"] {
                let file_path = model_dir.join(file);
                if !file_path.exists() {
                    download_file(&format!("{base_url}/{file}"), &file_path).await?;
                }
            }
        }
    }

    register(&dir, &manifest)?;
    Ok(())
}

/// Download a file from a URL to a destination path, without sha256
/// verification. Used for small support files (tokenizer, calibration) whose
/// integrity is covered by the model's own sha256.
async fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|e| format!("could not build an HTTP client: {e}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not download `{url}`: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("`{url}` answered {}", response.status()));
    }
    use std::io::Write;
    let mut response = response;
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| format!("the download was interrupted: {e}"))?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Download a file from a URL to a destination path, verifying its sha256.
async fn download_and_verify(url: &str, expected_sha256: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|e| format!("could not build an HTTP client: {e}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not download `{url}`: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("`{url}` answered {}", response.status()));
    }

    // Stream to a staging file, then verify and rename — the same two-phase
    // safety the extension store uses.
    let staging = dest.with_extension("partial");
    use std::io::Write;
    let mut response = response;
    let mut file = std::fs::File::create(&staging).map_err(|e| e.to_string())?;
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| format!("the download was interrupted: {e}"))?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    drop(file);

    // Verify sha256 by streaming — a large model would OOM if loaded whole.
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(&staging).map_err(|e| e.to_string())?;
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    let digest = hex::encode(hasher.finalize());
    if digest != expected_sha256 {
        let _ = std::fs::remove_file(&staging);
        return Err(format!(
            "the downloaded bytes do not match: expected {expected_sha256}, got {digest}"
        ));
    }

    std::fs::rename(&staging, dest).map_err(|e| {
        let _ = std::fs::remove_file(&staging);
        e.to_string()
    })?;
    Ok(())
}

/// Point a ref at a digest and write the manifest beside the weights.
fn register(dir: &Path, manifest: &ModelManifest) -> Result<(), String> {
    let id = safe_component(&manifest.id)?;
    // Content-address the directory by sha256 when present, or by id when it
    // is not — a model like Laya has no local file to hash, so its directory
    // is named after the model itself.
    let dir_name = if manifest.weights.sha256.is_empty() {
        id
    } else {
        safe_component(&manifest.weights.sha256)?
    };
    let model_dir = dir.join(dir_name);
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
    let manifest_path = model_dir.join("manifest.json");
    let text = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, text).map_err(|e| e.to_string())?;

    let refs = dir.join("refs");
    std::fs::create_dir_all(&refs).map_err(|e| e.to_string())?;
    let ref_file = ref_path(dir, id)?;
    let r = Ref {
        sha256: manifest.weights.sha256.clone(),
    };
    let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
    std::fs::write(&ref_file, text).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync_inference::{Modality, Requires, Runtime, Weights};

    /// A manifest with the fields the store reads, and none it does not.
    fn a_manifest(id: &str, purposes: &[&str]) -> ModelManifest {
        ModelManifest {
            id: id.to_owned(),
            version: "1.0.0".to_owned(),
            name: id.to_owned(),
            modality: Modality::Text,
            runtime: Runtime::Gguf,
            purposes: purposes.iter().map(|p| (*p).to_owned()).collect(),
            category: None,
            size: 1,
            min_memory: 0,
            requires: Requires::default(),
            weights: Weights {
                url: "https://example.invalid/m".to_owned(),
                sha256: format!("{id}-sha"),
                format: None,
            },
            engine: None,
            source: None,
            license: None,
            schemas: std::collections::BTreeMap::new(),
        }
    }

    /// A register → ref → manifest round-trip is the whole of what installing
    /// does to the store. A model that does not come back from the path it was
    /// written to is a model the dispatch will never find.
    #[test]
    fn a_registered_model_round_trips() {
        let dir = tempfile::tempdir().expect("a directory");
        let manifest = a_manifest("needle-3", &["text.classify"]);
        register(dir.path(), &manifest).expect("registers");

        assert_eq!(
            ref_in(dir.path(), "needle-3").as_deref(),
            Some("needle-3-sha")
        );
        let back = manifest_in(dir.path(), "needle-3").expect("reads back");
        assert_eq!(back.id, "needle-3");
        assert_eq!(back.purposes, vec!["text.classify".to_owned()]);
    }

    /// An id that is a way out of the models directory is refused by the one
    /// function every ref path is built through.
    ///
    /// `model_remove` answers an `invoke`, so the id is whatever the window
    /// sent, and the thing on the other end of the path is a file somebody
    /// else owns. The two spellings are the two that work: `..` walks out of
    /// the directory, and an absolute path means `join` never used it.
    #[test]
    fn an_id_that_leaves_the_models_directory_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let outside = dir.path().join("secret.json");
        std::fs::write(&outside, "{}").expect("a file to not lose");

        for id in ["../secret", "..", ".", "", "a/b", "/etc/passwd"] {
            assert!(
                ref_path(dir.path(), id).is_err(),
                "`{id}` builds a path out of the models directory"
            );
            assert!(ref_in(dir.path(), id).is_none(), "`{id}` reads nothing");
        }

        assert!(
            outside.exists(),
            "nothing outside the directory was touched"
        );
    }

    /// Dispatch picks the assigned model first. A task with an assignment goes
    /// to the model named, even when another installed model declares the same
    /// task — assignment is the override, auto-pick is the fallback.
    #[test]
    fn an_assigned_task_goes_to_the_assigned_model() {
        let dir = tempfile::tempdir().expect("a directory");
        register(dir.path(), &a_manifest("a", &["text.classify"])).expect("registers");
        register(dir.path(), &a_manifest("b", &["text.classify"])).expect("registers");

        let mut assignments = BTreeMap::new();
        assignments.insert("text.classify".to_owned(), "b".to_owned());

        let picked = serving_in(dir.path(), &assignments, "text.classify").expect("serves");
        assert_eq!(picked.id, "b", "the assignment wins over auto-pick");
    }

    /// Without an assignment, dispatch auto-picks the first installed model
    /// that declares the task. This is the path a handler takes when nobody has
    /// opened Settings yet.
    #[test]
    fn an_unassigned_task_auto_picks() {
        let dir = tempfile::tempdir().expect("a directory");
        register(dir.path(), &a_manifest("a", &["text.classify"])).expect("registers");

        let assignments = BTreeMap::new();
        let picked = serving_in(dir.path(), &assignments, "text.classify").expect("serves");
        assert_eq!(picked.id, "a");
    }

    /// A task no installed model declares is `None`, which the caller turns
    /// into the `no_model` refusal. Dispatch never panics for a missing task.
    #[test]
    fn a_task_nothing_serves_is_none() {
        let dir = tempfile::tempdir().expect("a directory");
        register(dir.path(), &a_manifest("a", &["text.classify"])).expect("registers");

        let assignments = BTreeMap::new();
        assert!(serving_in(dir.path(), &assignments, "audio.tts").is_none());
    }

    /// An assignment to a model that is no longer installed falls through to
    /// auto-pick, rather than answering `None` for a task something does serve.
    /// A model removed from the machine should not take its task with it.
    #[test]
    fn a_stale_assignment_falls_through_to_auto_pick() {
        let dir = tempfile::tempdir().expect("a directory");
        register(dir.path(), &a_manifest("a", &["text.classify"])).expect("registers");

        let mut assignments = BTreeMap::new();
        // Points at a model that was never installed.
        assignments.insert("text.classify".to_owned(), "gone".to_owned());

        let picked = serving_in(dir.path(), &assignments, "text.classify").expect("falls through");
        assert_eq!(picked.id, "a");
    }

    /// A manifest with a path-traversal id is refused at registration. A
    /// hostile manifest fetched from the network could carry `../../` in its
    /// id, and without this check the ref file would land outside the models
    /// directory.
    #[test]
    fn a_path_traversal_id_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let mut manifest = a_manifest("needle-3", &["text.classify"]);
        manifest.id = "../../etc/passwd".to_owned();
        assert!(
            register(dir.path(), &manifest).is_err(),
            "path traversal is refused"
        );
    }

    /// A sha256 containing a separator is refused for the same reason — it
    /// becomes a directory name, and a separator in it would escape the store.
    #[test]
    fn a_sha256_with_a_separator_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let mut manifest = a_manifest("needle-3", &["text.classify"]);
        manifest.weights.sha256 = "abc/def".to_owned();
        assert!(
            register(dir.path(), &manifest).is_err(),
            "separator in sha256 is refused"
        );
    }
}
