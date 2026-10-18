use rand::RngCore;

use crate::error::SdkError;

/// Sign a message using BIP-340 Schnorr signature on secp256k1.
pub fn sign_schnorr(secret_key: &[u8; 32], message: &[u8]) -> Result<[u8; 64], SdkError> {
    let signing_key = k256::schnorr::SigningKey::from_bytes(secret_key)
        .map_err(|_| SdkError::InvalidArgument("Invalid secret key for Schnorr signing".into()))?;

    let mut aux_random = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut aux_random);

    let signature = signing_key
        .sign_raw(message, &aux_random)
        .map_err(|_| SdkError::Internal(anyhow::anyhow!("Failed to generate Schnorr signature")))?;

    Ok(signature.to_bytes())
}

/// Verify a BIP-340 Schnorr signature over a message against an x-only public key.
pub fn verify_schnorr(public_key: &[u8; 32], message: &[u8], signature: &[u8]) -> bool {
    let Ok(pk) = k256::schnorr::VerifyingKey::from_bytes(public_key) else {
        return false;
    };

    let Ok(sig) = k256::schnorr::Signature::try_from(signature) else {
        return false;
    };

    pk.verify_raw(message, &sig).is_ok()
}
