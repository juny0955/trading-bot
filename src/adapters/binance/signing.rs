use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SigningError {
    #[error("invalid HMAC key")]
    InvalidKey,
    #[error("system clock is before UNIX epoch: {0}")]
    Clock(#[from] SystemTimeError),
}

pub(crate) fn sign(secret: &str, payload: &str) -> Result<String, SigningError> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| SigningError::InvalidKey)?;
    mac.update(payload.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

pub(crate) fn timestamp_ms() -> Result<u64, SigningError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_is_stable_for_same_payload() -> anyhow::Result<()> {
        let first = sign("secret", "symbol=BTCUSDT&timestamp=1")?;
        let second = sign("secret", "symbol=BTCUSDT&timestamp=1")?;
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        Ok(())
    }
}
