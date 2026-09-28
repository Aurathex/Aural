//! Small local language models (Qwen2-style decoders exported to ONNX) for AI cleanup,
//! run by the same ONNX Runtime as the speech models. Expected folder: `model.onnx` (or
//! `model_int8.onnx`), `config.json`, `tokenizer.json`.
//!
//! Greedy decoding with a key/value cache; generation stops at the end-of-turn token or
//! the token limit. The prompt is the model's own chat format with a system message.

use crate::TextModel;
use anyhow::{anyhow, bail, Context, Result};
use ndarray::{Array2, ArrayD, Axis, IxDyn, Slice};
use ort::session::Session;
use std::borrow::Cow;
use std::path::Path;

pub struct Llm {
    session: Session,
    tokenizer: tokenizers::Tokenizer,
    layers: usize,
    kv_heads: usize,
    head_dim: usize,
    stop: Vec<u32>,
    label: String,
    model_type: Option<String>,
    inputs: Vec<String>,
    /// The last prompt and its key/value cache: the instructions and examples are the
    /// same every time, so the next prompt only computes what follows the shared part.
    prefix: Option<(Vec<i64>, Vec<ArrayD<f32>>)>,
}

/// How many leading tokens two prompts share, leaving at least one new token to run.
pub fn reusable(previous: &[i64], next: &[i64]) -> usize {
    let same = previous
        .iter()
        .zip(next)
        .take_while(|(a, b)| a == b)
        .count();
    same.min(next.len().saturating_sub(1))
}

fn config_usize(c: &serde_json::Value, key: &str) -> Result<usize> {
    c.get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as usize)
        .ok_or_else(|| anyhow!("config.json has no {key}"))
}

pub fn load(dir: &Path, threads: usize) -> Result<Box<dyn TextModel>> {
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("config.json")).context("reading config.json")?,
    )
    .context("parsing config.json")?;
    let layers = config_usize(&config, "num_hidden_layers")?;
    let heads = config_usize(&config, "num_attention_heads")?;
    let kv_heads = config_usize(&config, "num_key_value_heads")?;
    let hidden = config_usize(&config, "hidden_size")?;
    let model = ["model.onnx", "model_int8.onnx"]
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.exists())
        .ok_or_else(|| anyhow!("no model.onnx in {}", dir.display()))?;
    let session = transcribe_rs::onnx::session::create_session_with_threads(&model, threads.max(1))
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("loading {}", model.display()))?;
    let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| anyhow!("{e}"))
        .context("reading tokenizer.json")?;
    let stop = ["<|im_end|>", "<|endoftext|>"]
        .iter()
        .filter_map(|t| tokenizer.token_to_id(t))
        .collect::<Vec<_>>();
    if stop.is_empty() {
        bail!("the tokenizer has no end-of-turn token");
    }
    let inputs = session
        .inputs()
        .iter()
        .map(|i| i.name().to_owned())
        .collect();
    let label = config
        .get("_name_or_path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("text model")
        .to_owned();
    Ok(Box::new(Llm {
        session,
        tokenizer,
        layers,
        kv_heads,
        // Qwen3 states it (128 for 0.6B, not hidden / heads = 64).
        head_dim: config_usize(&config, "head_dim").unwrap_or(hidden / heads),
        stop,
        label,
        model_type: config
            .get("model_type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        inputs,
        prefix: None,
    }))
}

/// The Qwen2 chat format: system message, example turns, then the real request.
pub fn chat_prompt(system: &str, examples: &[(String, String)], user: &str) -> String {
    let mut p = format!("<|im_start|>system\n{system}<|im_end|>\n");
    for (q, a) in examples {
        p.push_str(&format!(
            "<|im_start|>user\n{q}<|im_end|>\n<|im_start|>assistant\n{a}<|im_end|>\n"
        ));
    }
    p.push_str(&format!(
        "<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
    ));
    p
}

/// `chat_prompt` for a model family (`model_type` in config.json). Qwen3 would first
/// "think" aloud; its own template turns that off with an empty thinking block.
pub fn chat_prompt_for(
    model_type: Option<&str>,
    system: &str,
    examples: &[(String, String)],
    user: &str,
) -> String {
    let mut p = chat_prompt(system, examples, user);
    if model_type == Some("qwen3") {
        p.push_str("<think>\n\n</think>\n\n");
    }
    p
}

impl Llm {
    fn step(&mut self, ids: &[i64], past_len: usize, cache: &mut [ArrayD<f32>]) -> Result<i64> {
        let n = ids.len();
        let total = past_len + n;
        let mut feed: Vec<(Cow<'_, str>, ort::session::SessionInputValue<'_>)> = vec![
            (
                "input_ids".into(),
                ort::value::Value::from_array(Array2::from_shape_vec((1, n), ids.to_vec())?)?
                    .into_dyn()
                    .into(),
            ),
            (
                "attention_mask".into(),
                ort::value::Value::from_array(Array2::<i64>::ones((1, total)))?
                    .into_dyn()
                    .into(),
            ),
        ];
        if self.inputs.iter().any(|i| i == "position_ids") {
            let pos: Vec<i64> = (past_len as i64..total as i64).collect();
            feed.push((
                "position_ids".into(),
                ort::value::Value::from_array(Array2::from_shape_vec((1, n), pos)?)?
                    .into_dyn()
                    .into(),
            ));
        }
        for layer in 0..self.layers {
            for (k, kind) in ["key", "value"].iter().enumerate() {
                feed.push((
                    format!("past_key_values.{layer}.{kind}").into(),
                    ort::value::TensorRef::from_array_view(cache[layer * 2 + k].view())?.into(),
                ));
            }
        }
        let out = self.session.run(feed)?;
        let logits = out
            .get("logits")
            .ok_or_else(|| anyhow!("no logits output"))?
            .try_extract_array::<f32>()?;
        let last = logits.slice(ndarray::s![0, n - 1, ..]);
        let next = last
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i as i64)
            .ok_or_else(|| anyhow!("empty logits"))?;
        for layer in 0..self.layers {
            for (k, kind) in ["key", "value"].iter().enumerate() {
                let t = out
                    .get(format!("present.{layer}.{kind}").as_str())
                    .ok_or_else(|| anyhow!("no present.{layer}.{kind} output"))?
                    .try_extract_array::<f32>()?;
                cache[layer * 2 + k] = t.to_owned();
            }
        }
        Ok(next)
    }
}

impl TextModel for Llm {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn generate(
        &mut self,
        system: &str,
        examples: &[(String, String)],
        user: &str,
        max_tokens: usize,
    ) -> Result<String> {
        let prompt = chat_prompt_for(self.model_type.as_deref(), system, examples, user);
        let enc = self
            .tokenizer
            .encode(prompt.as_str(), false)
            .map_err(|e| anyhow!("{e}"))?;
        let prompt_ids: Vec<i64> = enc.get_ids().iter().map(|&i| i64::from(i)).collect();
        let (mut past, mut cache): (usize, Vec<ArrayD<f32>>) = match self.prefix.take() {
            Some((prev, cache)) => {
                let keep = reusable(&prev, &prompt_ids);
                let cut = cache
                    .iter()
                    .map(|c| c.slice_axis(Axis(2), Slice::from(..keep)).to_owned())
                    .collect();
                (keep, cut)
            }
            None => (
                0,
                (0..self.layers * 2)
                    .map(|_| ArrayD::zeros(IxDyn(&[1, self.kv_heads, 0, self.head_dim])))
                    .collect(),
            ),
        };
        let mut ids = prompt_ids[past..].to_vec();
        let mut out: Vec<u32> = Vec::new();
        for step in 0..max_tokens {
            let next = self.step(&ids, past, &mut cache)?;
            past += ids.len();
            if step == 0 {
                self.prefix = Some((prompt_ids.clone(), cache.clone()));
            }
            let token = u32::try_from(next)?;
            if self.stop.contains(&token) {
                break;
            }
            out.push(token);
            ids = vec![next];
        }
        self.tokenizer
            .decode(&out, true)
            .map_err(|e| anyhow!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_prompt_start_is_reused_but_one_token_is_always_run() {
        assert_eq!(reusable(&[1, 2, 3, 4], &[1, 2, 3, 9, 9]), 3);
        assert_eq!(reusable(&[1, 2, 3], &[1, 2, 3]), 2);
        assert_eq!(reusable(&[], &[5]), 0);
        assert_eq!(reusable(&[7], &[8, 9]), 0);
    }

    #[test]
    fn qwen3_answers_start_after_an_empty_thinking_block_and_others_do_not() {
        let p = chat_prompt_for(Some("qwen3"), "s", &[("q".into(), "a".into())], "hi");
        assert!(p.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
        // Example answers are plain turns, as in Qwen3's own chat template.
        assert!(p.contains("<|im_start|>assistant\na<|im_end|>"));
        let q2 = chat_prompt_for(Some("qwen2"), "s", &[], "hi");
        assert_eq!(q2, chat_prompt("s", &[], "hi"));
    }

    #[test]
    fn prompt_uses_the_chat_format_with_the_system_message_first() {
        let p = chat_prompt("be brief", &[("q".into(), "a".into())], "hi");
        assert!(p.contains("<|im_start|>user\nq<|im_end|>\n<|im_start|>assistant\na<|im_end|>"));
        assert!(p.starts_with("<|im_start|>system\nbe brief<|im_end|>"));
        assert!(p.ends_with("<|im_start|>assistant\n"));
    }

    /// Real model (AURAL_TEST_LLM_DIR): tidies a dictation and passes Aural's check.
    /// Run with `--features onnx -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_small_local_model_tidies_a_dictation() {
        let _ort = crate::test_support::ort_lock();
        crate::onnx_accel::select(crate::Backend::Cpu).unwrap();
        let dir = std::env::var("AURAL_TEST_LLM_DIR").expect("AURAL_TEST_LLM_DIR");
        let t0 = std::time::Instant::now();
        let mut m = load(Path::new(&dir), 4).unwrap();
        let load_ms = t0.elapsed().as_millis();
        let system = std::env::var("AURAL_TEST_LLM_SYSTEM").unwrap_or_else(|_| {
            "You tidy up dictated text. Fix punctuation and capitals and remove \
             hesitations (um, uh). Keep the words and meaning. Reply with the tidied text only."
                .into()
        });
        let examples = vec![
            (
                "<dictation>uh can you send me the report by tuesday</dictation>".to_string(),
                "Can you send me the report by Tuesday?".to_string(),
            ),
            (
                "<dictation>what time is it in tokyo right now</dictation>".to_string(),
                "What time is it in Tokyo right now?".to_string(),
            ),
        ];
        let t1 = std::time::Instant::now();
        let text = m
            .generate(
                &system,
                &examples,
                "<dictation>um so the meeting is at 3pm on friday uh in room 204</dictation>",
                64,
            )
            .unwrap();
        eprintln!(
            "{}: load {load_ms} ms, generate {} ms: {text}",
            m.label(),
            t1.elapsed().as_millis()
        );
        assert!(text.contains("3pm") && text.contains("204"), "{text}");
    }
}
