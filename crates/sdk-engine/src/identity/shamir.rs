//! Two-tier Shamir Secret Sharing (SSS) over GF(2^8).

use sharks::{Share, Sharks};

use crate::error::SdkError;

/// Splits a 32-byte secret into `total_m` parts with threshold `threshold_n`.
/// Performed over the Galois Field GF(2^8).
pub fn split_secret(
    secret: &[u8; 32],
    threshold_n: u32,
    total_m: u32,
) -> Result<Vec<Vec<u8>>, SdkError> {
    if threshold_n > total_m {
        return Err(SdkError::InvalidArgument(
            "Threshold (N) cannot exceed the total number of trustees (M)".into(),
        ));
    }
    if !(1..=255).contains(&threshold_n) {
        return Err(SdkError::InvalidArgument(
            "Threshold must be between 1 and 255".into(),
        ));
    }

    let sharks = Sharks(threshold_n as u8);
    let dealer = sharks.dealer(secret.as_slice());

    let shares: Vec<Vec<u8>> = dealer
        .take(total_m as usize)
        .map(|share| Vec::from(&share))
        .collect();

    Ok(shares)
}

/// Reconstructs a 32-byte secret using at least `threshold_n` shares via Lagrange interpolation.
pub fn recover_secret(
    shares_bytes: &[Vec<u8>],
    threshold_n: u32,
) -> Result<[u8; 32], SdkError> {
    if shares_bytes.len() < threshold_n as usize {
        return Err(SdkError::InvalidArgument(
            "The number of shares is below the required threshold".into(),
        ));
    }

    let sharks = Sharks(threshold_n as u8);

    let valid_shares: Vec<Share> = shares_bytes
        .iter()
        .filter_map(|b| Share::try_from(b.as_slice()).ok())
        .collect();

    if valid_shares.len() < threshold_n as usize {
        return Err(SdkError::Internal(anyhow::anyhow!(
            "Some shares are corrupt or invalid"
        )));
    }

    let secret_vec = sharks.recover(&valid_shares).map_err(|_| {
        SdkError::Internal(anyhow::anyhow!(
            "Polynomial interpolation failed. Share is invalid or does not match"
        ))
    })?;

    if secret_vec.len() != 32 {
        return Err(SdkError::Internal(anyhow::anyhow!(
            "The reconstructed secret length does not match 32 bytes"
        )));
    }

    let mut recovered = [0u8; 32];
    recovered.copy_from_slice(&secret_vec);
    Ok(recovered)
}
