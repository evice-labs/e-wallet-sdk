use sha2::{Digest, Sha256};

use super::ecdh;
use super::shamir;
use super::signature;
use super::types::{
    AttestationPayload, EncryptedShare, IdentityCommitment, IdentitySecret, RegistrationPayload,
    ScopedNullifier,
};
use crate::error::SdkError;

/// Derive the master identity secret from the BIP-39 root seed using HKDF-SHA256.
///
/// Salt: `"lez/identity/v1/secret"`
/// Info: `"IdentityMasterKey"`
pub fn derive_identity_secret(root_seed: &[u8]) -> IdentitySecret {
    // HKDF-Extract: PRK = HMAC-SHA256(salt, IKM)
    let salt = b"lez/identity/v1/secret";
    let prk = hmac_sha256(salt, root_seed);

    // HKDF-Expand: OKM = HMAC-SHA256(PRK, info || 0x01)
    let info = b"IdentityMasterKey";
    let mut expand_input = Vec::with_capacity(info.len() + 1);
    expand_input.extend_from_slice(info);
    expand_input.push(0x01);

    let okm = hmac_sha256(&prk, &expand_input);
    IdentitySecret::new(okm)
}

/// Derive the public identity commitment: `SHA-256("lez/identity/v1/commitment" || IdentitySecret)`.
pub fn derive_identity_commitment(secret: &IdentitySecret) -> IdentityCommitment {
    let mut hasher = Sha256::new();
    hasher.update(b"lez/identity/v1/commitment");
    hasher.update(secret.as_bytes());
    IdentityCommitment::new(hasher.finalize().into())
}

/// Derive a scope-specific pseudonymous nullifier: `SHA-256("lez/identity/v1/scoped_nullifier" || IdentitySecret || ScopeId)`.
pub fn derive_scoped_nullifier(secret: &IdentitySecret, scope_id: &str) -> ScopedNullifier {
    let mut hasher = Sha256::new();
    hasher.update(b"lez/identity/v1/scoped_nullifier");
    hasher.update(secret.as_bytes());
    hasher.update(scope_id.as_bytes());
    ScopedNullifier::new(hasher.finalize().into())
}

/// Derive a deterministic BIP-340 Schnorr private key scoped to a specific context.
pub fn derive_scope_attestation_key(secret: &IdentitySecret, scope_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"lez/identity/v1/attestation_key");
    hasher.update(secret.as_bytes());
    hasher.update(scope_id.as_bytes());
    let mut candidate: [u8; 32] = hasher.finalize().into();

    // Ensure it is a valid secp256k1 scalar
    let mut counter = 1u32;
    while k256::SecretKey::from_bytes(&candidate.into()).is_err() {
        let mut h = Sha256::new();
        h.update(candidate);
        h.update(counter.to_le_bytes());
        candidate = h.finalize().into();
        counter += 1;
    }
    candidate
}

/// Prepare an on-chain registration payload with SSS threshold shares encrypted to designated accountability trustee or governance guardian public keys.
pub fn prepare_identity_registration(
    secret: &IdentitySecret,
    scope_id: &str,
    node_pubkeys: &[[u8; 32]],
    threshold_k: u8,
) -> Result<RegistrationPayload, SdkError> {
    let commitment = derive_identity_commitment(secret);
    let total_nodes = node_pubkeys.len() as u8;

    let shares = shamir::split_secret(secret.as_bytes(), threshold_k as u32, total_nodes as u32)?;

    let mut encrypted_shares = Vec::with_capacity(shares.len());

    for (i, share) in shares.iter().enumerate() {
        let ephemeral_sk = ecdh::generate_ephemeral_scalar();
        let ephemeral_pk = ecdh::derive_xonly_pubkey(&ephemeral_sk);

        let shared_secret = ecdh::compute_shared_secret(&ephemeral_sk, &node_pubkeys[i])?;

        let mut ciphertext = share.clone();
        ecdh::xor_encrypt(&mut ciphertext, &shared_secret, i as u32);

        encrypted_shares.push(EncryptedShare {
            node_pubkey: node_pubkeys[i],
            ephemeral_pk,
            ciphertext,
        });
    }

    Ok(RegistrationPayload {
        commitment,
        scope_id: scope_id.to_string(),
        encrypted_shares,
        threshold_k,
    })
}

/// Sign an attestation using the scoped credential.
pub fn sign_identity_attestation(
    secret: &IdentitySecret,
    scope_id: &str,
    message: &[u8],
) -> Result<AttestationPayload, SdkError> {
    let commitment = derive_identity_commitment(secret);
    let nullifier = derive_scoped_nullifier(secret, scope_id);
    let attestation_sk = derive_scope_attestation_key(secret, scope_id);

    // Sign message using BIP-340 Schnorr
    let signature = signature::sign_schnorr(&attestation_sk, message)?;

    Ok(AttestationPayload {
        commitment,
        scope_id: scope_id.to_string(),
        nullifier,
        message: message.to_vec(),
        signature: signature.to_vec(),
    })
}

/// Lightweight HMAC-SHA256 helper for HKDF extraction and expansion.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        let h: [u8; 32] = Sha256::digest(key).into();
        block[..32].copy_from_slice(&h);
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut k_ipad = [0x36u8; 64];
    let mut k_opad = [0x5cu8; 64];
    for i in 0..64 {
        k_ipad[i] ^= block[i];
        k_opad[i] ^= block[i];
    }

    let mut inner = Sha256::new();
    inner.update(k_ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(k_opad);
    outer.update(inner_hash);
    outer.finalize().into()
}
