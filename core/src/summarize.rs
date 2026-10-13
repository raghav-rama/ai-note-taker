//! Meeting summarization module.
//!
//! Defines a `Summarizer` trait with a cloud implementation that calls the
//! Anthropic Messages API (Claude) to generate structured meeting summaries.
//!
//! The `CloudSummarizer` is only available when the `cloud` feature is enabled.

use crate::models::Summary;
use anyhow::Result;

/// Trait for meeting summarization backends.
pub trait Summarizer: Send + Sync {
    /// Summarize a full meeting transcript and return structured output.
    fn summarize(&self, transcript: &str) -> Result<Summary>;
}

/// Cloud-based summarizer using the Anthropic Messages API.
///
/// Requires the `cloud` feature and a valid Anthropic API key.
#[cfg(feature = "cloud")]
pub struct CloudSummarizer {
    api_key: String,
    model: String,
    runtime: tokio::runtime::Handle,
}

#[cfg(feature = "cloud")]
impl CloudSummarizer {
    /// Create a new cloud summarizer.
    ///
    /// - `api_key`: Anthropic API key.
    /// - `model`: Model identifier (e.g., "claude-sonnet-4-20250514").
    /// - `runtime`: Tokio runtime handle for executing async HTTP requests.
    pub fn new(api_key: String, model: Option<String>, runtime: tokio::runtime::Handle) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "claude-sonnet-4-20250514".to_string()),
            runtime,
        }
    }
}

#[cfg(feature = "cloud")]
impl Summarizer for CloudSummarizer {
    fn summarize(&self, transcript: &str) -> Result<Summary> {
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let transcript = transcript.to_string();

        self.runtime.block_on(async move {
            let client = reqwest::Client::new();

            let system_prompt = "You are a meeting summarization assistant. \
                Given a meeting transcript, produce a JSON object with exactly these fields:\n\
                - \"summary\": a concise narrative summary of the meeting\n\
                - \"key_points\": an array of key discussion points\n\
                - \"action_items\": an array of action items identified\n\
                - \"decisions\": an array of decisions made\n\n\
                Respond with ONLY the JSON object, no markdown fences or other text.";

            let body = serde_json::json!({
                "model": model,
                "max_tokens": 4096,
                "system": system_prompt,
                "messages": [
                    {
                        "role": "user",
                        "content": format!(
                            "Please summarize the following meeting transcript:\n\n{}",
                            transcript
                        )
                    }
                ]
            });

            let response = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("API request failed: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());
                return Err(anyhow::anyhow!(
                    "Anthropic API error ({}): {}",
                    status,
                    body_text
                ));
            }

            let resp_json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to parse API response: {}", e))?;

            // Extract the text content from the first content block.
            let content_text = resp_json["content"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|block| block["text"].as_str())
                .ok_or_else(|| anyhow::anyhow!("Unexpected API response structure"))?;

            let summary: Summary = serde_json::from_str(content_text)
                .map_err(|e| anyhow::anyhow!("Failed to parse summary JSON: {}", e))?;

            Ok(summary)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub summarizer for testing that returns fixed output.
    struct StubSummarizer;

    impl Summarizer for StubSummarizer {
        fn summarize(&self, _transcript: &str) -> Result<Summary> {
            Ok(Summary {
                summary: "Test meeting summary.".to_string(),
                key_points: vec!["Point 1".to_string()],
                action_items: vec!["Action 1".to_string()],
                decisions: vec!["Decision 1".to_string()],
            })
        }
    }

    #[test]
    fn test_stub_summarizer() {
        let summarizer = StubSummarizer;
        let result = summarizer.summarize("some transcript").unwrap();
        assert_eq!(result.summary, "Test meeting summary.");
        assert_eq!(result.key_points.len(), 1);
    }
}
