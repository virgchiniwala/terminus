use crate::providers::types::{
    ProviderError, ProviderKind, ProviderRequest, ProviderResponse, ProviderUsage,
};
use crate::transport::ExecutionTransport;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

pub struct LocalHttpTransport;

impl LocalHttpTransport {
    pub fn new() -> Self {
        Self
    }

    fn require_key(keychain_api_key: Option<&str>) -> Result<&str, ProviderError> {
        keychain_api_key
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::non_retryable(
                    "Provider key is not set. Add your API key in macOS Keychain and try again.",
                )
            })
    }

    fn classify_curl_failure(provider: &str, status: i32, stderr: &str) -> ProviderError {
        // Curl exit codes: https://curl.se/docs/manpage.html#EXIT-CODES
        // We avoid echoing stderr (it may include network details); only use it for classification.
        let retryable = matches!(status, 5 | 6 | 7 | 28 | 52 | 56);
        if retryable {
            ProviderError::retryable(format!(
                "{provider} is temporarily unavailable. Try again shortly."
            ))
        } else if stderr.to_ascii_lowercase().contains("could not resolve") {
            ProviderError::retryable(format!(
                "{provider} is temporarily unavailable. Try again shortly."
            ))
        } else {
            ProviderError::non_retryable(format!(
                "{provider} rejected the request. Update the input and try again."
            ))
        }
    }

    fn classify_http_status(provider: &str, http_status: u16) -> ProviderError {
        match http_status {
            401 | 403 => ProviderError::non_retryable(format!(
                "{provider} rejected the request. Check your API key and try again."
            )),
            408 | 429 => ProviderError::retryable(format!(
                "{provider} is rate limiting or temporarily unavailable. Try again shortly."
            )),
            500..=599 => ProviderError::retryable(format!(
                "{provider} is temporarily unavailable. Try again shortly."
            )),
            _ => ProviderError::non_retryable(format!(
                "{provider} rejected the request. Update the input and try again."
            )),
        }
    }

    fn curl_json_request(
        &self,
        provider: &str,
        url: &str,
        headers: &[(&str, String)],
        body_json: &Value,
    ) -> Result<Value, ProviderError> {
        // Security: put secrets only on stdin via curl config. Avoid passing API keys in argv.
        // Also avoid writing request bodies to disk.
        //
        // We append a sentinel line with the HTTP status code, then split on it.
        let sentinel = "__TERMINUS_HTTP_STATUS__:";

        let mut config = String::new();
        config.push_str("silent\n");
        config.push_str("show-error\n");
        config.push_str("location\n");
        config.push_str("max-time = 30\n");
        config.push_str("request = \"POST\"\n");
        config.push_str(&format!("url = \"{url}\"\n"));
        config.push_str("header = \"Content-Type: application/json\"\n");
        for (k, v) in headers {
            // v may include secrets; it stays on stdin (not argv).
            config.push_str(&format!("header = \"{k}: {v}\"\n"));
        }

        // Use single-line JSON to keep config parsing straightforward.
        let body = serde_json::to_string(body_json)
            .map_err(|_| ProviderError::non_retryable("Request could not be encoded."))?;
        config.push_str(&format!("data = {body}\n"));

        // Write out status code as a final line (stdout), separate from JSON.
        config.push_str(&format!("write-out = \"\\n{sentinel}%{{http_code}}\"\n"));

        let mut child = Command::new("curl")
            .arg("--config")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| ProviderError::retryable("Network transport is unavailable."))?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| ProviderError::non_retryable("Network transport is unavailable."))?;
            stdin
                .write_all(config.as_bytes())
                .map_err(|_| ProviderError::retryable("Network transport is unavailable."))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|_| ProviderError::retryable("Network transport is unavailable."))?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(1);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Self::classify_curl_failure(provider, code, &stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let (json_str, status_str) = stdout
            .rsplit_once(sentinel)
            .ok_or_else(|| ProviderError::retryable("Provider response could not be parsed."))?;

        let http_status: u16 = status_str.trim().parse().unwrap_or(0);
        if !(200..=299).contains(&http_status) {
            return Err(Self::classify_http_status(provider, http_status));
        }

        serde_json::from_str(json_str.trim())
            .map_err(|_| ProviderError::retryable("Provider response could not be parsed."))
    }

    fn dispatch_openai(
        &self,
        request: &ProviderRequest,
        keychain_api_key: Option<&str>,
    ) -> Result<ProviderResponse, ProviderError> {
        let key = Self::require_key(keychain_api_key)?;

        let body = serde_json::json!({
          "model": request.model,
          "messages": [{"role": "user", "content": request.input}],
          "max_tokens": request.max_output_tokens
        });

        let json = self.curl_json_request(
            "OpenAI",
            "https://api.openai.com/v1/chat/completions",
            &[("Authorization", format!("Bearer {key}"))],
            &body,
        )?;

        let text = json
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("message"))
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let input_tokens = json
            .get("usage")
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = json
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let estimated_cost_usd_cents = estimate_openai_cost_usd_cents(
            &request.model,
            input_tokens as i64,
            output_tokens as i64,
        );

        Ok(ProviderResponse {
            provider_kind: request.provider_kind,
            provider_tier: request.provider_tier,
            model: request.model.clone(),
            text,
            usage: ProviderUsage {
                input_tokens,
                output_tokens,
                estimated_cost_usd_cents,
            },
        })
    }

    fn dispatch_anthropic(
        &self,
        request: &ProviderRequest,
        keychain_api_key: Option<&str>,
    ) -> Result<ProviderResponse, ProviderError> {
        let key = Self::require_key(keychain_api_key)?;

        let max_tokens = request.max_output_tokens.unwrap_or(512).max(1);
        let body = serde_json::json!({
          "model": request.model,
          "max_tokens": max_tokens,
          "messages": [{"role": "user", "content": request.input}]
        });

        let json = self.curl_json_request(
            "Anthropic",
            "https://api.anthropic.com/v1/messages",
            &[
                ("x-api-key", key.to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            &body,
        )?;

        let text = json
            .get("content")
            .and_then(|v| v.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            b.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<String>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let input_tokens = json
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = json
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // Pricing intentionally not hard-coded for Anthropic yet; runner ledger will fall back to per-step estimates if this is 0.
        let estimated_cost_usd_cents = 0;

        Ok(ProviderResponse {
            provider_kind: request.provider_kind,
            provider_tier: request.provider_tier,
            model: request.model.clone(),
            text,
            usage: ProviderUsage {
                input_tokens,
                output_tokens,
                estimated_cost_usd_cents,
            },
        })
    }
}

impl ExecutionTransport for LocalHttpTransport {
    fn dispatch(
        &self,
        request: &ProviderRequest,
        keychain_api_key: Option<&str>,
    ) -> Result<ProviderResponse, ProviderError> {
        match request.provider_kind {
            ProviderKind::OpenAi => self.dispatch_openai(request, keychain_api_key),
            ProviderKind::Anthropic => self.dispatch_anthropic(request, keychain_api_key),
            ProviderKind::Gemini => Err(ProviderError::non_retryable(
                "Gemini local BYOK is not enabled yet. Use Mock transport for now.",
            )),
        }
    }

    fn requires_keychain_key(&self) -> bool {
        true
    }
}

fn estimate_openai_cost_usd_cents(model: &str, input_tokens: i64, output_tokens: i64) -> i64 {
    // Best-effort local estimate for caps/ledger. Authoritative billing is always the provider for BYOK.
    // Rates are USD per 1M tokens.
    let (usd_per_m_input, usd_per_m_output) = match model {
        "gpt-4o-mini" => (0.15, 0.60),
        // Unknown model: do not guess.
        _ => return 0,
    };

    let cost_usd = (input_tokens as f64 * usd_per_m_input / 1_000_000.0)
        + (output_tokens as f64 * usd_per_m_output / 1_000_000.0);
    (cost_usd * 100.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::LocalHttpTransport;
    use crate::providers::types::{ProviderKind, ProviderRequest, ProviderTier};
    use crate::transport::ExecutionTransport;

    // Env-gated integration tests. These require local Keychain keys and real network access.
    #[test]
    fn live_openai_call_is_env_gated() {
        if std::env::var("TERMINUS_RUN_LIVE_TESTS").ok().as_deref() != Some("1") {
            return;
        }

        let transport = LocalHttpTransport::new();
        let key =
            crate::providers::keychain::get_api_key(ProviderKind::OpenAi).expect("keychain access");
        let req = ProviderRequest {
            provider_kind: ProviderKind::OpenAi,
            provider_tier: ProviderTier::Supported,
            model: "gpt-4o-mini".to_string(),
            input: "Reply with the single word: ok".to_string(),
            max_output_tokens: Some(16),
            correlation_id: Some("live_openai_test".to_string()),
        };

        let resp = transport
            .dispatch(&req, key.as_deref())
            .expect("openai response");
        assert!(!resp.text.is_empty());
    }

    #[test]
    fn live_anthropic_call_is_env_gated() {
        if std::env::var("TERMINUS_RUN_LIVE_TESTS").ok().as_deref() != Some("1") {
            return;
        }

        let transport = LocalHttpTransport::new();
        let key = crate::providers::keychain::get_api_key(ProviderKind::Anthropic)
            .expect("keychain access");
        let req = ProviderRequest {
            provider_kind: ProviderKind::Anthropic,
            provider_tier: ProviderTier::Supported,
            model: "claude-3-5-sonnet-latest".to_string(),
            input: "Reply with the single word: ok".to_string(),
            max_output_tokens: Some(16),
            correlation_id: Some("live_anthropic_test".to_string()),
        };

        let resp = transport
            .dispatch(&req, key.as_deref())
            .expect("anthropic response");
        assert!(!resp.text.is_empty());
    }
}
