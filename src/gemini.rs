use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Gemini REST API client for focus synthesis (text generation only).
///
/// Uses direct HTTP calls via reqwest, no SDK dependency.
/// API key read from `~/.config/gemini-api-key`.
pub struct GeminiClient {
    api_key: String,
    api_base: String,
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
            api_base: API_BASE.to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none()).build()?,
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

        let url = format!("{}/{}:generateContent", self.api_base, self.model);

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

            let mut key = reqwest::header::HeaderValue::from_str(&self.api_key)
                .map_err(|_| anyhow::anyhow!("Gemini API key has an invalid header format"))?;
            key.set_sensitive(true);
            let resp = self.client.post(&url)
                .header("x-goog-api-key", key)
                .json(&body).send().await
                .map_err(|error| anyhow::anyhow!("Gemini request failed: {}", error.without_url()))?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = (attempt + 1) * 30;
                eprintln!("    Rate limited, waiting {wait}s ({}/{MAX_RETRIES})...", attempt + 1);
                tokio::time::sleep(Duration::from_secs(wait as u64)).await;
                continue;
            }

            if !resp.status().is_success() {
                last_error = format!("HTTP {}", resp.status());
                if attempt < MAX_RETRIES - 1 {
                    eprintln!("    Retrying ({}/{MAX_RETRIES})...", attempt + 1);
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
                anyhow::bail!("Gemini API error: {last_error}");
            }

            let json: serde_json::Value = resp.json().await
                .map_err(|error| anyhow::anyhow!("Gemini response could not be decoded: {}", error.without_url()))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn local_client(base: String) -> GeminiClient {
        GeminiClient {
            api_key: "provider-key-fixture".into(), api_base: base,
            model: "demo".into(), client: reqwest::Client::builder().no_proxy().build().unwrap(),
            last_call: None,
        }
    }

    #[tokio::test]
    async fn connection_errors_do_not_include_provider_keys() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let error = local_client(base).generate("prompt fixture", "data fixture")
            .await.unwrap_err().to_string();
        assert!(!error.contains("provider-key-fixture"), "credential in transport diagnostic");
        assert!(!error.contains("?key="), "credential-bearing URL in diagnostic");
    }

    #[tokio::test]
    async fn provider_errors_exclude_response_payloads_and_key_urls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..MAX_RETRIES {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut data = vec![0; 8192];
                let n = stream.read(&mut data).await.unwrap();
                requests.push(String::from_utf8_lossy(&data[..n]).into_owned());
                let body = "private-response-fixture provider-key-fixture";
                let response = format!("HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        let error = local_client(base).generate("prompt fixture", "data fixture")
            .await.unwrap_err().to_string();
        let requests = task.await.unwrap();
        assert!(!error.contains("private-response-fixture"), "provider body in diagnostic");
        assert!(!error.contains("provider-key-fixture"), "provider credential in diagnostic");
        assert!(error.contains("400"), "status should remain useful");
        for request in requests {
            assert!(!request.lines().next().unwrap().contains("key="));
            assert!(request.to_ascii_lowercase().contains("x-goog-api-key: provider-key-fixture"));
        }
    }
}
