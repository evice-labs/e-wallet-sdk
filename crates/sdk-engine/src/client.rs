//! Resilient sequencer client wrapper that provides backward compatibility
//! between the SDK and testnet sequencers returning legacy response formats.

use common::{HashType, PageLimit, block::Block, transaction::LeeTransaction};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use lee::{Account, AccountId, Actor, EncryptedNote, ProgramId, Publication};
use lee_core::{
    BlockId, Commitment, CommitmentPosition, CommitmentSetDigest, MembershipProof, account::Nonce,
};
use sequencer_service_rpc::{ClientError, FeeStateQuote, RpcClient, SequencerClient};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wallet::WalletSequencerClient;

/// The protocol version detected from the sequencer RPC response schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequencerProtocolVersion {
    V03ShardModel,
    V04ActorModel,
}

/// Sequencer client wrapper that adapts legacy testnet RPC formats (`shards` field, 2-tuple proofs)
/// to the modern Actor-model wallet traits seamlessly, with adaptive protocol version detection.
#[derive(Clone)]
pub struct TestnetSequencerClient {
    inner: SequencerClient,
    protocol_version: Arc<RwLock<Option<SequencerProtocolVersion>>>,
}

impl TestnetSequencerClient {
    pub fn new(inner: SequencerClient) -> Self {
        Self {
            inner,
            protocol_version: Arc::new(RwLock::new(None)),
        }
    }

    pub fn inner(&self) -> &SequencerClient {
        &self.inner
    }

    /// Dynamically detects whether the connected sequencer speaks the LEZ v0.3 (shard model)
    /// or LEZ v0.4 (actor model) protocol by inspecting the schema of account states.
    pub async fn detect_protocol_version(&self) -> SequencerProtocolVersion {
        {
            let lock = self.protocol_version.read().await;
            if let Some(version) = *lock {
                return version;
            }
        }

        let probe_target = AccountId::default();
        let version = match self
            .inner
            .request::<Option<serde_json::Value>, _>("getAccount", rpc_params![probe_target])
            .await
        {
            Ok(Some(v)) => {
                if v.get("data")
                    .and_then(|d| d.as_object())
                    .map_or(false, |o| o.contains_key("shards"))
                {
                    SequencerProtocolVersion::V03ShardModel
                } else {
                    SequencerProtocolVersion::V04ActorModel
                }
            }
            Ok(None) => SequencerProtocolVersion::V03ShardModel,
            Err(_) => SequencerProtocolVersion::V03ShardModel,
        };

        let mut lock = self.protocol_version.write().await;
        *lock = Some(version);
        version
    }

    /// Dispatches a raw, pre-serialized base64 Borsh transaction payload directly to
    /// the sequencer's `sendTransaction` JSON-RPC endpoint.
    pub async fn send_raw_transaction(
        &self,
        base64_payload: String,
    ) -> Result<HashType, ClientError> {
        self.inner
            .request("sendTransaction", rpc_params![base64_payload])
            .await
            .map_err(Self::format_client_error)
    }

    pub fn format_client_error(err: ClientError) -> ClientError {
        match err {
            ClientError::Call(ref err_obj) => {
                let msg = if let Some(data) = err_obj.data() {
                    format!("{}: {}", err_obj.message(), data)
                } else {
                    err_obj.message().to_string()
                };
                ClientError::Custom(msg)
            }
            other => other,
        }
    }
}

impl WalletSequencerClient for TestnetSequencerClient {
    type Error = ClientError;

    async fn send_transaction(&self, tx: LeeTransaction) -> Result<HashType, Self::Error> {
        RpcClient::send_transaction(&self.inner, tx)
            .await
            .map_err(Self::format_client_error)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_legacy_account_shards() {
        let mut val = serde_json::json!({
            "nonce": 42,
            "data": {
                "shards": {
                    "some_program": 100
                }
            }
        });
        normalize_account_json(&mut val);
        assert!(val["data"].get("actor_states").is_some());
        assert!(val["data"].get("shards").is_none());
    }

    #[test]
    fn test_format_client_error_preserves_error_data() {
        let err_obj = jsonrpsee::types::ErrorObject::owned(
            -32602,
            "Invalid params",
            Some("Unexpected length of input"),
        );
        let client_err = ClientError::Call(err_obj);
        let formatted = TestnetSequencerClient::format_client_error(client_err);
        match formatted {
            ClientError::Custom(msg) => {
                assert!(msg.contains("Invalid params"));
                assert!(msg.contains("Unexpected length of input"));
            }
            _ => panic!("Expected ClientError::Custom"),
        }
    }

    #[tokio::test]
    async fn test_live_sequencer_protocol_detection() {
        let url = "https://testnet.lez.logos.co";
        if let Ok(inner) = jsonrpsee::http_client::HttpClientBuilder::default().build(url) {
            let client = TestnetSequencerClient::new(inner);
            let version = client.detect_protocol_version().await;
            assert_eq!(version, SequencerProtocolVersion::V03ShardModel);
        }
    }

    #[tokio::test]
    async fn test_live_send_raw_v03_transaction_format_validation() {
        let url = "https://testnet.lez.logos.co";
        if let Ok(inner) = jsonrpsee::http_client::HttpClientBuilder::default().build(url) {
            let client = TestnetSequencerClient::new(inner);
            let key = lee::PrivateKey::new_os_random();
            let pubkey = lee::PublicKey::new_from_private_key(&key);
            let from = lee::AccountId::from(&pubkey);
            let to = lee::AccountId::new([5u8; 32]);
            let (payload, _hash) = crate::v03_compat::build_and_sign_v03_native_transfer(
                from,
                to,
                1000,
                lee_core::account::Nonce(0),
                &key,
                200_000,
                8,
            )
            .expect("build transfer");

            let res = client.send_raw_transaction(payload).await;
            if let Err(e) = res {
                let err_str = e.to_string();
                assert!(
                    !err_str.contains("Invalid params"),
                    "Must not fail with Invalid params, got: {}",
                    err_str
                );
            }
        }
    }
}
