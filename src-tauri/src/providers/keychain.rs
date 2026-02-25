use crate::providers::types::{ProviderError, ProviderKind};
use std::io::Write;
use std::process::{Command, Stdio};

pub const RELAY_SUBSCRIBER_TOKEN_SERVICE: &str = "terminus.relay.subscriber_token";
pub const RELAY_SUBSCRIBER_TOKEN_ACCOUNT: &str = "TerminusRelay";
pub const RELAY_CALLBACK_SECRET_SERVICE: &str = "terminus.relay.callback_secret";
pub const RELAY_CALLBACK_SECRET_ACCOUNT: &str = "TerminusRelayCallback";
pub const RELAY_DEVICE_ID_SERVICE: &str = "terminus.relay.device_id";
pub const RELAY_DEVICE_ID_ACCOUNT: &str = "TerminusRelayDevice";

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
