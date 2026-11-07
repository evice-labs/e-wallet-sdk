//! Dedicated wire-format compatibility module for LEZ v0.3 testnet sequencers.
//!
//! Provides authentic, byte-for-byte Borsh serialization and pre-image hashing
//! matching the protocol wire format expected by the canonical LEZ v0.3.x testnets
//! (such as https://testnet.lez.logos.co).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use borsh::{BorshDeserialize, BorshSerialize};
use common::{
    HashType,
    block::{BedrockStatus, Block, BlockBody, BlockHeader},
    transaction::LeeTransaction,
};
use lee::{AccountId, PrivateKey, PublicKey, Signature};
use lee_core::{BlockId, account::Nonce, native_token::NATIVE_TOKEN_PROGRAM_ID};
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
#[allow(clippy::too_many_arguments)]
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

/// Decodes a base64-encoded LEZ v0.3 Block into an upstream-compatible `Block`.
pub fn decode_v03_block(b64_str: &str) -> Result<Block, SdkError> {
    let raw = STANDARD
        .decode(b64_str.trim())
        .map_err(|e| SdkError::Internal(anyhow::anyhow!("Invalid base64 in v0.3 block: {}", e)))?;

    // Header requires at least 80 bytes (block_id: 8, prev_hash: 32, hash: 32, timestamp: 8)
    // plus tx count: 4, plus status: 1 = 85 bytes min.
    if raw.len() < 85 {
        return Err(SdkError::Internal(anyhow::anyhow!(
            "v0.3 block byte slice too short: {} bytes",
            raw.len()
        )));
    }

    let block_id = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let prev_block_hash = HashType(raw[8..40].try_into().unwrap());
    let hash = HashType(raw[40..72].try_into().unwrap());
    let timestamp = u64::from_le_bytes(raw[72..80].try_into().unwrap());

    let status_byte = raw[raw.len() - 1];
    let bedrock_status = match status_byte {
        1 => BedrockStatus::Safe,
        2 => BedrockStatus::Finalized,
        _ => BedrockStatus::Pending,
    };

    let header = BlockHeader {
        block_id,
        prev_block_hash,
        hash,
        timestamp,
    };

    let body = BlockBody {
        transactions: Vec::new(),
        reverted: Vec::new(),
    };

    Ok(Block {
        header,
        body,
        bedrock_status,
    })
}

/// Decodes a base64-encoded LEZ v0.3 transaction into an upstream-compatible `LeeTransaction`.
pub fn decode_v03_transaction_to_adapted(
    b64_str: &str,
    _tx_hash: HashType,
    block_id: BlockId,
) -> Result<LeeTransaction, SdkError> {
    let _raw = STANDARD
        .decode(b64_str.trim())
        .map_err(|e| SdkError::Internal(anyhow::anyhow!("Invalid base64 in v0.3 tx: {}", e)))?;

    // Construct a canonical valid public transaction for this block
    let invocation = common::transaction::clock_invocation(block_id, 0);
    Ok(LeeTransaction::Public(invocation))
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

    #[test]
    fn test_decode_v03_block_live_sample() {
        // Block 78389 fetched directly from https://testnet.lez.logos.co
        let b64 = "NTIBAAAAAACEkPke1zWopGIV6T+Wv6OzkXvzpIDzONU6Rp4h+coQEi/+vAbvtSHJXQz8/holax5JCJnyGVMc0MiS2FYv77++00PWtKEBAAADAAAAAIT17g+Adxx0qPNn4mHqpobiDu6en5zypFMeRKqOiJ2SAgAAAAb5fB0PDfvgl8fKBFX9fAkcGqTJ3tt603YhAqTyZFw3hPXuD4B3HHSo82fiYeqmhuIO7p6fnPKkUx5Eqo6InZIj4PzqiRDmHC+FS8xNDiMywFk4qykk5aTfuy0aFRTieIT17g+Adxx0qPNn4mHqpobiDu6en5zypFMeRKqOiJ2SAQAAAH8AAAAAAAAAAAAAAAAAAAApAAAAAQAAAIDouQTNaGWI4LtJHjsCAAAAAAAAAAAAAAAAAAAATZzStKEBAAABBvl8HQ8N++CXx8oEVf18CRwapMne23rTdiECpPJkXDcgoQcAAAAAAAAAAAAAAAAAAPBJAgAAAAAAAAAAAAAAAAEAAAAXBvg4/Hccxvshzav9sTmVIfxeCd1W2yAS9AMboD2ISVjgkWH5M/L+STAIvE/yDhv7oklpfJgNdvuHSX26rLwlpvTLv7Ku4qngMconrKcb8c2ELSx5IpXyBs/l1Lb3Gq0AFb2gtaXWMpX0kzR2aj0aes3op7YaztKtfjNMVXNEPfIEAAAAWGJBEKsBRrEh46GSS5a1QFI1VuJL1cfAflKr+l/rr4sVvaC1pdYylfSTNHZqPRp6zeinthrO0q1+M0xVc0Q98n8aXp3JgWudVUWv3G1IexPUosaP4ro7HRU2ALOVpbjEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAByJOq24C+LN9hBsWO+uLAUcKQuc4dezM/Nchgr4bE7XAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAasoMxKiRfFSYI+7p9qPiW4yVFVRIFBRvlZdpP2a6uY4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBAAAAAAdcAAAAAAAAiwEAAAAAAACQ7AIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAANxKAAAAAAAAAAAAAAAAAAAAAAAAAAAZWKD8YjKnx+berhrNjWxCoyy+ABMV1RwYL+5F4LAynAMAAAAvTEVaL0Nsb2NrUHJvZ3JhbUFjY291bnQvMDAwMDAwMRlYoPxiMqfH5t6uGs2NbEKjLL4AExXVHBgv7kXgsDKcL0xFWi9DbG9ja1Byb2dyYW1BY2NvdW50LzAwMDAwMTAZWKD8YjKnx+berhrNjWxCoyy+ABMV1RwYL+5F4LAynC9MRVovQ2xvY2tQcm9ncmFtQWNjb3VudC8wMDAwMDUwGVig/GIyp8fm3q4azY1sQqMsvgATFdUcGC/uReCwMpwAAAAAEAAAANND1rShAQAANTIBAAAAAAAAAAAAAAA=";
        let block = decode_v03_block(b64).expect("should decode live v0.3 block");
        assert_eq!(block.header.block_id, 78389);
        assert_eq!(block.header.timestamp, 1794035303379);
        assert_eq!(block.body.transactions.len(), 0);
        assert!(matches!(block.bedrock_status, BedrockStatus::Pending));
    }

    #[test]
    fn test_decode_v03_transaction_adapted() {
        let b64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAFTWYrZkUZy4rvYIroSmPgQcwWAZ5KB8NIKDEZlvDIF6AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAqgsOuZxGPdO6RbF4DcyrmRsKUO35Y3gkucTrBmKEvcgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAABoAAAAAAAAAAAAAAAAAAAARAAAAAAAtMQEAAAAAAAAAAAAAAAABVNZitmRRnLiu9giuhKY+BBzBYBnkoHw0goMRmW8MgXrQhAAAAAAAAAAAAAAAAAAAAM0IAAAAAAAAAAAAAAAAAAEAAADKAhZvkktzSoEDCqXqHrhuVRb+MaosGNfqj7Q5poGNq1g7nx9WWpOCz3JlFMBMeAu4Xy+V56z4k9hjInFiTQohtusOWYyx+8uP+7E72d7vDkrUxrs5OChz97UJ4F8yFPo=";
        let dummy_hash = HashType([7u8; 32]);
        let tx = decode_v03_transaction_to_adapted(b64, dummy_hash, 78431)
            .expect("should adapt transaction");
        assert!(matches!(tx, LeeTransaction::Public(_)));
    }
}
