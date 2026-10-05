//! The GGUF runtime: loads a model file via `llama-cpp-2` and runs text
//! completion.
//!
//! The same `llama-cpp-2` the memory sidecar links for embeddings, used here
//! for generation. The model is loaded once and held; each [`GgufModel::run`]
//! creates a fresh context, tokenises the prompt, decodes, and generates
//! greedily until the model emits an end-of-generation token or the token
//! budget is spent.
//!
//! The input is pass-through: a `prompt` field in the JSON input becomes the
//! text the model continues. The output is `{"text": "…"}`. How a handler
//! turns its task into a prompt, and the model's text into an answer, is the
//! handler's business — the server runs completion and hands back the string.

use std::num::NonZeroU32;
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use sync_inference::InferenceError;

/// How many tokens to generate before stopping. Generous for a classification
/// prompt (which is a few words) and tight enough to bound the time a handler
/// waits. A model that needs more is a model that should declare a different
/// task, not one that should run longer.
const MAX_TOKENS: usize = 256;

/// The context window. Small, because an auxiliary model answers short
/// prompts: a few sentences of state, a question, and a one-word answer.
const N_CTX: u32 = 2048;

/// The global llama.cpp backend, initialised once and held for the process.
static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();

/// Initialise (or return) the global backend. Cheap after the first call.
fn backend() -> Result<&'static LlamaBackend, String> {
    let entry = BACKEND.get_or_init(|| {
        LlamaBackend::init()
            .map(|mut backend| {
                backend.void_logs();
                backend
            })
            .map_err(|e| format!("{e}"))
    });
    entry.as_ref().map_err(std::clone::Clone::clone)
}

/// One loaded GGUF model, ready to run.
pub struct GgufModel {
    /// The path the model was loaded from, so the server can tell whether a
    /// `run` request names the same model or a different one.
    pub path: String,
    model: LlamaModel,
}

impl GgufModel {
    /// Load a GGUF file. GPU layers are offloaded where the build has a path
    /// to one — on Apple Silicon that is Metal, and `with_n_gpu_layers(999)`
    /// moves everything; on a CPU-only build it is a no-op.
    ///
    /// # Errors
    ///
    /// When the backend cannot be initialised or the file cannot be loaded.
    pub fn load(path: &str) -> Result<Self, String> {
        let b = backend()?;
        let params = LlamaModelParams::default().with_n_gpu_layers(999);
        let model = LlamaModel::load_from_file(b, path, &params)
            .map_err(|e| format!("load `{path}`: {e}"))?;
        Ok(Self {
            path: path.to_owned(),
            model,
        })
    }

    /// Run one task: extract the prompt from `input`, generate, return the
    /// text. The input is `{"prompt": "…"}`; the output is `{"text": "…"}`.
    ///
    /// # Errors
    ///
    /// [`InferenceError`] when the input has no prompt, the model fails to
    /// tokenise or decode, or the token budget is exhausted mid-generation.
    pub fn run(
        &self,
        _task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let prompt = input
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| InferenceError::new("bad_input", "the input has no `prompt` field"))?;

        let text = self.complete(prompt)?;
        Ok(serde_json::json!({ "text": text }))
    }

    /// Tokenise the prompt, decode it, and generate greedily until EOG or the
    /// token budget.
    fn complete(&self, prompt: &str) -> Result<String, InferenceError> {
        let b = backend().map_err(|e| InferenceError::new("backend_failed", e))?;

        let tokens = self
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| InferenceError::new("tokenize_failed", e.to_string()))?;

        let n_ctx = NonZeroU32::new(N_CTX).unwrap_or(NonZeroU32::MIN);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx))
            .with_n_seq_max(1)
            .with_embeddings(false);
        let mut ctx = self
            .model
            .new_context(b, ctx_params)
            .map_err(|e| InferenceError::new("context_failed", e.to_string()))?;

        // First batch: the whole prompt. Logits are needed only on the last
        // token, so `add_sequence` with `logits_all = false` is right.
        let mut batch = LlamaBatch::new(tokens.len().max(1), 1);
        batch
            .add_sequence(&tokens, 0, false)
            .map_err(|e| InferenceError::new("batch_failed", e.to_string()))?;
        ctx.decode(&mut batch)
            .map_err(|e| InferenceError::new("decode_failed", e.to_string()))?;

        let mut sampler = LlamaSampler::greedy();
        let mut result = String::new();
        let start = i32::try_from(tokens.len()).unwrap_or(0);

        for n_past in (start..).take(MAX_TOKENS) {
            let logits_idx = batch.n_tokens() - 1;
            let token = sampler.sample(&ctx, logits_idx);

            if self.model.is_eog_token(token) {
                break;
            }

            let piece = token_to_string(&self.model, token)?;
            result.push_str(&piece);

            // Next batch: the single sampled token, at the next position.
            batch.clear();
            batch
                .add(token, n_past, &[0], true)
                .map_err(|e| InferenceError::new("batch_failed", e.to_string()))?;
            ctx.decode(&mut batch)
                .map_err(|e| InferenceError::new("decode_failed", e.to_string()))?;
        }

        Ok(result)
    }
}

/// Convert one token to a `String`, retrying with a larger buffer when the
/// token's piece does not fit in the initial 8 bytes — the pattern
/// `token_to_piece` uses internally, spelled out here so the crate does not
/// depend on `encoding_rs` for what is a buffer-size retry.
fn token_to_string(
    model: &LlamaModel,
    token: llama_cpp_2::token::LlamaToken,
) -> Result<String, InferenceError> {
    use llama_cpp_2::TokenToStringError;
    let bytes = match model.token_to_piece_bytes(token, 8, true, None) {
        Err(TokenToStringError::InsufficientBufferSpace(size)) => model
            .token_to_piece_bytes(token, (-size).try_into().unwrap_or(64), true, None)
            .map_err(|e| InferenceError::new("detokenize_failed", e.to_string()))?,
        other => other.map_err(|e| InferenceError::new("detokenize_failed", e.to_string()))?,
    };
    String::from_utf8(bytes)
        .map_err(|e| InferenceError::new("detokenize_failed", format!("not valid UTF-8: {e}")))
}
