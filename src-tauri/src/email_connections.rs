use crate::providers::keychain;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{
    distributions::{Alphanumeric, DistString},
    rngs::OsRng,
};
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

const KEYCHAIN_ACCOUNT: &str = "Terminus";
const OAUTH_SESSION_TTL_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectorMode {
    Mock,
    LocalHttp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageAction {
    Archive,
}

#[derive(Debug, Clone)]
pub struct OutboundEmailRequest<'a> {
    pub provider: EmailProvider,
    pub recipient: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub thread_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct OutboundEmailResult {
    pub provider_message_id: String,
    pub provider_thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TriageResult {
    pub provider_message_id: String,
    pub action: TriageAction,
}

#[derive(Debug, Clone)]
pub struct EffectorError {
    pub message: String,
    pub retryable: bool,
}

impl EffectorError {
    fn retryable(message: &str) -> Self {
        Self {
            message: message.to_string(),
            retryable: true,
        }
    }

    fn non_retryable(message: &str) -> Self {
        Self {
            message: message.to_string(),
            retryable: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmailProvider {
    Gmail,
    Microsoft365,
}

impl EmailProvider {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "gmail" => Some(Self::Gmail),
            "microsoft365" => Some(Self::Microsoft365),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Microsoft365 => "microsoft365",
        }
    }

    fn default_scopes(&self) -> &'static [&'static str] {
        match self {
            Self::Gmail => &[
                "openid",
                "email",
                "profile",
                "https://www.googleapis.com/auth/gmail.modify",
                "https://www.googleapis.com/auth/gmail.send",
            ],
            Self::Microsoft365 => &[
                "openid",
                "email",
                "profile",
                "offline_access",
                "Mail.ReadWrite",
                "Mail.Send",
            ],
        }
    }

    fn authorize_url(&self) -> &'static str {
        match self {
            Self::Gmail => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft365 => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }

    fn token_url(&self) -> &'static str {
        match self {
            Self::Gmail => "https://oauth2.googleapis.com/token",
            Self::Microsoft365 => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }

    fn userinfo_url(&self) -> &'static str {
        match self {
            Self::Gmail => "https://www.googleapis.com/oauth2/v3/userinfo",
            Self::Microsoft365 => {
                "https://graph.microsoft.com/v1.0/me?$select=mail,userPrincipalName"
            }
        }
    }

    fn keychain_service_name(&self) -> &'static str {
        match self {
            Self::Gmail => "terminus.gmail.oauth_tokens",
            Self::Microsoft365 => "terminus.microsoft365.oauth_tokens",
        }
    }
}

pub fn current_effector_mode() -> EffectorMode {
    match std::env::var("TERMINUS_EMAIL_EFFECTOR")
        .unwrap_or_else(|_| "local_http".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "mock" => EffectorMode::Mock,
        _ => EffectorMode::LocalHttp,
    }
}

pub fn send_outbound_email(
    connection: &Connection,
    request: OutboundEmailRequest<'_>,
) -> Result<OutboundEmailResult, EffectorError> {
    match current_effector_mode() {
        EffectorMode::Mock => Ok(OutboundEmailResult {
            provider_message_id: format!("mock_sent_{}", now_ms()),
            provider_thread_id: request.thread_id.map(|v| v.to_string()),
        }),
        EffectorMode::LocalHttp => send_outbound_email_live(connection, request),
    }
}

pub fn apply_triage_action(
    connection: &Connection,
    provider: EmailProvider,
    provider_message_id: &str,
    action: TriageAction,
) -> Result<TriageResult, EffectorError> {
    match current_effector_mode() {
        EffectorMode::Mock => Ok(TriageResult {
            provider_message_id: provider_message_id.to_string(),
            action,
        }),
        EffectorMode::LocalHttp => {
            apply_triage_action_live(connection, provider, provider_message_id, action)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailConnectionRecord {
    pub provider: String,
    pub status: String,
    pub account_email: Option<String>,
    pub scopes: Vec<String>,
    pub connected_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartResponse {
    pub provider: String,
    pub auth_url: String,
    pub state: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthConfigInput {
    pub provider: String,
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCompleteInput {
    pub provider: String,
    pub state: String,
    pub code: String,
}

pub fn list_connections(connection: &Connection) -> Result<Vec<EmailConnectionRecord>, String> {
    let mut output = Vec::new();
    for provider in [EmailProvider::Gmail, EmailProvider::Microsoft365] {
        let row: Option<(String, Option<String>, String, Option<i64>, i64, Option<String>)> =
            connection
                .query_row(
                    "SELECT status, account_email, scopes_json, connected_at_ms, updated_at_ms, last_error
                     FROM email_connections
                     WHERE provider = ?1",
                    params![provider.as_str()],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| format!("Failed to query email connection: {e}"))?;

        if let Some((
            status,
            account_email,
            scopes_json,
            connected_at_ms,
            updated_at_ms,
            last_error,
        )) = row
        {
            output.push(EmailConnectionRecord {
                provider: provider.as_str().to_string(),
                status,
                account_email,
                scopes: parse_scopes(&scopes_json),
                connected_at_ms,
                updated_at_ms,
                last_error,
            });
        } else {
            output.push(EmailConnectionRecord {
                provider: provider.as_str().to_string(),
                status: "disconnected".to_string(),
                account_email: None,
                scopes: provider
                    .default_scopes()
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
                connected_at_ms: None,
                updated_at_ms: now_ms(),
                last_error: None,
            });
        }
    }
    Ok(output)
}

pub fn upsert_oauth_config(connection: &Connection, input: OAuthConfigInput) -> Result<(), String> {
    let provider = EmailProvider::parse(input.provider.as_str())
        .ok_or_else(|| "Unsupported email provider.".to_string())?;
    let client_id = input.client_id.trim();
    let redirect_uri = input.redirect_uri.trim();
    if client_id.is_empty() || redirect_uri.is_empty() {
        return Err("Client ID and redirect URI are required.".to_string());
    }
    let _ =
        Url::parse(redirect_uri).map_err(|_| "Redirect URI must be a valid URL.".to_string())?;
    validate_redirect_uri(redirect_uri)?;

    connection
        .execute(
            "INSERT INTO email_oauth_config (provider, client_id, redirect_uri, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider) DO UPDATE SET
               client_id = excluded.client_id,
               redirect_uri = excluded.redirect_uri,
               updated_at_ms = excluded.updated_at_ms",
            params![provider.as_str(), client_id, redirect_uri, now_ms()],
        )
        .map_err(|e| format!("Failed to save OAuth config: {e}"))?;
    Ok(())
}

pub fn start_oauth(
    connection: &Connection,
    provider_raw: &str,
) -> Result<OAuthStartResponse, String> {
    let provider = EmailProvider::parse(provider_raw)
        .ok_or_else(|| "Unsupported email provider.".to_string())?;
    let (client_id, redirect_uri) = load_oauth_config(connection, provider)?;
    let state = random_token(36);
    let verifier = random_token(64);
    let challenge = pkce_challenge(&verifier);
    let now = now_ms();
    let expires_at = now + OAUTH_SESSION_TTL_MS;

    connection
        .execute(
            "INSERT INTO email_oauth_sessions (provider, state, code_verifier, created_at_ms, expires_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![provider.as_str(), state, verifier, now, expires_at],
        )
        .map_err(|e| format!("Failed to start OAuth session: {e}"))?;

    let scopes = provider.default_scopes().join(" ");
    let auth_url = match provider {
        EmailProvider::Gmail => Url::parse_with_params(
            provider.authorize_url(),
            &[
                ("client_id", client_id.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("response_type", "code"),
                ("scope", scopes.as_str()),
                ("state", state.as_str()),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
        ),
        EmailProvider::Microsoft365 => Url::parse_with_params(
            provider.authorize_url(),
            &[
                ("client_id", client_id.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("response_type", "code"),
                ("response_mode", "query"),
                ("scope", scopes.as_str()),
                ("state", state.as_str()),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
            ],
        ),
    }
    .map_err(|_| "Could not build OAuth URL.".to_string())?;

    Ok(OAuthStartResponse {
        provider: provider.as_str().to_string(),
        auth_url: auth_url.to_string(),
        state,
        expires_at_ms: expires_at,
    })
}

pub fn complete_oauth(
    connection: &Connection,
    input: OAuthCompleteInput,
) -> Result<EmailConnectionRecord, String> {
    let provider = EmailProvider::parse(input.provider.as_str())
        .ok_or_else(|| "Unsupported email provider.".to_string())?;
    let state = input.state.trim();
    let code = input.code.trim();
    if state.is_empty() || code.is_empty() {
        return Err("State and authorization code are required.".to_string());
    }

    let now = now_ms();
    let session: Option<(String, i64)> = connection
        .query_row(
            "SELECT code_verifier, expires_at_ms
             FROM email_oauth_sessions
             WHERE provider = ?1 AND state = ?2
             LIMIT 1",
            params![provider.as_str(), state],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Failed to load OAuth session: {e}"))?;
    let Some((code_verifier, expires_at_ms)) = session else {
        return Err("OAuth session was not found. Start connection again.".to_string());
    };
    if expires_at_ms < now {
        let _ = connection.execute(
            "DELETE FROM email_oauth_sessions WHERE provider = ?1 AND state = ?2",
            params![provider.as_str(), state],
        );
        return Err("This connection link expired. Start connection again.".to_string());
    }

    let (client_id, redirect_uri) = load_oauth_config(connection, provider)?;
    let token_json = exchange_token(provider, &client_id, &redirect_uri, code, &code_verifier)?;
    let access_token = token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provider did not return an access token.".to_string())?;
    let refresh_token = token_json.get("refresh_token").and_then(|v| v.as_str());
    let expires_in = token_json
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);
    let scope_str = token_json
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let scope_values = if scope_str.is_empty() {
        provider
            .default_scopes()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
    } else {
        scope_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
    };
    let account_email = fetch_account_email(provider, access_token).ok();
    let token_payload = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "expires_at_ms": now + expires_in.saturating_mul(1000),
        "token_type": token_json.get("token_type").and_then(|v| v.as_str()),
        "scope": scope_values,
    })
    .to_string();

    keychain::set_secret(
        provider.keychain_service_name(),
        KEYCHAIN_ACCOUNT,
        &token_payload,
    )
    .map_err(|e| e.message)?;

    connection
        .execute(
            "INSERT INTO email_connections (
               provider, status, account_email, scopes_json, connected_at_ms, updated_at_ms, last_error
             ) VALUES (?1, 'connected', ?2, ?3, ?4, ?4, NULL)
             ON CONFLICT(provider) DO UPDATE SET
               status = 'connected',
               account_email = excluded.account_email,
               scopes_json = excluded.scopes_json,
               connected_at_ms = excluded.connected_at_ms,
               updated_at_ms = excluded.updated_at_ms,
               last_error = NULL",
            params![
                provider.as_str(),
                account_email,
                serde_json::to_string(&scope_values).unwrap_or_else(|_| "[]".to_string()),
                now
            ],
        )
        .map_err(|e| format!("Failed to persist connection status: {e}"))?;

    connection
        .execute(
            "DELETE FROM email_oauth_sessions WHERE provider = ?1 AND state = ?2",
            params![provider.as_str(), state],
        )
        .map_err(|e| format!("Failed to clear OAuth session: {e}"))?;

    Ok(EmailConnectionRecord {
        provider: provider.as_str().to_string(),
        status: "connected".to_string(),
        account_email,
        scopes: scope_values,
        connected_at_ms: Some(now),
        updated_at_ms: now,
        last_error: None,
    })
}

pub fn disconnect(connection: &Connection, provider_raw: &str) -> Result<(), String> {
    let provider = EmailProvider::parse(provider_raw)
        .ok_or_else(|| "Unsupported email provider.".to_string())?;
    keychain::delete_secret(provider.keychain_service_name(), KEYCHAIN_ACCOUNT)
        .map_err(|e| e.message)?;
    connection
        .execute(
            "DELETE FROM email_connections WHERE provider = ?1",
            params![provider.as_str()],
        )
        .map_err(|e| format!("Failed to clear connection status: {e}"))?;
    connection
        .execute(
            "DELETE FROM email_oauth_sessions WHERE provider = ?1",
            params![provider.as_str()],
        )
        .map_err(|e| format!("Failed to clear OAuth sessions: {e}"))?;
    Ok(())
}

pub fn get_access_token(
    connection: &Connection,
    provider: EmailProvider,
) -> Result<String, String> {
    let payload_raw = keychain::get_secret(provider.keychain_service_name(), KEYCHAIN_ACCOUNT)
        .map_err(|e| e.message)?
        .ok_or_else(|| "Provider is not connected yet.".to_string())?;
    let payload: Value = serde_json::from_str(&payload_raw)
        .map_err(|_| "Stored provider session is invalid. Reconnect this provider.".to_string())?;
    let now = now_ms();
    let expires_at_ms = payload
        .get("expires_at_ms")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let access_token = payload
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !access_token.is_empty() && expires_at_ms > now + 60_000 {
        return Ok(access_token.to_string());
    }

    let refresh_token = payload
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let _ = disconnect(connection, provider.as_str());
            "Session expired and refresh token is missing. Reconnect provider.".to_string()
        })?;
    let (client_id, _redirect_uri) = load_oauth_config(connection, provider)?;
    let refreshed = refresh_access_token(provider, &client_id, refresh_token)?;
    let next_access = refreshed
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Could not refresh provider session. Reconnect provider.".to_string())?;
    let next_expires = refreshed
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);
    let next_scope = refreshed
        .get("scope")
        .and_then(|v| v.as_str())
        .map(|s| {
            s.split_whitespace()
                .map(|part| part.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|| {
            payload
                .get("scope")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|x| x.to_string()))
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default()
        });

    let merged_payload = json!({
        "access_token": next_access,
        "refresh_token": refresh_token,
        "expires_at_ms": now + next_expires.saturating_mul(1000),
        "token_type": refreshed.get("token_type").and_then(|v| v.as_str()),
        "scope": next_scope,
    })
    .to_string();
    keychain::set_secret(
        provider.keychain_service_name(),
        KEYCHAIN_ACCOUNT,
        &merged_payload,
    )
    .map_err(|e| e.message)?;
    Ok(next_access.to_string())
}

fn send_outbound_email_live(
    connection: &Connection,
    request: OutboundEmailRequest<'_>,
) -> Result<OutboundEmailResult, EffectorError> {
    if request.recipient.trim().is_empty() {
        return Err(EffectorError::non_retryable(
            "Recipient is missing for this send action.",
        ));
    }
    let token = get_access_token(connection, request.provider)
        .map_err(|e| EffectorError::non_retryable(&e))?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| EffectorError::retryable("Could not initialize secure network client."))?;

    match request.provider {
        EmailProvider::Gmail => {
            let mut mime = format!(
                "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\n\r\n{}",
                request.recipient.trim(),
                request.subject.trim(),
                request.body
            );
            if mime.len() > 120_000 {
                mime.truncate(120_000);
            }
            let raw = URL_SAFE_NO_PAD.encode(mime.as_bytes());
            let mut payload = json!({ "raw": raw });
            if let Some(thread_id) = request.thread_id {
                if !thread_id.trim().is_empty() {
                    payload["threadId"] = Value::String(thread_id.trim().to_string());
                }
            }
            let response = client
                .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
                .bearer_auth(&token)
                .json(&payload)
                .send()
                .map_err(|_| {
                    EffectorError::retryable(
                        "Could not reach Gmail send endpoint. Try again shortly.",
                    )
                })?;
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                return Err(EffectorError::retryable(
                    "Gmail is temporarily unavailable. Terminus will retry.",
                ));
            }
            if !response.status().is_success() {
                return Err(EffectorError::non_retryable(
                    "Gmail rejected this send action. Check recipient and permissions.",
                ));
            }
            let json = response
                .json::<Value>()
                .map_err(|_| EffectorError::retryable("Could not parse Gmail send response."))?;
            Ok(OutboundEmailResult {
                provider_message_id: json
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("gmail_sent_unknown")
                    .to_string(),
                provider_thread_id: json
                    .get("threadId")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string()),
            })
        }
        EmailProvider::Microsoft365 => {
            let payload = json!({
                "message": {
                    "subject": request.subject,
                    "body": { "contentType": "Text", "content": request.body },
                    "toRecipients": [
                        { "emailAddress": { "address": request.recipient.trim() } }
                    ]
                },
                "saveToSentItems": true
            });
            let response = client
                .post("https://graph.microsoft.com/v1.0/me/sendMail")
                .bearer_auth(&token)
                .json(&payload)
                .send()
                .map_err(|_| {
                    EffectorError::retryable(
                        "Could not reach Microsoft 365 send endpoint. Try again shortly.",
                    )
                })?;
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                return Err(EffectorError::retryable(
                    "Microsoft 365 is temporarily unavailable. Terminus will retry.",
                ));
            }
            if !(response.status().is_success() || response.status().as_u16() == 202) {
                return Err(EffectorError::non_retryable(
                    "Microsoft 365 rejected this send action. Check recipient and permissions.",
                ));
            }
            Ok(OutboundEmailResult {
                provider_message_id: format!("graph_sent_{}", now_ms()),
                provider_thread_id: request.thread_id.map(|v| v.to_string()),
            })
        }
    }
}

fn apply_triage_action_live(
    connection: &Connection,
    provider: EmailProvider,
    provider_message_id: &str,
    action: TriageAction,
) -> Result<TriageResult, EffectorError> {
    let token =
        get_access_token(connection, provider).map_err(|e| EffectorError::non_retryable(&e))?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| EffectorError::retryable("Could not initialize secure network client."))?;

    match (provider, action) {
        (EmailProvider::Gmail, TriageAction::Archive) => {
            let endpoint = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/modify",
                provider_message_id
            );
            let response = client
                .post(endpoint)
                .bearer_auth(&token)
                .json(&json!({"removeLabelIds": ["INBOX"]}))
                .send()
                .map_err(|_| {
                    EffectorError::retryable(
                        "Could not reach Gmail triage endpoint. Try again shortly.",
                    )
                })?;
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                return Err(EffectorError::retryable(
                    "Gmail triage is temporarily unavailable. Terminus will retry.",
                ));
            }
            if !response.status().is_success() {
                return Err(EffectorError::non_retryable(
                    "Gmail rejected this triage action.",
                ));
            }
        }
        (EmailProvider::Microsoft365, TriageAction::Archive) => {
            let endpoint = format!(
                "https://graph.microsoft.com/v1.0/me/messages/{}/move",
                provider_message_id
            );
            let response = client
                .post(endpoint)
                .bearer_auth(&token)
                .json(&json!({ "destinationId": "archive" }))
                .send()
                .map_err(|_| {
                    EffectorError::retryable(
                        "Could not reach Microsoft 365 triage endpoint. Try again shortly.",
                    )
                })?;
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                return Err(EffectorError::retryable(
                    "Microsoft 365 triage is temporarily unavailable. Terminus will retry.",
                ));
            }
            if !response.status().is_success() {
                return Err(EffectorError::non_retryable(
                    "Microsoft 365 rejected this triage action.",
                ));
            }
        }
    }

    Ok(TriageResult {
        provider_message_id: provider_message_id.to_string(),
        action,
    })
}

fn load_oauth_config(
    connection: &Connection,
    provider: EmailProvider,
) -> Result<(String, String), String> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT client_id, redirect_uri FROM email_oauth_config WHERE provider = ?1",
            params![provider.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Failed to read OAuth configuration: {e}"))?;
    row.ok_or_else(|| {
        "Set up this provider first by saving Client ID and Redirect URI.".to_string()
    })
}

fn exchange_token(
    provider: EmailProvider,
    client_id: &str,
    redirect_uri: &str,
    code: &str,
    code_verifier: &str,
) -> Result<Value, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize secure network client.".to_string())?;
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
    ];
    let response = client
        .post(provider.token_url())
        .form(&params)
        .send()
        .map_err(|_| {
            "Could not reach provider token endpoint. Check network and try again.".to_string()
        })?;

    if !response.status().is_success() {
        return Err(
            "Provider rejected the authorization code. Start connection again.".to_string(),
        );
    }

    response
        .json::<Value>()
        .map_err(|_| "Could not parse provider token response.".to_string())
}

fn refresh_access_token(
    provider: EmailProvider,
    client_id: &str,
    refresh_token: &str,
) -> Result<Value, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize secure network client.".to_string())?;
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ];
    let response = client
        .post(provider.token_url())
        .form(&params)
        .send()
        .map_err(|_| {
            "Could not refresh provider session. Check network and try again.".to_string()
        })?;
    if !response.status().is_success() {
        return Err("Could not refresh provider session. Reconnect provider.".to_string());
    }
    response
        .json::<Value>()
        .map_err(|_| "Could not parse provider refresh response.".to_string())
}

fn fetch_account_email(provider: EmailProvider, access_token: &str) -> Result<String, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize secure network client.".to_string())?;
    let response = client
        .get(provider.userinfo_url())
        .bearer_auth(access_token)
        .send()
        .map_err(|_| "Connected, but couldn't fetch account details.".to_string())?;
    if !response.status().is_success() {
        return Err("Connected, but couldn't fetch account details.".to_string());
    }
    let json = response
        .json::<Value>()
        .map_err(|_| "Connected, but couldn't read account details.".to_string())?;
    let email = match provider {
        EmailProvider::Gmail => json.get("email").and_then(|v| v.as_str()),
        EmailProvider::Microsoft365 => json
            .get("mail")
            .and_then(|v| v.as_str())
            .or_else(|| json.get("userPrincipalName").and_then(|v| v.as_str())),
    };
    email
        .map(|e| e.to_string())
        .ok_or_else(|| "Connected, but couldn't resolve account email.".to_string())
}

fn parse_scopes(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn random_token(len: usize) -> String {
    Alphanumeric.sample_string(&mut OsRng, len)
}

fn validate_redirect_uri(raw: &str) -> Result<(), String> {
    let parsed = Url::parse(raw).map_err(|_| "Redirect URI must be a valid URL.".to_string())?;
    match parsed.scheme() {
        "http" => {
            let host = parsed
                .host_str()
                .ok_or_else(|| "Redirect URI must include a host.".to_string())?;
            if host != "127.0.0.1" && host != "localhost" {
                return Err(
                    "Redirect URI must use localhost for local OAuth callbacks.".to_string()
                );
            }
            if parsed.port().is_none() {
                return Err("Redirect URI must include a localhost port.".to_string());
            }
            Ok(())
        }
        "terminus" => Ok(()),
        _ => Err("Redirect URI must use localhost or the Terminus app scheme.".to_string()),
    }
}

fn pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(digest)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::validate_redirect_uri;

    #[test]
    fn redirect_uri_allows_localhost_http_with_port() {
        assert!(validate_redirect_uri("http://127.0.0.1:3000/callback").is_ok());
        assert!(validate_redirect_uri("http://localhost:5173/oauth").is_ok());
    }

    #[test]
    fn redirect_uri_allows_terminus_scheme() {
        assert!(validate_redirect_uri("terminus://oauth/callback").is_ok());
    }

    #[test]
    fn redirect_uri_rejects_non_local_http_and_https() {
        assert!(validate_redirect_uri("https://attacker.example/callback").is_err());
        assert!(validate_redirect_uri("http://example.com:8080/callback").is_err());
    }
}
