use std::path::Path;

pub(crate) type Result<T> = std::result::Result<T, CryptoDeviceError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CryptoDeviceError {
    /// IO error
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    /// Invalid secret key bytes
    #[error(transparent)]
    InvalidSecretKey(#[from] std::array::TryFromSliceError),
    /// Cannot unseal
    #[error(transparent)]
    UnsealError(#[from] crypto_box::aead::Error),
}

pub struct CryptoDevice {
    sk: crypto_box::SecretKey,
}

impl CryptoDevice {
    pub(crate) fn new(secret_key_path: impl AsRef<Path>) -> Result<Self> {
        let sk_bytes = std::fs::read(secret_key_path)?;
        let sk = crypto_box::SecretKey::from_slice(&sk_bytes)?;
        Ok(Self { sk })
    }

    pub(crate) fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        Ok(self.sk.unseal(ciphertext)?)
    }
}
