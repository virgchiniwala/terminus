use crate::providers::types::{ProviderError, ProviderKind};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const RELAY_SUBSCRIBER_TOKEN_SERVICE: &str = "terminus.relay.subscriber_token";
pub const RELAY_SUBSCRIBER_TOKEN_ACCOUNT: &str = "TerminusRelay";
pub const RELAY_CALLBACK_SECRET_SERVICE: &str = "terminus.relay.callback_secret";
pub const RELAY_CALLBACK_SECRET_ACCOUNT: &str = "TerminusRelayCallback";
pub const RELAY_DEVICE_ID_SERVICE: &str = "terminus.relay.device_id";
pub const RELAY_DEVICE_ID_ACCOUNT: &str = "TerminusRelayDevice";
pub const API_KEY_REF_SERVICE_PREFIX: &str = "terminus.api_key_ref.";
pub const API_KEY_REF_ACCOUNT: &str = "TerminusApiKeyRef";
pub const WEBHOOK_TRIGGER_SECRET_SERVICE_PREFIX: &str = "terminus.webhook_trigger_secret";
pub const CODEX_OAUTH_BUNDLE_SERVICE: &str = "terminus.openai.codex_oauth_bundle";
pub const CODEX_OAUTH_BUNDLE_ACCOUNT: &str = "TerminusOpenAiCodexOAuth";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexOauthBundle {
    pub auth_mode: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub account_id: Option<String>,
    pub last_refresh: Option<String>,
    pub imported_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct CodexCliAuthSnapshot {
    pub auth_mode: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub account_id: Option<String>,
    pub last_refresh: Option<String>,
    pub openai_api_key_present: bool,
}

#[derive(Debug, Deserialize)]
struct CodexCliAuthFile {
    #[serde(default)]
    auth_mode: String,
    #[serde(default)]
    last_refresh: Option<String>,
    #[serde(default)]
    tokens: CodexCliAuthTokens,
    #[serde(default, rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexCliAuthTokens {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

pub fn get_api_key(provider_kind: ProviderKind) -> Result<Option<String>, ProviderError> {
    let service = provider_kind.keychain_service_name();
    let output = Command::new("security")
        .arg("find-generic-password")
        .arg("-a")
        .arg("Terminus")
        .arg("-s")
        .arg(service)
        .arg("-w")
        .output()
        .map_err(|_| {
            ProviderError::non_retryable(
                "Could not access Keychain. Check local security settings.",
            )
        })?;

    if output.status.success() {
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            return Ok(None);
        }
        return Ok(Some(key));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not be found")
        || stderr.contains("The specified item could not be found")
    {
        return Ok(None);
    }

    Err(ProviderError::non_retryable(
        "Could not read provider key from Keychain.",
    ))
}

pub fn codex_cli_auth_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(".codex").join("auth.json");
    Some(path)
}

pub fn read_codex_cli_auth_snapshot() -> Result<Option<CodexCliAuthSnapshot>, ProviderError> {
    let Some(path) = codex_cli_auth_path() else {
        return Ok(None);
    };
    read_codex_cli_auth_snapshot_from_path(&path)
}

fn read_codex_cli_auth_snapshot_from_path(
    path: &Path,
) -> Result<Option<CodexCliAuthSnapshot>, ProviderError> {
    if !path.exists() {
        return Ok(None);
    }
    let body = fs::read_to_string(path)
        .map_err(|_| ProviderError::non_retryable("Could not read local Codex auth file."))?;
    let parsed: CodexCliAuthFile = serde_json::from_str(&body)
        .map_err(|_| ProviderError::non_retryable("Local Codex auth file is invalid."))?;

    let access_token = parsed
        .tokens
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ProviderError::non_retryable(
                "Local Codex auth is missing an access token. Reconnect Codex and try again.",
            )
        })?
        .to_string();

    let auth_mode = parsed.auth_mode.trim().to_string();
    if auth_mode.is_empty() {
        return Err(ProviderError::non_retryable(
            "Local Codex auth is missing auth mode metadata.",
        ));
    }

    Ok(Some(CodexCliAuthSnapshot {
        auth_mode,
        access_token,
        refresh_token: parsed
            .tokens
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string),
        account_id: parsed
            .tokens
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string),
        last_refresh: parsed.last_refresh.and_then(|v| {
            let t = v.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }),
        openai_api_key_present: parsed
            .openai_api_key
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty()),
    }))
}

pub fn set_secret(service: &str, account: &str, secret: &str) -> Result<(), ProviderError> {
    if secret.trim().is_empty() {
        return Err(ProviderError::non_retryable(
            "Secret cannot be empty for Keychain storage.",
        ));
    }

    let mut child = Command::new("security")
        .arg("add-generic-password")
        .arg("-a")
        .arg(account)
        .arg("-s")
        .arg(service)
        .arg("-w")
        .arg("-")
        .arg("-U")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| {
            ProviderError::non_retryable(
                "Could not write to Keychain. Check local security settings.",
            )
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(secret.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|_| ProviderError::non_retryable("Could not write secret to Keychain."))?;
    }
    let output = child.wait_with_output().map_err(|_| {
        ProviderError::non_retryable("Could not write to Keychain. Check local security settings.")
    })?;

    if output.status.success() {
        return Ok(());
    }

    Err(ProviderError::non_retryable(
        "Could not save secret to Keychain.",
    ))
}

pub fn get_secret(service: &str, account: &str) -> Result<Option<String>, ProviderError> {
    let output = Command::new("security")
        .arg("find-generic-password")
        .arg("-a")
        .arg(account)
        .arg("-s")
        .arg(service)
        .arg("-w")
        .output()
        .map_err(|_| {
            ProviderError::non_retryable(
                "Could not access Keychain. Check local security settings.",
            )
        })?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return Ok(None);
        }
        return Ok(Some(value));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not be found")
        || stderr.contains("The specified item could not be found")
    {
        return Ok(None);
    }

    Err(ProviderError::non_retryable(
        "Could not read secret from Keychain.",
    ))
}

pub fn delete_secret(service: &str, account: &str) -> Result<(), ProviderError> {
    let output = Command::new("security")
        .arg("delete-generic-password")
        .arg("-a")
        .arg(account)
        .arg("-s")
        .arg(service)
        .output()
        .map_err(|_| {
            ProviderError::non_retryable(
                "Could not access Keychain. Check local security settings.",
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not be found")
        || stderr.contains("The specified item could not be found")
    {
        return Ok(());
    }

    Err(ProviderError::non_retryable(
        "Could not delete secret from Keychain.",
    ))
}

pub fn get_codex_oauth_bundle() -> Result<Option<CodexOauthBundle>, ProviderError> {
    let Some(raw) = get_secret(CODEX_OAUTH_BUNDLE_SERVICE, CODEX_OAUTH_BUNDLE_ACCOUNT)? else {
        return Ok(None);
    };
    let bundle: CodexOauthBundle = serde_json::from_str(&raw)
        .map_err(|_| ProviderError::non_retryable("Stored Codex OAuth credentials are invalid."))?;
    Ok(Some(bundle))
}

pub fn set_codex_oauth_bundle(bundle: &CodexOauthBundle) -> Result<(), ProviderError> {
    let raw = serde_json::to_string(bundle)
        .map_err(|_| ProviderError::non_retryable("Could not encode Codex OAuth credentials."))?;
    set_secret(CODEX_OAUTH_BUNDLE_SERVICE, CODEX_OAUTH_BUNDLE_ACCOUNT, &raw)
}

pub fn delete_codex_oauth_bundle() -> Result<(), ProviderError> {
    delete_secret(CODEX_OAUTH_BUNDLE_SERVICE, CODEX_OAUTH_BUNDLE_ACCOUNT)
}

pub fn get_codex_oauth_access_token() -> Result<Option<String>, ProviderError> {
    Ok(get_codex_oauth_bundle()?.map(|b| b.access_token))
}

pub fn import_codex_oauth_from_local_auth(now_ms: i64) -> Result<CodexOauthBundle, ProviderError> {
    let snapshot = read_codex_cli_auth_snapshot()?.ok_or_else(|| {
        ProviderError::non_retryable("Codex auth was not found at ~/.codex/auth.json.")
    })?;
    if !snapshot.auth_mode.eq_ignore_ascii_case("chatgpt")
        && !snapshot.auth_mode.eq_ignore_ascii_case("codex")
    {
        return Err(ProviderError::non_retryable(
            "Local Codex auth mode is not supported for BYOK import.",
        ));
    }
    let bundle = CodexOauthBundle {
        auth_mode: snapshot.auth_mode,
        access_token: snapshot.access_token,
        refresh_token: snapshot.refresh_token,
        account_id: snapshot.account_id,
        last_refresh: snapshot.last_refresh,
        imported_at_ms: now_ms,
    };
    set_codex_oauth_bundle(&bundle)?;
    Ok(bundle)
}

pub fn get_relay_subscriber_token() -> Result<Option<String>, ProviderError> {
    get_secret(
        RELAY_SUBSCRIBER_TOKEN_SERVICE,
        RELAY_SUBSCRIBER_TOKEN_ACCOUNT,
    )
}

pub fn set_relay_subscriber_token(token: &str) -> Result<(), ProviderError> {
    set_secret(
        RELAY_SUBSCRIBER_TOKEN_SERVICE,
        RELAY_SUBSCRIBER_TOKEN_ACCOUNT,
        token,
    )
}

pub fn delete_relay_subscriber_token() -> Result<(), ProviderError> {
    delete_secret(
        RELAY_SUBSCRIBER_TOKEN_SERVICE,
        RELAY_SUBSCRIBER_TOKEN_ACCOUNT,
    )
}

pub fn get_relay_callback_secret() -> Result<Option<String>, ProviderError> {
    get_secret(RELAY_CALLBACK_SECRET_SERVICE, RELAY_CALLBACK_SECRET_ACCOUNT)
}

pub fn set_relay_callback_secret(secret: &str) -> Result<(), ProviderError> {
    set_secret(
        RELAY_CALLBACK_SECRET_SERVICE,
        RELAY_CALLBACK_SECRET_ACCOUNT,
        secret,
    )
}

pub fn delete_relay_callback_secret() -> Result<(), ProviderError> {
    delete_secret(RELAY_CALLBACK_SECRET_SERVICE, RELAY_CALLBACK_SECRET_ACCOUNT)
}

pub fn get_relay_device_id() -> Result<Option<String>, ProviderError> {
    get_secret(RELAY_DEVICE_ID_SERVICE, RELAY_DEVICE_ID_ACCOUNT)
}

pub fn set_relay_device_id(device_id: &str) -> Result<(), ProviderError> {
    set_secret(RELAY_DEVICE_ID_SERVICE, RELAY_DEVICE_ID_ACCOUNT, device_id)
}

fn api_key_ref_service(ref_name: &str) -> String {
    format!("{}{}", API_KEY_REF_SERVICE_PREFIX, ref_name.trim())
}

pub fn get_api_key_ref_secret(ref_name: &str) -> Result<Option<String>, ProviderError> {
    get_secret(&api_key_ref_service(ref_name), API_KEY_REF_ACCOUNT)
}

pub fn set_api_key_ref_secret(ref_name: &str, secret: &str) -> Result<(), ProviderError> {
    set_secret(&api_key_ref_service(ref_name), API_KEY_REF_ACCOUNT, secret)
}

pub fn delete_api_key_ref_secret(ref_name: &str) -> Result<(), ProviderError> {
    delete_secret(&api_key_ref_service(ref_name), API_KEY_REF_ACCOUNT)
}

fn webhook_trigger_secret_service(trigger_id: &str) -> String {
    format!("{WEBHOOK_TRIGGER_SECRET_SERVICE_PREFIX}.{trigger_id}")
}

pub fn get_webhook_trigger_secret(trigger_id: &str) -> Result<Option<String>, ProviderError> {
    get_secret(
        &webhook_trigger_secret_service(trigger_id),
        "TerminusWebhookTrigger",
    )
}

pub fn set_webhook_trigger_secret(trigger_id: &str, secret: &str) -> Result<(), ProviderError> {
    set_secret(
        &webhook_trigger_secret_service(trigger_id),
        "TerminusWebhookTrigger",
        secret,
    )
}

pub fn delete_webhook_trigger_secret(trigger_id: &str) -> Result<(), ProviderError> {
    delete_secret(
        &webhook_trigger_secret_service(trigger_id),
        "TerminusWebhookTrigger",
    )
}

#[cfg(test)]
mod tests {
    use super::read_codex_cli_auth_snapshot_from_path;
    use std::fs;

    #[test]
    fn parses_codex_cli_auth_snapshot_and_ignores_empty_openai_key() {
        let tmp = std::env::temp_dir().join(format!(
            "terminus_codex_auth_{}_{}.json",
            std::process::id(),
            1u64
        ));
        let body = r#"{
          "auth_mode": "chatgpt",
          "last_refresh": "2026-02-25T00:00:00Z",
          "OPENAI_API_KEY": "",
          "tokens": {
            "access_token": "at_123",
            "refresh_token": "rt_123",
            "account_id": "acct_123"
          }
        }"#;
        fs::write(&tmp, body).expect("write fixture");
        let snapshot = read_codex_cli_auth_snapshot_from_path(&tmp)
            .expect("parse")
            .expect("present");
        assert_eq!(snapshot.auth_mode, "chatgpt");
        assert_eq!(snapshot.access_token, "at_123");
        assert_eq!(snapshot.refresh_token.as_deref(), Some("rt_123"));
        assert_eq!(snapshot.account_id.as_deref(), Some("acct_123"));
        assert!(!snapshot.openai_api_key_present);
        let _ = fs::remove_file(tmp);
    }

    #[test]
    fn rejects_missing_access_token() {
        let tmp = std::env::temp_dir().join(format!(
            "terminus_codex_auth_{}_{}.json",
            std::process::id(),
            2u64
        ));
        let body = r#"{"auth_mode":"chatgpt","tokens":{"refresh_token":"rt_only"}}"#;
        fs::write(&tmp, body).expect("write fixture");
        let err = read_codex_cli_auth_snapshot_from_path(&tmp).expect_err("must fail");
        assert!(err
            .to_string()
            .to_ascii_lowercase()
            .contains("access token"));
        let _ = fs::remove_file(tmp);
    }
}
