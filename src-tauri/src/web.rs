use serde::{Deserialize, Serialize};
use std::process::Command;
use thiserror::Error;

const FETCH_TIMEOUT_SECS: u32 = 15;
const MAX_REDIRECTS: usize = 3;
const MAX_RESPONSE_BYTES: usize = 200_000;
const EXCERPT_MAX_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchResult {
    pub url: String,
    pub fetched_at_ms: i64,
    pub status_code: u16,
    pub content_type: String,
    pub content_text: String,
    pub content_hash: String,
}

#[derive(Debug, Error)]
pub enum WebFetchError {
    #[error("Only HTTP or HTTPS URLs are supported.")]
    InvalidScheme,
    #[error("This website is not in the allowlist for this Autopilot.")]
    HostNotAllowlisted,
    #[error("Website fetch timed out. Try again.")]
    Timeout,
    #[error("Website is temporarily unavailable. Try again.")]
    RetryableNetwork,
    #[error("Website response was too large. Reduce scope.")]
    TooLarge,
    #[error("Website content type is not supported yet.")]
    UnsupportedContentType,
    #[error("Could not read website content.")]
    FetchFailed,
    #[error("Website redirected to an unsupported location.")]
    InvalidRedirect,
}

impl WebFetchError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::RetryableNetwork)
    }
}

pub fn fetch_allowlisted_text(
    url: &str,
    allowlisted_hosts: &[String],
) -> Result<WebFetchResult, WebFetchError> {
    let (scheme, host) = parse_scheme_host(url).ok_or(WebFetchError::InvalidScheme)?;
    validate_scheme(&scheme)?;
    validate_allowlist(&host, allowlisted_hosts)?;

    let mut current_url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let response = fetch_once(&current_url)?;
        if (300..400).contains(&response.status_code) {
            let location = response.location.ok_or(WebFetchError::InvalidRedirect)?;
            let next_url = resolve_redirect_url(&current_url, &location)
                .ok_or(WebFetchError::InvalidRedirect)?;
            let (next_scheme, next_host) =
                parse_scheme_host(&next_url).ok_or(WebFetchError::InvalidRedirect)?;
            validate_scheme(&next_scheme)?;
            validate_allowlist(&next_host, allowlisted_hosts)?;
            current_url = next_url;
            continue;
        }

        if !(200..300).contains(&response.status_code) {
            return Err(WebFetchError::FetchFailed);
        }
        if response.body.len() > MAX_RESPONSE_BYTES {
            return Err(WebFetchError::TooLarge);
        }

        let normalized_content_type = response
            .content_type
            .to_ascii_lowercase()
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if normalized_content_type != "text/html" && normalized_content_type != "text/plain" {
            return Err(WebFetchError::UnsupportedContentType);
        }

        let extracted = if normalized_content_type == "text/html" {
            html_to_text(&response.body)
        } else {
            collapse_whitespace(&response.body)
        };
        let excerpt = truncate_chars(&extracted, EXCERPT_MAX_CHARS);
        let content_hash = fnv1a_64_hex(&extracted);

        return Ok(WebFetchResult {
            url: current_url,
            fetched_at_ms: now_ms(),
            status_code: response.status_code,
            content_type: normalized_content_type,
            content_text: excerpt,
            content_hash,
        });
    }

    Err(WebFetchError::InvalidRedirect)
}

#[derive(Debug)]
struct SingleFetchResponse {
    status_code: u16,
    content_type: String,
    location: Option<String>,
    body: String,
}

fn fetch_once(url: &str) -> Result<SingleFetchResponse, WebFetchError> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            &FETCH_TIMEOUT_SECS.to_string(),
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--max-filesize",
            &MAX_RESPONSE_BYTES.to_string(),
            "--dump-header",
            "-",
            "--output",
            "-",
            url,
        ])
        .output()
        .map_err(|_| WebFetchError::RetryableNetwork)?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(1);
        return Err(match code {
            28 => WebFetchError::Timeout,
            63 => WebFetchError::TooLarge,
            5 | 6 | 7 | 52 | 56 => WebFetchError::RetryableNetwork,
            _ => WebFetchError::FetchFailed,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let (raw_headers, body) = split_headers_and_body(&stdout).ok_or(WebFetchError::FetchFailed)?;
    let status_code = parse_status_code(raw_headers).ok_or(WebFetchError::FetchFailed)?;
    let content_type = parse_header(raw_headers, "content-type").unwrap_or_default();
    let location = parse_header(raw_headers, "location");

    Ok(SingleFetchResponse {
        status_code,
        content_type,
        location,
        body: body.to_string(),
    })
}

fn split_headers_and_body(raw: &str) -> Option<(&str, &str)> {
    raw.find("\r\n\r\n")
        .map(|idx| (&raw[..idx], &raw[idx + 4..]))
        .or_else(|| raw.find("\n\n").map(|idx| (&raw[..idx], &raw[idx + 2..])))
}

fn parse_status_code(raw_headers: &str) -> Option<u16> {
    let first = raw_headers.lines().next()?.trim();
    let mut parts = first.split_whitespace();
    let _http = parts.next()?;
    let code = parts.next()?;
    code.parse().ok()
}

fn parse_header(raw_headers: &str, key: &str) -> Option<String> {
    raw_headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case(key) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn validate_scheme(scheme: &str) -> Result<(), WebFetchError> {
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        Ok(())
    } else {
        Err(WebFetchError::InvalidScheme)
    }
}

fn validate_allowlist(host: &str, allowlisted_hosts: &[String]) -> Result<(), WebFetchError> {
    let host_lc = host.to_ascii_lowercase();
    let allowed = allowlisted_hosts.iter().any(|entry| {
        let entry = entry.to_ascii_lowercase();
        host_lc == entry || host_lc.ends_with(&format!(".{entry}"))
    });
    if allowed {
        Ok(())
    } else {
        Err(WebFetchError::HostNotAllowlisted)
    }
}

pub fn parse_scheme_host(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let host_port = rest.split('/').next()?.trim();
    if host_port.is_empty() {
        return None;
    }
    let host = host_port.split('@').next_back()?.split(':').next()?.trim();
    if host.is_empty() {
        return None;
    }
    Some((scheme.to_string(), host.to_string()))
}

fn resolve_redirect_url(current_url: &str, location: &str) -> Option<String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Some(location.to_string());
    }
    let (scheme, host) = parse_scheme_host(current_url)?;
    if location.starts_with('/') {
        return Some(format!("{scheme}://{host}{location}"));
    }
    let base = current_url
        .rsplit_once('/')
        .map(|(b, _)| b)
        .unwrap_or(current_url);
    Some(format!("{base}/{location}"))
}

fn html_to_text(input: &str) -> String {
    let without_scripts = remove_tag_blocks(input, "script");
    let without_styles = remove_tag_blocks(&without_scripts, "style");
    let without_nav = remove_tag_blocks(&without_styles, "nav");
    let mut out = String::with_capacity(without_nav.len());
    let mut in_tag = false;
    for ch in without_nav.chars() {
        if ch == '<' {
            in_tag = true;
            out.push(' ');
            continue;
        }
        if ch == '>' {
            in_tag = false;
            out.push(' ');
            continue;
        }
        if !in_tag {
            out.push(ch);
        }
    }
    collapse_whitespace(&out)
}

fn remove_tag_blocks(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut remaining = input.to_string();
    let mut out = String::new();

    loop {
        let lower = remaining.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else {
            out.push_str(&remaining);
            break;
        };
        out.push_str(&remaining[..start]);
        let after_start = &remaining[start..];
        let after_start_lower = after_start.to_ascii_lowercase();
        let Some(end_rel) = after_start_lower.find(&close) else {
            break;
        };
        let end_idx = start + end_rel + close.len();
        remaining = remaining[end_idx..].to_string();
    }

    out
}

fn collapse_whitespace(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .trim()
        .to_string()
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect::<String>()
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn fnv1a_64_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
