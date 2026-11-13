//! Bounded scrypt implementation using a versioned encoded format.
use super::auth::{PasswordHasher, hex, internal, random_hex};
use lenso_app::{ActionError, ErrorCode};
use scrypt::{Params, scrypt};
use std::sync::Arc;
use subtle::ConstantTimeEq;
const PREFIX: &str = "scrypt$v1$32768$8$3$";
#[derive(Clone, Debug)]
pub struct ScryptHasher {
    permits: Arc<tokio::sync::Semaphore>,
}
impl Default for ScryptHasher {
    fn default() -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}
impl ScryptHasher {
    async fn derive(&self, password: String, salt: String) -> Result<[u8; 64], ActionError> {
        let permit = self.permits.clone().try_acquire_owned().map_err(|_| {
            ActionError::new(ErrorCode::Internal, "Password worker capacity reached")
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut out = [0; 64];
            scrypt(
                password.as_bytes(),
                salt.as_bytes(),
                &Params::new(15, 8, 3, 64).map_err(internal)?,
                &mut out,
            )
            .map_err(internal)?;
            Ok(out)
        })
        .await
        .map_err(internal)?
    }
}
impl PasswordHasher for ScryptHasher {
    async fn hash(&self, password: String) -> Result<String, ActionError> {
        let salt = random_hex(16)?;
        let hash = self.derive(password, salt.clone()).await?;
        Ok(format!("{PREFIX}{salt}${}", hex(&hash)))
    }
    async fn verify(&self, password: String, encoded: &str) -> Result<bool, ActionError> {
        let parts: Vec<_> = encoded
            .strip_prefix(PREFIX)
            .unwrap_or("")
            .split('$')
            .collect();
        if parts.len() != 2
            || parts[0].len() != 32
            || parts[1].len() != 128
            || !parts.iter().all(|s| {
                s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(ActionError::new(
                ErrorCode::Internal,
                "Unsupported stored password format",
            ));
        }
        let expected = parts[1].to_owned();
        let hash = hex(&self.derive(password, parts[0].into()).await?);
        Ok(bool::from(hash.as_bytes().ct_eq(expected.as_bytes())))
    }
    fn dummy(&self) -> String {
        format!("{PREFIX}{}${}", "00".repeat(16), "00".repeat(64))
    }
}
