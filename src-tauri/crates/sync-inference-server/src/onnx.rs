//! The ONNX runtime: loads an ONNX model + tokenizer and runs inference,
//! natively, no Python.
//!
//! For Laya — a `DecisionModel` exported to ONNX. The model takes
//! `input_ids`, `attention_mask`, `marker_pos`, `marker_mask`, `qtype` and
//! returns `logits` at the mask-token positions. This module builds the input
//! sequence (the same `[CLS] type instructions [SEP] <mask> opt0 ... [SEP]
//! state [SEP]` layout the Python `build_sequence` constructs), runs the ONNX
//! session, and post-processes the logits into `choice`/`score`/`noul` answers
//! with calibrated confidence.
//!
//! The tokenizer and calibration live beside the ONNX model in the store:
//! `models/<sha>/weights` (the ONNX file), `models/<sha>/tokenizer.json`,
//! `models/<sha>/calibration.json`.

#![allow(clippy::cast_precision_loss)]

use ndarray::{Array1, Array2};
use ort::session::{Session, builder::GraphOptimizationLevel};
use ort::value::Tensor;
use serde::Deserialize;
use sync_inference::InferenceError;
use tokenizers::Tokenizer;

/// Calibration data: per-type temperatures, token IDs, context limits.
/// Loaded from `calibration.json` beside the model.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Calibration {
    temperature: Vec<f32>,
    max_len: usize,
    head_max_len: usize,
    cls_token_id: u32,
    sep_token_id: u32,
    mask_token_id: u32,
}

/// One loaded ONNX model: the session, the tokenizer, and the calibration.
pub struct OnnxModel {
    /// The path to the ONNX file, so the server can tell whether a `run`
    /// request names the same model or a different one.
    pub path: String,
    session: Session,
    tokenizer: Tokenizer,
    cal: Calibration,
}

/// One question, parsed from the handler's input.
struct Question {
    qtype: u32,
    instructions: String,
    options: Vec<String>,
    option_keys: Vec<String>,
}

impl OnnxModel {
    /// Load the ONNX model, tokenizer, and calibration from the store
    /// directory. `model_path` is the path to the ONNX file; the tokenizer
    /// and calibration are looked up beside it.
    ///
    /// # Errors
    ///
    /// When any file cannot be read or the ONNX session cannot be created.
    pub fn load(model_path: &str) -> Result<Self, String> {
        let dir = std::path::Path::new(model_path)
            .parent()
            .ok_or("the model path has no parent directory")?;

        let session = Session::builder()
            .map_err(|e| format!("could not create a session builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level1)
            .map_err(|e| format!("could not set optimization level: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("could not set threads: {e}"))?
            .commit_from_file(model_path)
            .map_err(|e| format!("could not load the ONNX model: {e}"))?;

        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| format!("could not load the tokenizer: {e}"))?;

        let cal_text = std::fs::read_to_string(dir.join("calibration.json"))
            .map_err(|e| format!("could not read calibration.json: {e}"))?;
        let cal: Calibration = serde_json::from_str(&cal_text)
            .map_err(|e| format!("could not parse calibration.json: {e}"))?;

        Ok(Self {
            path: model_path.to_owned(),
            session,
            tokenizer,
            cal,
        })
    }

    /// Run one or more questions over a state. The input is
    /// `{"state": {...}, "questions": {...}}` — the same shape Laya's Python
    /// API takes.
    ///
    /// # Errors
    ///
    /// [`InferenceError`] when the input is malformed or inference fails.
    pub fn run(
        &mut self,
        _task: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let state = input
            .get("state")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let questions = input
            .get("questions")
            .and_then(|q| q.as_object())
            .ok_or_else(|| {
                InferenceError::new("bad_input", "the input has no `questions` field")
            })?;

        let mut answers = serde_json::Map::new();

        for (name, q) in questions {
            let question = parse_question(q)?;
            let result = self.run_one(&state, &question)?;
            answers.insert(name.clone(), result);
        }

        Ok(serde_json::json!({ "answers": answers }))
    }

    /// Run one question: build the sequence, run the model, post-process.
    #[allow(clippy::too_many_lines)]
    fn run_one(
        &mut self,
        state: &serde_json::Value,
        q: &Question,
    ) -> Result<serde_json::Value, InferenceError> {
        let (ids, markers) = build_sequence(
            &self.tokenizer,
            state,
            q,
            self.cal.max_len,
            self.cal.head_max_len,
            self.cal.cls_token_id,
            self.cal.sep_token_id,
            self.cal.mask_token_id,
        );

        if markers.is_empty() {
            return Err(InferenceError::new(
                "bad_input",
                "no options fit in the context",
            ));
        }

        let seq_len = ids.len();
        let batch = 1usize;
        let n_markers = markers.len();

        // Pad to a consistent marker count for the ONNX model.
        let max_markers = 20usize;
        let mut marker_pos = vec![0i64; max_markers];
        let mut marker_mask = vec![false; max_markers];
        for (i, &pos) in markers.iter().enumerate().take(max_markers) {
            marker_pos[i] = i64::try_from(pos).unwrap_or(0);
            marker_mask[i] = true;
        }

        let input_ids: Vec<i64> = ids.iter().map(|&id| i64::from(id)).collect();
        let attention_mask = vec![1i64; seq_len];

        let input_ids = Array2::from_shape_vec((batch, seq_len), input_ids)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let attention_mask = Array2::from_shape_vec((batch, seq_len), attention_mask)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let marker_pos = Array2::from_shape_vec((batch, max_markers), marker_pos)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let marker_mask_arr = Array2::from_shape_vec((batch, max_markers), marker_mask)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let qtype = Array1::from_vec(vec![i64::from(q.qtype)]);

        let input_ids_t = Tensor::from_array(input_ids)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let attention_mask_t = Tensor::from_array(attention_mask)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let marker_pos_t = Tensor::from_array(marker_pos)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let marker_mask_t = Tensor::from_array(marker_mask_arr)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;
        let qtype_t = Tensor::from_array(qtype)
            .map_err(|e| InferenceError::new("tensor_failed", e.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_t,
                "attention_mask" => attention_mask_t,
                "marker_pos" => marker_pos_t,
                "marker_mask" => marker_mask_t,
                "qtype" => qtype_t
            ])
            .map_err(|e| InferenceError::new("inference_failed", e.to_string()))?;

        let (_shape, logits_data) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|e| InferenceError::new("inference_failed", e.to_string()))?;
        let raw_logits: Vec<f32> = logits_data.iter().take(n_markers).copied().collect();

        // Apply temperature and softmax.
        let temp = self
            .cal
            .temperature
            .get(q.qtype as usize)
            .copied()
            .unwrap_or(1.0)
            .max(0.1);
        let scaled: Vec<f32> = raw_logits.iter().map(|&l| l / temp).collect();
        let probs = softmax(&scaled);
        let confidence = probs.iter().fold(0.0f32, |a, &p| a.max(p));

        // Produce the answer for this question type.
        let answer = match q.qtype {
            0 => {
                // choice: pick the option with highest probability.
                let best = probs
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map_or(0, |(i, _)| i);
                serde_json::json!({
                    "choice": q.option_keys.get(best).cloned().unwrap_or_default(),
                    "confidence": confidence,
                    "probabilities": probs,
                })
            }
            1 => {
                // score: expected value over levels.
                let score: f32 = probs.iter().enumerate().map(|(i, &p)| p * i as f32).sum();
                serde_json::json!({
                    "score": score,
                    "confidence": confidence,
                    "probabilities": probs,
                })
            }
            _ => {
                // noul: probability of "true" (last option).
                let noul = probs.last().copied().unwrap_or(0.0);
                serde_json::json!({
                    "noul": noul,
                    "confidence": confidence.max(1.0 - noul),
                    "probabilities": probs,
                })
            }
        };

        Ok(answer)
    }
}

/// Parse a question from the handler's JSON input.
fn parse_question(q: &serde_json::Value) -> Result<Question, InferenceError> {
    let qtype_str = q
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| InferenceError::new("bad_input", "question has no type"))?;

    let qtype = match qtype_str {
        "choice" => 0,
        "score" => 1,
        "noul" => 2,
        other => {
            return Err(InferenceError::new(
                "bad_input",
                format!("unknown question type `{other}`"),
            ));
        }
    };

    let instructions = q
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();

    let (options, option_keys) = render_options(qtype_str, q);

    Ok(Question {
        qtype,
        instructions,
        options,
        option_keys,
    })
}

/// Render option texts for a question, porting Laya's `render_options`.
fn render_options(qtype: &str, q: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    match qtype {
        "choice" => {
            let crit = q
                .get("criteria")
                .or_else(|| q.get("crit"))
                .cloned()
                .unwrap_or_default();
            if let Some(obj) = crit.as_object() {
                let opts: Vec<(String, String)> = obj
                    .iter()
                    .map(|(k, v)| {
                        let desc = v.as_str().unwrap_or("");
                        if desc.is_empty() {
                            (k.clone(), k.clone())
                        } else {
                            (k.clone(), format!("{k}: {desc}"))
                        }
                    })
                    .collect();
                (
                    opts.iter().map(|(_, t)| t.clone()).collect(),
                    opts.iter().map(|(k, _)| k.clone()).collect(),
                )
            } else {
                (vec![], vec![])
            }
        }
        "score" => {
            let crit = q
                .get("criteria")
                .or_else(|| q.get("crit"))
                .cloned()
                .unwrap_or_default();
            if let Some(arr) = crit.as_array() {
                (
                    arr.iter()
                        .enumerate()
                        .map(|(i, c)| format!("level {i}: {}", c.as_str().unwrap_or("")))
                        .collect(),
                    (0..arr.len()).map(|i| i.to_string()).collect(),
                )
            } else {
                (vec![], vec![])
            }
        }
        _ => {
            // noul: [false, true]
            (
                vec![
                    "false: no, the statement does not hold".to_owned(),
                    "true: yes, the statement holds".to_owned(),
                ],
                vec!["false".to_owned(), "true".to_owned()],
            )
        }
    }
}

/// Build the input sequence: `[CLS] type question: instructions [SEP] <mask>
/// opt0 <mask> opt1 ... [SEP] state [SEP]`. Returns token IDs and the
/// positions of the mask tokens (markers).
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn build_sequence(
    tokenizer: &Tokenizer,
    state: &serde_json::Value,
    q: &Question,
    max_len: usize,
    head_max_len: usize,
    cls_id: u32,
    sep_id: u32,
    mask_id: u32,
) -> (Vec<u32>, Vec<usize>) {
    let qtype_name = match q.qtype {
        0 => "choice",
        1 => "score",
        _ => "noul",
    };

    // Head: "type question: instructions"
    let head_text = format!("{qtype_name} question: {}", q.instructions);
    let head_ids = tokenize(tokenizer, &head_text);

    // Options: [mask_token_id] + tokenized option text (truncated to 48 tokens)
    let mut opt_ids: Vec<Vec<u32>> = Vec::new();
    for opt in &q.options {
        let mut ids = vec![mask_id];
        let opt_text = format!(" {opt}");
        let mut toks = tokenize(tokenizer, &opt_text);
        toks.truncate(48);
        ids.extend(toks);
        opt_ids.push(ids);
    }

    // Budget for the head
    let opt_total: usize = opt_ids.iter().map(std::vec::Vec::len).sum();
    let mut opt_budget = head_max_len.saturating_sub(opt_total);
    if opt_budget < 16 && !opt_ids.is_empty() {
        let per = ((head_max_len - 16) / opt_ids.len()).max(4);
        for o in &mut opt_ids {
            o.truncate(per);
        }
        opt_budget =
            head_max_len.saturating_sub(opt_ids.iter().map(std::vec::Vec::len).sum::<usize>());
    }
    let head_limit = opt_budget.max(8);

    // Assemble: [CLS] head[:head_limit] [SEP] <mask> opt0 <mask> opt1 ... [SEP] state [SEP]
    let mut ids = vec![cls_id];
    ids.extend(head_ids.iter().take(head_limit));
    ids.push(sep_id);

    let mut markers = Vec::new();
    for o in &opt_ids {
        markers.push(ids.len());
        ids.extend(o.iter());
    }
    ids.push(sep_id);

    // State
    let state_text = serde_json::to_string(state).unwrap_or_default();
    let state_ids = tokenize(tokenizer, &state_text);
    let room = max_len.saturating_sub(ids.len() + 1);
    ids.extend(state_ids.iter().take(room));
    ids.push(sep_id);

    // Truncate to max_len and filter markers
    ids.truncate(max_len);
    markers.retain(|&m| m < max_len);

    (ids, markers)
}

/// Tokenize text without special tokens.
fn tokenize(tokenizer: &Tokenizer, text: &str) -> Vec<u32> {
    match tokenizer.encode(text, false) {
        Ok(enc) => enc.get_ids().to_vec(),
        Err(_) => Vec::new(),
    }
}

/// Softmax over a slice.
fn softmax(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return Vec::new();
    }
    let max = logits.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let exp: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    if sum > 0.0 {
        exp.iter().map(|&e| e / sum).collect()
    } else {
        vec![1.0 / logits.len() as f32; logits.len()]
    }
}
