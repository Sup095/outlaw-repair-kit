//! Talking to a model.
//!
//! Two adapters cover everything the router can choose. The first speaks the
//! OpenAI wire format, which LM Studio, Ollama, vLLM, and llama.cpp's server
//! all implement -- so "a model on this machine" and "a model on the machine
//! in the other room" are the same code with a different address. The second
//! speaks Anthropic's API for the cloud tier.
//!
//! Nothing here decides *whether* to send anything. That is the router's job,
//! and the distinction matters: this module should be impossible to misuse
//! into sending diagnostics somewhere the user did not choose.

use std::time::Duration;

use anyhow::{Context, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::Result;

/// How long to wait for a model to respond.
///
/// Unlike everything in the diagnostic core, a request to a model does get an
/// upper bound. The reason is that this is a network request with no liveness
/// signal available -- a socket waiting on a server that will never answer
/// looks identical to one waiting on a server that is thinking. It is set
/// generously, because a large model on modest hardware is genuinely slow.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(300);

/// What a model was asked and what it said.
#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    /// Which model actually answered, as the server reported it. This can
    /// differ from what was requested.
    pub model: String,
}

/// Something that can answer a prompt.
#[async_trait]
pub trait ModelClient: Send + Sync {
    /// Human-readable description, for logs and for the UI.
    fn describe(&self) -> String;

    /// Ask the model a question and get its answer.
    async fn complete(&self, system: &str, user: &str) -> Result<Completion>;
}

fn http_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("could not create an HTTP client")
}

/// Trim a trailing slash so joining paths does not produce a double slash.
fn base(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// Check whether an OpenAI-compatible endpoint is answering, and what it has.
///
/// This is the router's reachability test. It is deliberately a real request
/// rather than a socket connect, because a port that accepts connections but
/// has no model loaded is not a usable endpoint.
pub async fn list_models(url: &str, timeout: Duration) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct ModelEntry {
        id: String,
    }
    #[derive(Deserialize)]
    struct ModelList {
        data: Vec<ModelEntry>,
    }

    let response = http_client(timeout)?
        .get(format!("{}/models", base(url)))
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;

    if !response.status().is_success() {
        bail!("{url} answered with {}", response.status());
    }

    let list: ModelList = response
        .json()
        .await
        .with_context(|| format!("{url} returned an unexpected reply"))?;
    Ok(list.data.into_iter().map(|entry| entry.id).collect())
}

/// A model reached over the OpenAI chat-completions API.
///
/// Used for both the local tier and the remote-machine tier; they differ only
/// in the address and whether a token is needed.
pub struct OpenAiCompatibleClient {
    url: String,
    model: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl OpenAiCompatibleClient {
    pub fn new(
        url: impl Into<String>,
        model: impl Into<String>,
        token: Option<String>,
    ) -> Result<Self> {
        Ok(Self {
            url: url.into(),
            model: model.into(),
            token,
            http: http_client(RESPONSE_TIMEOUT)?,
        })
    }
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    /// Low but not zero: this is analysis, not creative writing, and the same
    /// findings should produce broadly the same explanation twice.
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    model: String,
    choices: Vec<ChatChoice>,
}

#[async_trait]
impl ModelClient for OpenAiCompatibleClient {
    fn describe(&self) -> String {
        format!("{} at {}", self.model, self.url)
    }

    async fn complete(&self, system: &str, user: &str) -> Result<Completion> {
        let request = ChatRequest {
            model: &self.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system,
                },
                ChatMessage {
                    role: "user",
                    content: user,
                },
            ],
            temperature: 0.2,
        };

        let mut builder = self
            .http
            .post(format!("{}/chat/completions", base(&self.url)));
        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }

        let response = builder
            .json(&request)
            .send()
            .await
            .with_context(|| format!("could not reach {}", self.url))?;

        let status = response.status();
        if !status.is_success() {
            // The body usually explains what was wrong -- an unloaded model, a
            // context length exceeded -- and discarding it in favour of a bare
            // status code would throw away the only useful part.
            let body = response.text().await.unwrap_or_default();
            bail!("{} answered with {status}: {}", self.url, body.trim());
        }

        let parsed: ChatResponse = response
            .json()
            .await
            .context("the model returned a reply we could not read")?;
        let text = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .unwrap_or_default();

        if text.trim().is_empty() {
            bail!("the model returned an empty answer");
        }

        Ok(Completion {
            model: if parsed.model.is_empty() {
                self.model.clone()
            } else {
                parsed.model
            },
            text,
        })
    }
}

/// The cloud tier, speaking Anthropic's Messages API.
///
/// Rust has no official Anthropic SDK, so this is the documented raw HTTP
/// shape rather than a wrapper around one.
pub struct AnthropicClient {
    model: String,
    api_key: String,
    http: reqwest::Client,
}

/// Wire version for the Messages API. Pinned deliberately: this is the
/// contract the request below is written against, and it should not silently
/// change underneath us.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Generous enough for a full analysis without risking a truncated reply
/// mid-sentence.
const MAX_TOKENS: u32 = 16000;

impl AnthropicClient {
    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            model: model.into(),
            api_key: api_key.into(),
            http: http_client(RESPONSE_TIMEOUT)?,
        })
    }
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct AnthropicStopDetails {
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    explanation: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    model: String,
    #[serde(default)]
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    stop_details: Option<AnthropicStopDetails>,
}

/// Pull the answer out of a response, or explain why there is not one.
///
/// A refusal arrives as a perfectly successful HTTP 200 with a `stop_reason`
/// of `refusal`, so checking the status code alone would report a refusal as
/// an empty answer and leave the user wondering what went wrong.
fn extract_text(parsed: AnthropicResponse) -> Result<(String, String)> {
    if parsed.stop_reason.as_deref() == Some("refusal") {
        let category = parsed
            .stop_details
            .as_ref()
            .and_then(|details| details.category.clone())
            .unwrap_or_else(|| "unspecified".to_string());
        let explanation = parsed
            .stop_details
            .as_ref()
            .and_then(|details| details.explanation.clone())
            .unwrap_or_default();
        bail!(
            "the model declined to answer (category: {category}){}{}",
            if explanation.is_empty() { "" } else { ": " },
            explanation
        );
    }

    let text = parsed
        .content
        .into_iter()
        .filter(|block| block.kind == "text")
        .map(|block| block.text)
        .collect::<Vec<_>>()
        .join("");

    if text.trim().is_empty() {
        bail!("the model returned an empty answer");
    }
    Ok((text, parsed.model))
}

#[async_trait]
impl ModelClient for AnthropicClient {
    fn describe(&self) -> String {
        format!("{} (Anthropic API)", self.model)
    }

    async fn complete(&self, system: &str, user: &str) -> Result<Completion> {
        let request = AnthropicRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            system,
            messages: vec![ChatMessage {
                role: "user",
                content: user,
            }],
        };

        let response = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&request)
            .send()
            .await
            .context("could not reach the Anthropic API")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("the Anthropic API answered with {status}: {}", body.trim());
        }

        let parsed: AnthropicResponse = response
            .json()
            .await
            .context("the Anthropic API returned a reply we could not read")?;
        let (text, model) = extract_text(parsed)?;
        Ok(Completion {
            model: if model.is_empty() {
                self.model.clone()
            } else {
                model
            },
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(
        stop_reason: Option<&str>,
        blocks: Vec<(&str, &str)>,
        details: Option<AnthropicStopDetails>,
    ) -> AnthropicResponse {
        AnthropicResponse {
            model: "claude-opus-5".to_string(),
            content: blocks
                .into_iter()
                .map(|(kind, text)| AnthropicContentBlock {
                    kind: kind.to_string(),
                    text: text.to_string(),
                })
                .collect(),
            stop_reason: stop_reason.map(str::to_string),
            stop_details: details,
        }
    }

    #[test]
    fn text_blocks_are_joined_and_other_blocks_ignored() {
        let parsed = response(
            Some("end_turn"),
            vec![
                ("thinking", "ignored"),
                ("text", "first "),
                ("text", "second"),
            ],
            None,
        );
        let (text, model) = extract_text(parsed).expect("should extract");
        assert_eq!(text, "first second");
        assert_eq!(model, "claude-opus-5");
    }

    #[test]
    fn a_refusal_is_an_error_even_though_the_request_succeeded() {
        // A refusal arrives as HTTP 200. Treating it as a successful empty
        // answer would leave the user with no idea what happened.
        let parsed = response(
            Some("refusal"),
            vec![],
            Some(AnthropicStopDetails {
                category: Some("cyber".to_string()),
                explanation: Some("declined".to_string()),
            }),
        );
        let error = extract_text(parsed).expect_err("a refusal must be reported");
        let message = error.to_string();
        assert!(message.contains("declined to answer"), "got {message}");
        assert!(
            message.contains("cyber"),
            "the category should be surfaced: {message}"
        );
    }

    #[test]
    fn a_refusal_without_details_still_reports_clearly() {
        let parsed = response(Some("refusal"), vec![], None);
        let error = extract_text(parsed).expect_err("a refusal must be reported");
        assert!(error.to_string().contains("unspecified"));
    }

    #[test]
    fn an_empty_answer_is_an_error_rather_than_an_empty_report() {
        let parsed = response(Some("end_turn"), vec![("text", "   ")], None);
        assert!(extract_text(parsed).is_err());
    }

    #[test]
    fn trailing_slashes_in_a_base_url_do_not_produce_a_double_slash() {
        // People paste URLs with and without trailing slashes; both have to work.
        assert_eq!(
            base("http://localhost:1234/v1/"),
            "http://localhost:1234/v1"
        );
        assert_eq!(base("http://localhost:1234/v1"), "http://localhost:1234/v1");
    }
}
