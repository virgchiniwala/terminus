use crate::providers::types::{ProviderError, ProviderRequest, ProviderResponse};
use crate::transport::ExecutionTransport;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

pub struct RelayTransport {
    relay_url: String,
}

impl RelayTransport {
    pub fn new(relay_url: impl Into<String>) -> Self {
        Self {
            relay_url: relay_url.into(),
        }
    }

    pub fn default_url() -> String {
        std::env::var("TERMINUS_RELAY_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "https://relay.terminus.run/dispatch".to_string())
    }

    fn require_token(keychain_token: Option<&str>) -> Result<&str, ProviderError> {
        keychain_token
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::non_retryable(
                    "Hosted plan token is not set. Sign in to Terminus and try again.",
                )
            })
    }

    fn classify_curl_failure(status: i32, stderr: &str) -> ProviderError {
        let retryable = matches!(status, 5 | 6 | 7 | 28 | 52 | 56);
        if retryable || stderr.to_ascii_lowercase().contains("could not resolve") {
            ProviderError::retryable(
                "Terminus relay is temporarily unavailable. Try again shortly.",
            )
        } else {
            ProviderError::non_retryable("Could not reach the Terminus relay.")
        }
    }

    fn classify_http_status(http_status: u16) -> ProviderError {
        match http_status {
            401 | 403 => ProviderError::non_retryable(
                "Your Terminus session needs attention. Sign in again and retry.",
            ),
            408 | 429 => ProviderError::retryable(
                "Terminus relay is rate limiting or temporarily unavailable. Try again shortly.",
            ),
            500..=599 => ProviderError::retryable(
                "Terminus relay is temporarily unavailable. Try again shortly.",
            ),
            _ => ProviderError::non_retryable("Terminus relay rejected this request."),
        }
    }

    fn curl_json_request(&self, token: &str, body_json: &Value) -> Result<Value, ProviderError> {
        let sentinel = "__TERMINUS_HTTP_STATUS__:";
        let mut config = String::new();
        config.push_str("silent\n");
        config.push_str("show-error\n");
        config.push_str("location\n");
        config.push_str("max-time = 30\n");
        config.push_str("request = \"POST\"\n");
        config.push_str(&format!("url = \"{}\"\n", self.relay_url));
        config.push_str("header = \"Content-Type: application/json\"\n");
        config.push_str(&format!("header = \"Authorization: Bearer {token}\"\n"));
        let body = serde_json::to_string(body_json)
            .map_err(|_| ProviderError::non_retryable("Relay request could not be encoded."))?;
        config.push_str(&format!("data = {body}\n"));
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
            return Err(Self::classify_curl_failure(code, &stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let (json_str, status_str) = stdout
            .rsplit_once(sentinel)
            .ok_or_else(|| ProviderError::retryable("Relay response could not be parsed."))?;
        let http_status: u16 = status_str.trim().parse().unwrap_or(0);
        if !(200..=299).contains(&http_status) {
            return Err(Self::classify_http_status(http_status));
        }

        serde_json::from_str(json_str.trim())
            .map_err(|_| ProviderError::retryable("Relay response could not be parsed."))
    }
}

impl ExecutionTransport for RelayTransport {
    fn dispatch(
        &self,
        request: &ProviderRequest,
        keychain_api_key: Option<&str>,
    ) -> Result<ProviderResponse, ProviderError> {
        let token = Self::require_token(keychain_api_key)?;
        let payload = serde_json::json!({
            "providerRequest": request
        });
        let json = self.curl_json_request(token, &payload)?;

        if let Some(inner) = json.get("providerResponse") {
            serde_json::from_value::<ProviderResponse>(inner.clone())
                .map_err(|_| ProviderError::retryable("Relay response could not be parsed."))
        } else {
            serde_json::from_value::<ProviderResponse>(json)
                .map_err(|_| ProviderError::retryable("Relay response could not be parsed."))
        }
    }

    fn requires_keychain_key(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::RelayTransport;

    #[test]
    fn default_url_uses_hosted_default_when_env_missing() {
        let url = RelayTransport::default_url();
        assert!(url.starts_with("http"));
    }
}
