use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Gemini REST API client for focus synthesis (text generation only).
///
/// Uses direct HTTP calls via reqwest, no SDK dependency.
/// API key read from `~/.config/gemini-api-key`.
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    last_call: Option<Instant>,
}

const API_KEY_PATH: &str = ".config/gemini-api-key";
const DEFAULT_MODEL: &str = "gemini-3-pro-preview";
const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const MIN_INTERVAL: Duration = Duration::from_secs(1);
const MAX_RETRIES: u32 = 3;

impl GeminiClient {
    /// Create a new Gemini client, reading the API key from disk.
    pub fn new(model: Option<&str>) -> anyhow::Result<Self> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let key_path = PathBuf::from(&home).join(API_KEY_PATH);
        let api_key = std::fs::read_to_string(&key_path)
            .map(|s| s.trim().to_string())
            .map_err(|_| anyhow::anyhow!("Gemini API key not found at {}", key_path.display()))?;

        Ok(Self {
            api_key,
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            client: reqwest::Client::new(),
            last_call: None,
        })
    }

    /// Generate text content using Gemini REST API.
    pub async fn generate(&mut self, prompt: &str, data: &str) -> anyhow::Result<String> {
        // Rate limiting: ensure minimum interval between calls
        if let Some(last) = self.last_call {
            let elapsed = last.elapsed();
            if elapsed < MIN_INTERVAL {
                tokio::time::sleep(MIN_INTERVAL - elapsed).await;
            }
        }

        let url = format!(
            "{}/{}:generateContent?key={}",
            API_BASE, self.model, self.api_key
        );

        let full_prompt = format!("{prompt}\n\n---\n\n{data}");
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": full_prompt}]}],
            "generationConfig": {
                "temperature": 0.05,
                "topP": 0.95,
                "topK": 40
            }
        });

        let mut last_error = String::new();

        for attempt in 0..MAX_RETRIES {
            self.last_call = Some(Instant::now());

            let resp = self.client.post(&url).json(&body).send().await?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = (attempt + 1) * 30;
                eprintln!("    Rate limited, waiting {wait}s ({}/{MAX_RETRIES})...", attempt + 1);
                tokio::time::sleep(Duration::from_secs(wait as u64)).await;
                continue;
            }

            if !resp.status().is_success() {
                last_error = format!("HTTP {}: {}", resp.status(), resp.text().await.unwrap_or_default());
                if attempt < MAX_RETRIES - 1 {
                    eprintln!("    Retrying ({}/{MAX_RETRIES})...", attempt + 1);
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
                anyhow::bail!("Gemini API error: {last_error}");
            }

            let json: serde_json::Value = resp.json().await?;

            // Extract text from response
            let text = json["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .unwrap_or("");

            if text.is_empty() {
                if attempt < MAX_RETRIES - 1 {
                    eprintln!("    Empty response, retrying ({}/{MAX_RETRIES})...", attempt + 1);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                anyhow::bail!("Gemini returned empty response");
            }

            return Ok(text.trim().to_string());
        }

        anyhow::bail!("Gemini max retries exceeded: {last_error}")
    }
}
