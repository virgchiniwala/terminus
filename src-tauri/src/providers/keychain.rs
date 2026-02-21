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

pub fn set_secret(service: &str, account: &str, secret: &str) -> Result<(), ProviderError> {
    if secret.trim().is_empty() {
        return Err(ProviderError::non_retryable(
            "Secret cannot be empty for Keychain storage.",
        ));
    }

    let status = Command::new("security")
        .arg("add-generic-password")
        .arg("-a")
        .arg(account)
        .arg("-s")
        .arg(service)
        .arg("-w")
        .arg(secret)
        .arg("-U")
        .status()
        .map_err(|_| {
            ProviderError::non_retryable(
                "Could not write to Keychain. Check local security settings.",
            )
        })?;

    if status.success() {
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
