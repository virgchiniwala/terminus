mod local_http;
mod mock;
mod relay;

pub use local_http::LocalHttpTransport;
pub use mock::MockTransport;
pub use relay::RelayTransport;

use crate::providers::types::{ProviderError, ProviderRequest, ProviderResponse};

pub trait ExecutionTransport: Send + Sync {
    fn dispatch(
        &self,
        request: &ProviderRequest,
        keychain_api_key: Option<&str>,
    ) -> Result<ProviderResponse, ProviderError>;

    fn requires_keychain_key(&self) -> bool {
        false
    }
}
