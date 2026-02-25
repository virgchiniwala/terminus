use crate::providers::keychain;
use crate::providers::types::{ProviderError, ProviderRequest, ProviderResponse};
use crate::transport::{ExecutionTransport, LocalHttpTransport, MockTransport, RelayTransport};
use std::sync::OnceLock;

pub struct ProviderRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Mock,
    LocalHttp,
    Relay,
}

#[derive(Debug, Clone)]
pub struct TransportStatus {
    pub mode: TransportMode,
    pub relay_configured: bool,
    pub relay_url: String,
}

impl ProviderRuntime {
    pub fn default() -> Self {
        Self
    }

    pub fn transport_status(&self) -> TransportStatus {
        let relay_configured = keychain::get_relay_subscriber_token()
            .ok()
            .flatten()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);
        let relay_url = RelayTransport::default_url();
        let mode = Self::resolve_mode(relay_configured);
        TransportStatus {
            mode,
            relay_configured,
            relay_url,
        }
    }

    fn resolve_mode(relay_configured: bool) -> TransportMode {
        match std::env::var("TERMINUS_TRANSPORT") {
            Ok(mode) if mode.eq_ignore_ascii_case("relay") => TransportMode::Relay,
            Ok(mode) if mode.eq_ignore_ascii_case("local_http") => TransportMode::LocalHttp,
            Ok(mode) if mode.eq_ignore_ascii_case("mock") => TransportMode::Mock,
            _ if relay_configured => TransportMode::Relay,
            _ => TransportMode::Mock,
        }
    }

    pub fn dispatch(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let relay_token = keychain::get_relay_subscriber_token()?;
        let mode = Self::resolve_mode(relay_token.as_ref().is_some_and(|t| !t.trim().is_empty()));
        match mode {
            TransportMode::Relay => {
                let transport = Self::relay_transport();
                transport.dispatch(request, relay_token.as_deref())
            }
            TransportMode::LocalHttp => {
                let transport = Self::local_http_transport();
                let key = if transport.requires_keychain_key() {
                    keychain::get_api_key(request.provider_kind)?
                } else {
                    None
                };
                transport.dispatch(request, key.as_deref())
            }
            TransportMode::Mock => {
                let transport = Self::mock_transport();
                transport.dispatch(request, None)
            }
        }
    }

    fn local_http_transport() -> &'static LocalHttpTransport {
        static LOCAL: OnceLock<LocalHttpTransport> = OnceLock::new();
        LOCAL.get_or_init(LocalHttpTransport::new)
    }

    fn mock_transport() -> &'static MockTransport {
        static MOCK: OnceLock<MockTransport> = OnceLock::new();
        MOCK.get_or_init(MockTransport::new)
    }

    fn relay_transport() -> &'static RelayTransport {
        static RELAY: OnceLock<RelayTransport> = OnceLock::new();
        RELAY.get_or_init(|| RelayTransport::new(RelayTransport::default_url()))
    }
}

impl TransportMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::LocalHttp => "byok_local",
            Self::Relay => "hosted_relay",
        }
    }
}
