use crate::providers::types::{ProviderError, ProviderKind};
use std::process::Command;

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
