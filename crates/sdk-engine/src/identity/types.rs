use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// 32-byte master identity secret derived deterministically from the root seed phrase.
///
/// Implements ZeroizeOnDrop to ensure sensitive key material is securely wiped from memory.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct IdentitySecret(pub [u8; 32]);

impl std::fmt::Debug for IdentitySecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IdentitySecret([REDACTED])")
    }
}

impl IdentitySecret {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// 32-byte public identity commitment: `SHA-256("lez/identity/v1/commitment" || IdentitySecret)`.
///
/// Can be safely published on-chain or shared publicly without compromising identity secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdentityCommitment(pub [u8; 32]);

impl IdentityCommitment {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for IdentityCommitment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

/// 32-byte scope-specific pseudonymous nullifier: `SHA-256("lez/identity/v1/scoped_nullifier" || IdentitySecret || ScopeId)`.
///
/// Guarantees that actions within the same scope are linkable to a stable pseudonym,
/// while actions across different scopes remain cryptographically uncorrelated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopedNullifier(pub [u8; 32]);

impl ScopedNullifier {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for ScopedNullifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

/// A Shamir Secret Sharing (SSS) share encrypted using ECDH to a designated accountability trustee or governance guardian.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedShare {
    /// Target trustee or guardian node's 32-byte x-only public key.
    pub node_pubkey: [u8; 32],
    /// Ephemeral 32-byte x-only public key used for ECDH key agreement.
    pub ephemeral_pk: [u8; 32],
    /// Encrypted share bytes (X-coordinate index + evaluated share bytes).
    pub ciphertext: Vec<u8>,
}

/// On-chain registration payload for accountable pseudonymous governance and community membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationPayload {
    /// User's public identity commitment.
    pub commitment: IdentityCommitment,
    /// Target governance scope or registry identifier.
    pub scope_id: String,
    /// Encrypted Shamir Secret Sharing shares distributed to designated accountability trustees.
    pub encrypted_shares: Vec<EncryptedShare>,
    /// Minimum threshold K of verified trustee attestations required to reconstruct the revocable identity secret.
    pub threshold_k: u8,
}

/// Signed attestation payload verifying participation within a specific scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationPayload {
    /// User's public identity commitment.
    pub commitment: IdentityCommitment,
    /// Target governance scope or registry identifier.
    pub scope_id: String,
    /// Scoped pseudonym / nullifier for this context.
    pub nullifier: ScopedNullifier,
    /// Arbitrary message payload (e.g. vote ballot or credential claim).
    pub message: Vec<u8>,
    /// 64-byte BIP-340 Schnorr signature over the message.
    pub signature: Vec<u8>,
}
