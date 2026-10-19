//! # Identity Accountability & Accountable Pseudonymity Module.
//!
//! Provides native accountable pseudonymity, zero-knowledge identity commitments,
//! scope-specific nullifiers, and Two-Tier Shamir Secret Sharing (SSS) over GF(2^8).
//!
//! Adapted from the Evice Labs `e-identity-stack` research repository for the Logos Execution Zone.

pub mod derivation;
pub mod ecdh;
pub mod shamir;
pub mod signature;
pub mod types;

pub use derivation::{
    derive_identity_commitment, derive_identity_secret, derive_scope_attestation_key,
    derive_scoped_nullifier, prepare_identity_registration, sign_identity_attestation,
};
pub use ecdh::{
    compute_shared_secret, derive_xonly_pubkey, generate_ephemeral_scalar, xor_encrypt,
};
pub use shamir::{recover_secret, split_secret};
pub use signature::{sign_schnorr, verify_schnorr};
pub use types::{
    AttestationPayload, EncryptedShare, IdentityCommitment, IdentitySecret, RegistrationPayload,
    ScopedNullifier,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_derivation_deterministic() {
        let root_seed = b"test_seed_phrase_for_bip39_deterministic_restoration_1234567890";
        let secret1 = derive_identity_secret(root_seed);
        let secret2 = derive_identity_secret(root_seed);
        assert_eq!(secret1.as_bytes(), secret2.as_bytes());

        let comm1 = derive_identity_commitment(&secret1);
        let comm2 = derive_identity_commitment(&secret2);
        assert_eq!(comm1, comm2);
    }

    #[test]
    fn test_scoped_nullifier_unlinkability() {
        let root_seed = b"test_seed_for_cross_scope_unlinkability_experiment";
        let secret = derive_identity_secret(root_seed);

        let nullifier_forum = derive_scoped_nullifier(&secret, "governance_forum");
        let nullifier_vote = derive_scoped_nullifier(&secret, "dao_voting_room");
        let nullifier_forum_dup = derive_scoped_nullifier(&secret, "governance_forum");

        // Same scope -> stable pseudonym
        assert_eq!(nullifier_forum, nullifier_forum_dup);

        // Different scopes -> cryptographically uncorrelated
        assert_ne!(nullifier_forum, nullifier_vote);
    }

    #[test]
    fn test_attestation_signing_and_verification() {
        let root_seed = b"seed_for_attestation_signature_test";
        let secret = derive_identity_secret(root_seed);
        let scope = "dao_proposal_42";
        let message = b"I vote YES on proposal 42";

        let attestation =
            sign_identity_attestation(&secret, scope, message).expect("Signing attestation failed");

        let attestation_sk = derive_scope_attestation_key(&secret, scope);
        let attestation_pk = derive_xonly_pubkey(&attestation_sk);

        let valid = verify_schnorr(&attestation_pk, message, &attestation.signature);
        assert!(valid, "Signature verification must succeed");

        let invalid = verify_schnorr(&attestation_pk, b"tampered message", &attestation.signature);
        assert!(!invalid, "Tampered message must fail verification");
    }

    #[test]
    fn test_registration_payload_and_recovery() {
        let root_seed = b"master_seed_for_full_registration_lifecycle";
        let secret = derive_identity_secret(root_seed);

        // Setup 3 accountability trustee nodes
        let node1_sk = generate_ephemeral_scalar();
        let node1_pk = derive_xonly_pubkey(&node1_sk);
        let node2_sk = generate_ephemeral_scalar();
        let node2_pk = derive_xonly_pubkey(&node2_sk);
        let node3_sk = generate_ephemeral_scalar();
        let node3_pk = derive_xonly_pubkey(&node3_sk);

        let nodes = vec![node1_pk, node2_pk, node3_pk];
        let k_threshold = 2;

        let payload =
            prepare_identity_registration(&secret, "governance_registry", &nodes, k_threshold)
                .expect("Registration preparation failed");

        assert_eq!(payload.encrypted_shares.len(), 3);
        assert_eq!(payload.threshold_k, 2);

        // Simulate Trustee 1 and Trustee 2 decrypting their shares upon verified governance consensus
        let s1 = &payload.encrypted_shares[0];
        let ss1 = compute_shared_secret(&node1_sk, &s1.ephemeral_pk).expect("ECDH 1 failed");
        let mut share1 = s1.ciphertext.clone();
        xor_encrypt(&mut share1, &ss1, 0);

        let s2 = &payload.encrypted_shares[1];
        let ss2 = compute_shared_secret(&node2_sk, &s2.ephemeral_pk).expect("ECDH 2 failed");
        let mut share2 = s2.ciphertext.clone();
        xor_encrypt(&mut share2, &ss2, 1);

        // Recover secret with K=2 shares
        let decrypted_shares = vec![share1, share2];
        let recovered =
            recover_secret(&decrypted_shares, k_threshold as u32).expect("Secret recovery failed");

        assert_eq!(recovered, *secret.as_bytes());
    }
}
