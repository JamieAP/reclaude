use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use tokenizers::Tokenizer;
use tracing::debug;

const MODEL_DIR: &str = "models";
const MODEL_FILE: &str = "model_quantized.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";
const VECTOR_DIM: usize = 768;

/// HuggingFace repo for auto-download.
const HF_REPO: &str = "nomic-ai/nomic-embed-text-v1.5";

/// Event types worth embedding (conversation-tier content).
const EMBEDDABLE_TYPES: &[&str] = &[
    "user_prompt",
    "assistant",
    "plan",
    "thinking",
];

/// Check if an event type should get a vector embedding.
pub fn should_embed(event_type: &str) -> bool {
    EMBEDDABLE_TYPES.contains(&event_type)
}

/// Local ONNX embedder using nomic-embed-text-v1.5.
///
/// Produces 768-dim L2-normalized vectors.
/// Requires task prefixes: "search_document: " for indexing, "search_query: " for retrieval.
pub struct NomicEmbedder {
    session: Session,
    tokenizer: Tokenizer,
}

impl NomicEmbedder {
    /// Load model from ~/.reclaude/models/.
    /// Returns None if model files don't exist yet (use `download()` first).
    pub fn load() -> Result<Option<Self>> {
        let base = crate::db::base_dir();
        let model_dir = base.join(MODEL_DIR);
        let model_path = model_dir.join(MODEL_FILE);
        let tokenizer_path = model_dir.join(TOKENIZER_FILE);

        if !model_path.exists() || !tokenizer_path.exists() {
            return Ok(None);
        }

        Self::from_paths(&model_path, &tokenizer_path).map(Some)
    }

    fn from_paths(model_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        debug!("loading ONNX model from {}", model_path.display());

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)
            .context("failed to load ONNX model")?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load error: {e}"))?;

        Ok(Self { session, tokenizer })
    }

    /// Embed text for document indexing (uses "search_document: " prefix).
    pub fn embed_document(&mut self, text: &str) -> Result<Vec<f32>> {
        self.embed(text, "search_document: ")
    }

    /// Embed text for query/retrieval (uses "search_query: " prefix).
    pub fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
        self.embed(text, "search_query: ")
    }

    fn embed(&mut self, text: &str, prefix: &str) -> Result<Vec<f32>> {
        let prefixed = format!("{prefix}{text}");
        let encoding = self.tokenizer.encode(prefixed.as_str(), true)
            .map_err(|e| anyhow::anyhow!("tokenize error: {e}"))?;

        let seq_len = encoding.get_ids().len();

        // Cast u32 -> i64 for ONNX Runtime
        let input_ids: Vec<i64> = encoding.get_ids()
            .iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask()
            .iter().map(|&m| m as i64).collect();
        let token_type_ids: Vec<i64> = encoding.get_type_ids()
            .iter().map(|&t| t as i64).collect();

        // Create ort Tensors from (shape, data) tuples
        let input_ids_t = Tensor::from_array(([1usize, seq_len], input_ids))?;
        let attn_mask_t = Tensor::from_array(([1usize, seq_len], attention_mask.clone()))?;
        let type_ids_t = Tensor::from_array(([1usize, seq_len], token_type_ids))?;

        let outputs = self.session.run(ort::inputs![
            "input_ids" => input_ids_t,
            "attention_mask" => attn_mask_t,
            "token_type_ids" => type_ids_t
        ])?;

        // Output shape: [1, seq_len, 768] - flat buffer of per-token embeddings
        let (shape, token_data) = outputs[0].try_extract_tensor::<f32>()?;
        let hidden_size = shape[2] as usize;

        // Mean pool weighted by attention mask
        let mut pooled = vec![0.0f32; hidden_size];
        let mut mask_sum = 0.0f32;

        for i in 0..seq_len {
            let mask_val = attention_mask[i] as f32;
            if mask_val > 0.0 {
                let offset = i * hidden_size; // row offset in flat [1, seq_len, 768] buffer
                for j in 0..hidden_size {
                    pooled[j] += token_data[offset + j] * mask_val;
                }
                mask_sum += mask_val;
            }
        }

        if mask_sum > 0.0 {
            for val in pooled.iter_mut() {
                *val /= mask_sum;
            }
        }

        // L2 normalize
        let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in pooled.iter_mut() {
                *val /= norm;
            }
        }

        debug_assert_eq!(pooled.len(), VECTOR_DIM);
        Ok(pooled)
    }

    /// Return the model directory path.
    pub fn model_dir() -> PathBuf {
        crate::db::base_dir().join(MODEL_DIR)
    }

    /// Check if model files are downloaded.
    pub fn is_available() -> bool {
        let dir = Self::model_dir();
        dir.join(MODEL_FILE).exists() && dir.join(TOKENIZER_FILE).exists()
    }

    /// Download model files directly from HuggingFace. Returns the model directory.
    pub async fn download() -> Result<PathBuf> {
        let model_dir = Self::model_dir();
        std::fs::create_dir_all(&model_dir)?;

        eprintln!("Downloading nomic-embed-text-v1.5 ONNX model...");
        eprintln!("  Dest: {}", model_dir.display());

        let base_url = format!("https://huggingface.co/{HF_REPO}/resolve/main");
        let files = [
            (format!("{base_url}/onnx/{MODEL_FILE}"), MODEL_FILE),
            (format!("{base_url}/{TOKENIZER_FILE}"), TOKENIZER_FILE),
        ];

        let client = reqwest::Client::new();

        for (url, filename) in &files {
            let dest = model_dir.join(filename);
            if dest.exists() {
                eprintln!("  {filename}: already exists, skipping");
                continue;
            }

            eprint!("  {filename}: downloading...");
            let response = client
                .get(url)
                .send()
                .await
                .with_context(|| format!("failed to fetch {url}"))?;

            if !response.status().is_success() {
                anyhow::bail!("HTTP {} fetching {url}", response.status());
            }

            let total = response.content_length();
            let bytes = response.bytes().await?;

            std::fs::write(&dest, &bytes)
                .with_context(|| format!("failed to write {}", dest.display()))?;

            match total {
                Some(size) => eprintln!(" {:.1} MB", size as f64 / 1_048_576.0),
                None => eprintln!(" {} bytes", bytes.len()),
            }
        }

        eprintln!("Model downloaded successfully.");
        Ok(model_dir)
    }
}
