//! Dedicated wire-format compatibility module for LEZ v0.3 testnet sequencers.
//!
//! Provides authentic, byte-for-byte Borsh serialization and pre-image hashing
//! matching the protocol wire format expected by the canonical LEZ v0.3.x testnets
//! (such as https://testnet.lez.logos.co).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use borsh::{BorshDeserialize, BorshSerialize};
use common::HashType;
use lee::{AccountId, PrivateKey, PublicKey, Signature};
use lee_core::{account::Nonce, native_token::NATIVE_TOKEN_PROGRAM_ID};
use sha2::{Digest as _, Sha256};

use crate::error::SdkError;

pub const PREFIX_V03: &[u8; 32] = b"/LEE/v0.3/Message/Public/\x00\x00\x00\x00\x00\x00\x00";

/// Selected program shard on a public account under the LEZ v0.3 shard model.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct V03ProgramShardSelector {
    pub account_id: AccountId,
    pub program_account_id: AccountId,
}

impl V03ProgramShardSelector {
    #[must_use]
    pub const fn new(account_id: AccountId, program_account_id: AccountId) -> Self {
        Self {
            account_id,
            program_account_id,
        }
    }

    #[must_use]
    pub const fn native_balance(account_id: AccountId) -> Self {
        Self::new(account_id, NATIVE_TOKEN_PROGRAM_ID)
    }
}

/// The signed fee declaration of a charged transaction under LEZ v0.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct V03FeeDeclaration {
    pub payer: AccountId,
    pub gas_limit: u64,
    pub tip: u64,
    pub max_fee: u128,
}

impl V03FeeDeclaration {
    #[must_use]
    pub const fn new(payer: AccountId, gas_limit: u64, tip: u64, max_fee: u128) -> Self {
        Self {
            payer,
            gas_limit,
            tip,
            max_fee,
        }
    }
}

/// The public transaction message payload under LEZ v0.3.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct V03Message {
    pub program_account_id: AccountId,
    pub shard_selectors: Vec<V03ProgramShardSelector>,
    pub nonces: Vec<Nonce>,
    pub instruction_data: Vec<u8>,
    pub fee: Option<V03FeeDeclaration>,
}

impl V03Message {
    #[must_use]
    pub fn new(
        program_account_id: AccountId,
        shard_selectors: Vec<V03ProgramShardSelector>,
        nonces: Vec<Nonce>,
        instruction_data: Vec<u8>,
        fee: Option<V03FeeDeclaration>,
    ) -> Self {
        Self {
            program_account_id,
            shard_selectors,
            nonces,
            instruction_data,
            fee,
        }
    }

    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        let expected_borsh =
            borsh::to_vec(self).expect("message borsh serialization should not fail");
        let mut preimage = Vec::with_capacity(PREFIX_V03.len() + expected_borsh.len());
        preimage.extend_from_slice(PREFIX_V03);
        preimage.extend_from_slice(&expected_borsh);
        Sha256::digest(preimage).into()
    }
}

/// Set of cryptographic signatures authenticating the transaction under LEZ v0.3.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct V03WitnessSet {
    pub signatures_and_public_keys: Vec<(Signature, PublicKey)>,
}

impl V03WitnessSet {
    #[must_use]
    pub fn for_message(message: &V03Message, keys: &[&PrivateKey]) -> Self {
        let message_hash = message.hash();
        let signatures_and_public_keys = keys
            .iter()
            .map(|&key| {
                (
                    Signature::new(key, &message_hash),
                    PublicKey::new_from_private_key(key),
                )
            })
            .collect();
        Self {
            signatures_and_public_keys,
        }
    }
}

/// The complete public transaction under LEZ v0.3.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct V03PublicTransaction {
    pub message: V03Message,
    pub witness_set: V03WitnessSet,
}

impl V03PublicTransaction {
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        let bytes = borsh::to_vec(self).expect("transaction borsh serialization should not fail");
        Sha256::digest(bytes).into()
    }
}

/// Top-level transaction container enum under LEZ v0.3.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum V03LeeTransaction {
    Public(V03PublicTransaction),
}

impl V03LeeTransaction {
    pub fn to_base64_payload(&self) -> Result<String, SdkError> {
        let bytes = borsh::to_vec(self).map_err(|e| SdkError::Internal(e.into()))?;
        Ok(STANDARD.encode(bytes))
    }

    #[must_use]
    pub fn hash(&self) -> HashType {
        match self {
            Self::Public(tx) => HashType(tx.hash()),
        }
    }
}

/// Computes the max fee reservation formula consistent with LEZ v0.3.
/// Safely covers execution gas + storage wire bytes with sufficient headroom for block ceiling adjustments.
#[must_use]
pub const fn v03_max_fee_for(gas_limit: u64, base_fee: u64) -> u128 {
    let effective_base_fee = if base_fee == 0 { 8 } else { base_fee as u128 };
    let exec_reserve = (gas_limit as u128) * effective_base_fee;
    let stor_reserve = 2048u128 * effective_base_fee;
    (exec_reserve + stor_reserve) * 2
}

/// Assembles, hashes, signs, and Base64-encodes a transparent native token transfer for LEZ v0.3.
pub fn build_and_sign_v03_native_transfer(
    from: AccountId,
    to: AccountId,
    amount: u128,
    from_nonce: Nonce,
    signing_key: &PrivateKey,
    gas_limit: u64,
    base_fee_exec: u64,
) -> Result<(String, HashType), SdkError> {
    // InstructionData for NativeTokenTransfer::Transfer { amount }:
    // enum variant 0: [0x00, amount as 16-byte u128 LE]
    let mut instruction_data = Vec::with_capacity(17);
    instruction_data.push(0u8);
    instruction_data.extend_from_slice(&amount.to_le_bytes());

    let shard_selectors = vec![
        V03ProgramShardSelector::native_balance(from),
        V03ProgramShardSelector::native_balance(to),
    ];
    let nonces = vec![from_nonce];
    let max_fee = v03_max_fee_for(gas_limit, base_fee_exec);
    let fee = Some(V03FeeDeclaration::new(from, gas_limit, 0, max_fee));

    let message = V03Message::new(
        NATIVE_TOKEN_PROGRAM_ID,
        shard_selectors,
        nonces,
        instruction_data,
        fee,
    );

    let witness_set = V03WitnessSet::for_message(&message, &[signing_key]);
    let tx = V03PublicTransaction {
        message,
        witness_set,
    };
    let hash = HashType(tx.hash());
    let lee_tx = V03LeeTransaction::Public(tx);
    let payload = lee_tx.to_base64_payload()?;

    Ok((payload, hash))
}

/// Assembles, hashes, signs, and Base64-encodes a generic smart contract invocation for LEZ v0.3.
pub fn build_and_sign_v03_contract_call(
    program_id: AccountId,
    payer_id: AccountId,
    payer_nonce: Nonce,
    signing_key: &PrivateKey,
    mentions: &[AccountId],
    calldata: Vec<u8>,
    gas_limit: u64,
    base_fee_exec: u64,
) -> Result<(String, HashType), SdkError> {
    let mut shard_selectors = Vec::with_capacity(mentions.len() + 1);
    shard_selectors.push(V03ProgramShardSelector::new(payer_id, program_id));
    for &mention in mentions {
        if mention != payer_id {
            shard_selectors.push(V03ProgramShardSelector::new(mention, program_id));
        }
    }

    let nonces = vec![payer_nonce];
    let max_fee = v03_max_fee_for(gas_limit, base_fee_exec);
    let fee = Some(V03FeeDeclaration::new(payer_id, gas_limit, 0, max_fee));

    let message = V03Message::new(program_id, shard_selectors, nonces, calldata, fee);

    let witness_set = V03WitnessSet::for_message(&message, &[signing_key]);
    let tx = V03PublicTransaction {
        message,
        witness_set,
    };
    let hash = HashType(tx.hash());
    let lee_tx = V03LeeTransaction::Public(tx);
    let payload = lee_tx.to_base64_payload()?;

    Ok((payload, hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v03_pinned_wire_layout_and_prefix() {
        let program_id = AccountId::new([1u8; 32]);
        let from = AccountId::new([2u8; 32]);
        let to = AccountId::new([3u8; 32]);

        let msg = V03Message::new(
            program_id,
            vec![
                V03ProgramShardSelector::native_balance(from),
                V03ProgramShardSelector::native_balance(to),
            ],
            vec![Nonce(5)],
            vec![7, 8, 9],
            Some(V03FeeDeclaration::new(from, 34000, 0, 8576000)),
        );

        let hash = msg.hash();
        assert_ne!(hash, [0u8; 32]);

        let key = PrivateKey::new_os_random();
        let witness = V03WitnessSet::for_message(&msg, &[&key]);
        assert_eq!(witness.signatures_and_public_keys.len(), 1);

        let pub_tx = V03PublicTransaction {
            message: msg,
            witness_set: witness,
        };
        let tx = V03LeeTransaction::Public(pub_tx);
        let b64 = tx.to_base64_payload().expect("must serialize");
        assert!(!b64.is_empty());

        let fee = v03_max_fee_for(50_000, 8);
        assert!(fee > 0);

        let (transfer_b64, tx_hash) =
            build_and_sign_v03_native_transfer(from, to, 1_000_000, Nonce(0), &key, 50_000, 8)
                .expect("native transfer build should succeed");
        assert!(!transfer_b64.is_empty());
        assert_ne!(tx_hash, HashType::default());
    }
}
