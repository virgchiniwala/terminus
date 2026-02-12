use crate::providers::keychain;
use crate::providers::types::{ProviderError, ProviderRequest, ProviderResponse};
use crate::transport::{ExecutionTransport, LocalHttpTransport, MockTransport};
use std::sync::{Arc, OnceLock};

pub struct ProviderRuntime {
    transport: Arc<dyn ExecutionTransport>,
}

impl ProviderRuntime {
    pub fn default() -> Self {
        static TRANSPORT: OnceLock<Arc<dyn ExecutionTransport>> = OnceLock::new();
        let transport = TRANSPORT.get_or_init(|| match std::env::var("TERMINUS_TRANSPORT") {
            Ok(mode) if mode.eq_ignore_ascii_case("local_http") => {
                Arc::new(LocalHttpTransport::new()) as Arc<dyn ExecutionTransport>
            }
            _ => Arc::new(MockTransport::new()) as Arc<dyn ExecutionTransport>,
        });
        Self {
            transport: Arc::clone(transport),
        }
    }

    pub fn dispatch(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let key = match self.transport.requires_keychain_key() {
            true => keychain::get_api_key(request.provider_kind)?,
            false => None,
        };

        self.transport.dispatch(request, key.as_deref())
    }
}
