use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    pub fn keychain_service_name(&self) -> &'static str {
        match self {
            Self::OpenAi => "terminus.openai.api_key",
            Self::Anthropic => "terminus.anthropic.api_key",
            Self::Gemini => "terminus.gemini.api_key",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTier {
    Supported,
    Experimental,
}

impl ProviderTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Experimental => "experimental",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub provider_kind: ProviderKind,
    pub provider_tier: ProviderTier,
    pub model: String,
    pub input: String,
    pub max_output_tokens: Option<u32>,
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub estimated_cost_usd_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub provider_kind: ProviderKind,
    pub provider_tier: ProviderTier,
    pub model: String,
    pub text: String,
    pub usage: ProviderUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Retryable,
    NonRetryable,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
}

impl ProviderError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Retryable,
            message: message.into(),
        }
    }

    pub fn non_retryable(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::NonRetryable,
            message: message.into(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self.kind, ProviderErrorKind::Retryable)
    }
}
