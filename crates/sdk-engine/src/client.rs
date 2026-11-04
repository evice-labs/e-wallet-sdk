//! Resilient sequencer client wrapper that provides backward compatibility
//! between the SDK and testnet sequencers returning legacy response formats.

use std::collections::BTreeMap;
use common::{HashType, PageLimit, block::Block, transaction::LeeTransaction};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use lee::{Account, AccountId, Actor, EncryptedNote, ProgramId, Publication};
use lee_core::{
    BlockId, Commitment, CommitmentPosition, CommitmentSetDigest, MembershipProof, account::Nonce,
};
use sequencer_service_rpc::{ClientError, FeeStateQuote, RpcClient, SequencerClient};
use wallet::WalletSequencerClient;

/// Sequencer client wrapper that adapts legacy testnet RPC formats (`shards` field, 2-tuple proofs)
/// to the modern Actor-model wallet traits seamlessly.
#[derive(Clone)]
pub struct TestnetSequencerClient {
    inner: SequencerClient,
}

impl TestnetSequencerClient {
    pub fn new(inner: SequencerClient) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &SequencerClient {
        &self.inner
    }
}

impl WalletSequencerClient for TestnetSequencerClient {
    type Error = ClientError;

    async fn send_transaction(&self, tx: LeeTransaction) -> Result<HashType, Self::Error> {
        RpcClient::send_transaction(&self.inner, tx).await
    }

    async fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, Self::Error> {
        RpcClient::get_block(&self.inner, block_id).await
    }

    async fn get_block_range(
        &self,
        start_block_id: BlockId,
        end_block_id: BlockId,
    ) -> Result<Vec<Block>, Self::Error> {
        RpcClient::get_block_range(&self.inner, start_block_id, end_block_id).await
    }

    async fn get_last_block_id(&self) -> Result<BlockId, Self::Error> {
        RpcClient::get_last_block_id(&self.inner).await
    }

    async fn get_account_balance(&self, account_id: AccountId) -> Result<u128, Self::Error> {
        RpcClient::get_account_balance(&self.inner, account_id).await
    }

    async fn get_transaction(
        &self,
        tx_hash: HashType,
    ) -> Result<Option<(LeeTransaction, BlockId)>, Self::Error> {
        RpcClient::get_transaction(&self.inner, tx_hash).await
    }

    async fn get_accounts_nonces(
        &self,
        account_ids: Vec<AccountId>,
    ) -> Result<Vec<Nonce>, Self::Error> {
        RpcClient::get_accounts_nonces(&self.inner, account_ids).await
    }

    async fn get_account_view(
        &self,
        actor_state_selector: Actor,
    ) -> Result<Option<Account>, Self::Error> {
        let val: Option<serde_json::Value> = self
            .inner
            .request("getAccountView", rpc_params![actor_state_selector])
            .await?;
        match val {
            Some(mut v) => {
                normalize_account_json(&mut v);
                let acc: Account = serde_json::from_value(v).map_err(|e| {
                    ClientError::Custom(format!("getAccountView parse error: {}", e))
                })?;
                Ok(Some(acc))
            }
            None => Ok(None),
        }
    }

    async fn get_proofs_and_root(
        &self,
        commitments: Vec<Commitment>,
        message_positions: Vec<CommitmentPosition>,
    ) -> Result<
        (
            Vec<Option<MembershipProof>>,
            Vec<Option<MembershipProof>>,
            CommitmentSetDigest,
        ),
        Self::Error,
    > {
        let val: serde_json::Value = self
            .inner
            .request(
                "getProofsAndRoot",
                rpc_params![commitments, message_positions],
            )
            .await?;
        if let Some(arr) = val.as_array() {
            if arr.len() == 2 {
                // Testnet sequencer returns 2-tuple: (proofs, root)
                let proofs: Vec<Option<MembershipProof>> = serde_json::from_value(arr[0].clone())
                    .map_err(|e| {
                    ClientError::Custom(format!("getProofsAndRoot proofs error: {}", e))
                })?;
                let root: CommitmentSetDigest =
                    serde_json::from_value(arr[1].clone()).map_err(|e| {
                        ClientError::Custom(format!("getProofsAndRoot root error: {}", e))
                    })?;
                return Ok((proofs, Vec::new(), root));
            } else if arr.len() == 3 {
                // Updated sequencer returns 3-tuple: (proofs, message_paths, root)
                let proofs: Vec<Option<MembershipProof>> = serde_json::from_value(arr[0].clone())
                    .map_err(|e| {
                    ClientError::Custom(format!("getProofsAndRoot proofs error: {}", e))
                })?;
                let msg_paths: Vec<Option<MembershipProof>> =
                    serde_json::from_value(arr[1].clone()).map_err(|e| {
                        ClientError::Custom(format!("getProofsAndRoot message_paths error: {}", e))
                    })?;
                let root: CommitmentSetDigest =
                    serde_json::from_value(arr[2].clone()).map_err(|e| {
                        ClientError::Custom(format!("getProofsAndRoot root error: {}", e))
                    })?;
                return Ok((proofs, msg_paths, root));
            }
        }
        Err(ClientError::Custom(
            "Unexpected getProofsAndRoot response shape from sequencer".to_string(),
        ))
    }

    async fn get_fee_state(&self) -> Result<FeeStateQuote, Self::Error> {
        RpcClient::get_fee_state(&self.inner).await
    }

    async fn get_account(&self, account_id: AccountId) -> Result<Option<Account>, Self::Error> {
        let val: Option<serde_json::Value> = self
            .inner
            .request("getAccount", rpc_params![account_id])
            .await?;
        match val {
            Some(mut v) => {
                normalize_account_json(&mut v);
                let acc: Account = serde_json::from_value(v)
                    .map_err(|e| ClientError::Custom(format!("getAccount parse error: {}", e)))?;
                Ok(Some(acc))
            }
            None => Ok(None),
        }
    }

    async fn get_program_ids(&self) -> Result<BTreeMap<String, ProgramId>, Self::Error> {
        RpcClient::get_program_ids(&self.inner).await
    }

    async fn get_publications(
        &self,
        from_position: CommitmentPosition,
        limit: PageLimit,
    ) -> Result<Vec<(CommitmentPosition, Publication)>, Self::Error> {
        RpcClient::get_publications(&self.inner, from_position, limit).await
    }

    async fn get_recovery_binding(
        &self,
        address: AccountId,
    ) -> Result<Option<EncryptedNote>, Self::Error> {
        RpcClient::get_recovery_binding(&self.inner, address).await
    }
}

fn normalize_account_json(val: &mut serde_json::Value) {
    if let Some(obj) = val.get_mut("data").and_then(|d| d.as_object_mut())
        && let Some(shards) = obj.remove("shards")
    {
        obj.insert("actor_states".to_string(), shards);
    }
}
