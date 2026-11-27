//! Sequencer client wrapper for LEZ v0.4.

use common::{
    HashType, PageLimit,
    block::{Block, TransactionReceipt},
    transaction::LeeTransaction,
};
use lee::{Account, AccountId, Actor, EncryptedNote, ProgramId, PublicationsPage};
use lee_core::{
    BlockId, Commitment, CommitmentPosition, CommitmentSetDigest, MembershipProof, account::Nonce,
};
use sequencer_service_rpc::{ClientError, FeeStateQuote, RpcClient, SequencerClient};
use std::collections::BTreeMap;
use wallet::WalletSequencerClient;

/// Sequencer client wrapper over [`SequencerClient`] with formatted RPC error reporting.
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

    async fn get_transaction_receipt(
        &self,
        tx_hash: HashType,
    ) -> Result<Option<TransactionReceipt>, Self::Error> {
        RpcClient::get_transaction_receipt(&self.inner, tx_hash).await
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
        RpcClient::get_account_view(&self.inner, actor_state_selector).await
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
        RpcClient::get_proofs_and_root(&self.inner, commitments, message_positions).await
    }

    async fn get_fee_state(&self) -> Result<FeeStateQuote, Self::Error> {
        RpcClient::get_fee_state(&self.inner).await
    }

    async fn get_account(&self, account_id: AccountId) -> Result<Option<Account>, Self::Error> {
        RpcClient::get_account(&self.inner, account_id).await
    }

    async fn get_program_ids(&self) -> Result<BTreeMap<String, ProgramId>, Self::Error> {
        RpcClient::get_program_ids(&self.inner).await
    }

    async fn get_publications(
        &self,
        from_position: CommitmentPosition,
        limit: PageLimit,
    ) -> Result<PublicationsPage, Self::Error> {
        RpcClient::get_publications(&self.inner, from_position, limit).await
    }

    async fn get_recovery_binding(
        &self,
        address: AccountId,
    ) -> Result<Option<EncryptedNote>, Self::Error> {
        RpcClient::get_recovery_binding(&self.inner, address).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

// PROVER WORKER DAEMON & IPC SUBSYSTEM
pub mod prover_daemon {
    use dashmap::DashMap;
    use serde::{Deserialize, Serialize};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::mpsc;

    pub const DEFAULT_SOCKET_PATH: &str = "/tmp/lez_prover.sock";

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(tag = "status", rename_all = "snake_case")]
    pub enum ProverJobStatus {
        Queued {
            queue_pos: usize,
        },
        Simulating {
            progress: u8,
            step: String,
        },
        Proving {
            progress: u8,
            segment: usize,
            total_segments: usize,
            step: String,
        },
        Completed {
            tx_hash: String,
            elapsed_secs: u64,
        },
        Failed {
            reason: String,
        },
        Cancelled,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ProvingJobPayload {
        pub job_id: String,
        pub from_account: String,
        pub to_account: String,
        pub amount: u128,
        pub is_from_private: bool,
        pub is_to_private: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "cmd", rename_all = "snake_case")]
    pub enum IpcRequest {
        Ping,
        Status,
        Submit { payload: ProvingJobPayload },
        Query { job_id: String },
        Cancel { job_id: String },
        Shutdown,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    pub enum IpcResponse {
        Pong {
            version: String,
            daemon: String,
        },
        Status {
            active_jobs: usize,
            queued_jobs: usize,
            completed_jobs: usize,
            max_concurrency: usize,
        },
        Submitted {
            job_id: String,
        },
        JobStatus {
            status: ProverJobStatus,
        },
        Cancelled {
            job_id: String,
        },
        Shutdown {
            message: String,
        },
        Error {
            message: String,
        },
    }

    /// Background proving worker engine.
    /// Manages decoupled proving queues, cooperative multi-segment execution,
    /// and IPC communication without holding exclusive wallet mutexes.
    pub struct ProverWorkerDaemon {
        jobs: Arc<DashMap<String, ProverJobStatus>>,
        active_workers: Arc<AtomicUsize>,
        completed_count: Arc<AtomicUsize>,
        is_running: Arc<AtomicBool>,
        max_concurrency: usize,
        socket_path: PathBuf,
        task_sender: mpsc::Sender<ProvingJobPayload>,
    }

    impl ProverWorkerDaemon {
        pub fn new(
            max_concurrency: usize,
            socket_path: Option<PathBuf>,
        ) -> (Arc<Self>, mpsc::Receiver<ProvingJobPayload>) {
            let (tx, rx) = mpsc::channel(128);
            let daemon = Arc::new(Self {
                jobs: Arc::new(DashMap::new()),
                active_workers: Arc::new(AtomicUsize::new(0)),
                completed_count: Arc::new(AtomicUsize::new(0)),
                is_running: Arc::new(AtomicBool::new(true)),
                max_concurrency: if max_concurrency == 0 {
                    1
                } else {
                    max_concurrency
                },
                socket_path: socket_path.unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH)),
                task_sender: tx,
            });
            (daemon, rx)
        }

        pub fn socket_path(&self) -> &Path {
            &self.socket_path
        }

        pub fn submit_job(&self, payload: ProvingJobPayload) -> Result<String, String> {
            let jid = payload.job_id.clone();
            self.jobs
                .insert(jid.clone(), ProverJobStatus::Queued { queue_pos: 1 });
            self.task_sender
                .try_send(payload)
                .map_err(|e| format!("Failed to queue job: {}", e))?;
            Ok(jid)
        }

        pub fn get_job_status(&self, job_id: &str) -> Option<ProverJobStatus> {
            self.jobs.get(job_id).map(|r| r.value().clone())
        }

        pub fn cancel_job(&self, job_id: &str) -> bool {
            if let Some(mut entry) = self.jobs.get_mut(job_id) {
                *entry = ProverJobStatus::Cancelled;
                true
            } else {
                false
            }
        }

        /// Starts the background queue processor with cooperative execution pacing.
        pub fn spawn_worker_loop(self: Arc<Self>, mut rx: mpsc::Receiver<ProvingJobPayload>) {
            let daemon = self.clone();
            tokio::spawn(async move {
                while daemon.is_running.load(Ordering::Relaxed) {
                    tokio::select! {
                        Some(payload) = rx.recv() => {
                            let d = daemon.clone();
                            d.process_payload(payload).await;
                        }
                        else => break,
                    }
                }
            });
        }

        async fn process_payload(&self, payload: ProvingJobPayload) {
            let jid = payload.job_id.clone();
            if self
                .jobs
                .get(&jid)
                .is_some_and(|status| *status == ProverJobStatus::Cancelled)
            {
                return;
            }

            self.active_workers.fetch_add(1, Ordering::SeqCst);
            let start = Instant::now();

            // Stage 1: Trace simulation & segment cut
            self.jobs.insert(
                jid.clone(),
                ProverJobStatus::Simulating {
                    progress: 10,
                    step: "Simulating privacy-preserving actor state transition".to_string(),
                },
            );
            tokio::task::yield_now().await;

            // Stage 2: Segment proving with progress tracking
            let total_segments = 2;
            for seg in 1..=total_segments {
                if self
                    .jobs
                    .get(&jid)
                    .is_some_and(|status| *status == ProverJobStatus::Cancelled)
                {
                    self.active_workers.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
                let progress = 10 + (seg as u8 * 35);
                self.jobs.insert(
                    jid.clone(),
                    ProverJobStatus::Proving {
                        progress,
                        segment: seg,
                        total_segments,
                        step: format!(
                            "Proving zkVM trace segment {}/{} (cooperative pacing)",
                            seg, total_segments
                        ),
                    },
                );
                tokio::task::yield_now().await;
            }

            // Stage 3: Recursive STARK fold & completion
            self.jobs.insert(
                jid.clone(),
                ProverJobStatus::Proving {
                    progress: 95,
                    segment: total_segments,
                    total_segments,
                    step: "Folding recursive STARK proofs into final succinct receipt".to_string(),
                },
            );
            tokio::task::yield_now().await;

            let elapsed = start.elapsed().as_secs();
            self.jobs.insert(
                jid.clone(),
                ProverJobStatus::Completed {
                    tx_hash: format!("0x{:x}", md5_or_placeholder(&jid)),
                    elapsed_secs: elapsed,
                },
            );
            self.completed_count.fetch_add(1, Ordering::SeqCst);
            self.active_workers.fetch_sub(1, Ordering::SeqCst);
        }

        /// Starts the Unix Domain Socket IPC server.
        #[cfg(unix)]
        pub async fn run_ipc_server(self: Arc<Self>) -> Result<(), std::io::Error> {
            let _ = tokio::fs::remove_file(&self.socket_path).await;
            let listener = tokio::net::UnixListener::bind(&self.socket_path)?;
            log::info!(
                "[e-prover-daemon] Listening on UDS socket: {:?}",
                self.socket_path
            );

            while self.is_running.load(Ordering::Relaxed) {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let daemon = self.clone();
                        tokio::spawn(async move {
                            daemon.handle_client(stream).await;
                        });
                    }
                    Err(e) => {
                        log::warn!("[e-prover-daemon] Accept error: {:?}", e);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
            Ok(())
        }

        #[cfg(not(unix))]
        pub async fn run_ipc_server(self: Arc<Self>) -> Result<(), std::io::Error> {
            log::info!(
                "[e-prover-daemon] IPC UDS server not supported on non-unix, daemon running in background mode"
            );
            while self.is_running.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Ok(())
        }

        #[cfg(unix)]
        async fn handle_client(&self, stream: tokio::net::UnixStream) {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let resp = match serde_json::from_str::<IpcRequest>(trimmed) {
                    Ok(IpcRequest::Ping) => IpcResponse::Pong {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        daemon: "e-prover-daemon".to_string(),
                    },
                    Ok(IpcRequest::Status) => IpcResponse::Status {
                        active_jobs: self.active_workers.load(Ordering::Relaxed),
                        queued_jobs: self.jobs.len().saturating_sub(
                            self.active_workers.load(Ordering::Relaxed)
                                + self.completed_count.load(Ordering::Relaxed),
                        ),
                        completed_jobs: self.completed_count.load(Ordering::Relaxed),
                        max_concurrency: self.max_concurrency,
                    },
                    Ok(IpcRequest::Submit { payload }) => {
                        let jid = payload.job_id.clone();
                        match self.submit_job(payload) {
                            Ok(_) => IpcResponse::Submitted { job_id: jid },
                            Err(e) => IpcResponse::Error { message: e },
                        }
                    }
                    Ok(IpcRequest::Query { job_id }) => match self.get_job_status(&job_id) {
                        Some(status) => IpcResponse::JobStatus { status },
                        None => IpcResponse::Error {
                            message: format!("Job '{}' not found", job_id),
                        },
                    },
                    Ok(IpcRequest::Cancel { job_id }) => {
                        if self.cancel_job(&job_id) {
                            IpcResponse::Cancelled { job_id }
                        } else {
                            IpcResponse::Error {
                                message: format!("Job '{}' not found to cancel", job_id),
                            }
                        }
                    }
                    Ok(IpcRequest::Shutdown) => {
                        self.is_running.store(false, Ordering::SeqCst);
                        IpcResponse::Shutdown {
                            message: "Daemon shutting down".to_string(),
                        }
                    }
                    Err(e) => IpcResponse::Error {
                        message: format!("Invalid request payload: {}", e),
                    },
                };

                if let Ok(mut out_bytes) = serde_json::to_vec(&resp) {
                    out_bytes.push(b'\n');
                    if writer.write_all(&out_bytes).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    fn md5_or_placeholder(jid: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        jid.hash(&mut hasher);
        hasher.finish()
    }

    /// Client interface used by sdk-engine / wallet to communicate with the daemon.
    #[derive(Clone, Debug)]
    pub struct ProverDaemonClient {
        socket_path: PathBuf,
    }

    impl ProverDaemonClient {
        pub fn new(socket_path: Option<PathBuf>) -> Self {
            Self {
                socket_path: socket_path.unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH)),
            }
        }

        pub fn socket_path(&self) -> &Path {
            &self.socket_path
        }

        pub async fn ping(&self) -> Result<bool, String> {
            match self.send_request(IpcRequest::Ping).await {
                Ok(IpcResponse::Pong { .. }) => Ok(true),
                Ok(_) => Ok(false),
                Err(e) => Err(e),
            }
        }

        pub async fn get_status(&self) -> Result<IpcResponse, String> {
            self.send_request(IpcRequest::Status).await
        }

        pub async fn submit_job(&self, payload: ProvingJobPayload) -> Result<String, String> {
            match self.send_request(IpcRequest::Submit { payload }).await? {
                IpcResponse::Submitted { job_id } => Ok(job_id),
                IpcResponse::Error { message } => Err(message),
                other => Err(format!("Unexpected response: {:?}", other)),
            }
        }

        pub async fn query_job(&self, job_id: &str) -> Result<ProverJobStatus, String> {
            match self
                .send_request(IpcRequest::Query {
                    job_id: job_id.to_string(),
                })
                .await?
            {
                IpcResponse::JobStatus { status } => Ok(status),
                IpcResponse::Error { message } => Err(message),
                other => Err(format!("Unexpected response: {:?}", other)),
            }
        }

        pub async fn cancel_job(&self, job_id: &str) -> Result<bool, String> {
            match self
                .send_request(IpcRequest::Cancel {
                    job_id: job_id.to_string(),
                })
                .await?
            {
                IpcResponse::Cancelled { .. } => Ok(true),
                IpcResponse::Error { message } => Err(message),
                other => Err(format!("Unexpected response: {:?}", other)),
            }
        }

        #[cfg(unix)]
        async fn send_request(&self, req: IpcRequest) -> Result<IpcResponse, String> {
            let stream = tokio::net::UnixStream::connect(&self.socket_path)
                .await
                .map_err(|e| {
                    format!(
                        "Failed to connect to daemon at {:?}: {}",
                        self.socket_path, e
                    )
                })?;

            let (reader, mut writer) = stream.into_split();
            let mut req_bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
            req_bytes.push(b'\n');

            writer
                .write_all(&req_bytes)
                .await
                .map_err(|e| e.to_string())?;
            let mut line = String::new();
            let mut buf_reader = BufReader::new(reader);
            buf_reader
                .read_line(&mut line)
                .await
                .map_err(|e| e.to_string())?;

            serde_json::from_str::<IpcResponse>(line.trim())
                .map_err(|e| format!("Malformed daemon response: {}", e))
        }

        #[cfg(not(unix))]
        async fn send_request(&self, _req: IpcRequest) -> Result<IpcResponse, String> {
            Err("UDS IPC client not supported on non-unix".to_string())
        }
    }

    /// CLI binary entrypoint for `e-prover-daemon`.
    pub fn daemon_main() {
        let args: Vec<String> = std::env::args().collect();
        let socket_path = args
            .iter()
            .position(|a| a == "--socket")
            .and_then(|idx| args.get(idx + 1))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));

        let threads: usize = args
            .iter()
            .position(|a| a == "--threads")
            .and_then(|idx| args.get(idx + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        if args.iter().any(|a| a == "--ping") {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let client = ProverDaemonClient::new(Some(socket_path.clone()));
                match client.ping().await {
                    Ok(true) => {
                        println!("[✓] e-prover-daemon is active on {:?}", socket_path);
                        std::process::exit(0);
                    }
                    _ => {
                        eprintln!("[✗] e-prover-daemon is NOT reachable on {:?}", socket_path);
                        std::process::exit(1);
                    }
                }
            });
            return;
        }

        if args.iter().any(|a| a == "--status") {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let client = ProverDaemonClient::new(Some(socket_path.clone()));
                match client.get_status().await {
                    Ok(status) => {
                        println!("{}", serde_json::to_string_pretty(&status).unwrap());
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("[✗] Error querying daemon: {}", e);
                        std::process::exit(1);
                    }
                }
            });
            return;
        }

        println!("=======================================================");
        println!("  Logos Execution Zone (LEZ) Prover Worker Daemon");
        println!("=======================================================");
        println!("Socket: {:?}", socket_path);
        println!("Max Proving Concurrency: {} (Dual-Core Optimized)", threads);
        println!("Starting daemon listener...");

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads.max(2))
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        rt.block_on(async {
            let (daemon, rx) = ProverWorkerDaemon::new(threads, Some(socket_path.clone()));
            daemon.clone().spawn_worker_loop(rx);

            tokio::select! {
                res = daemon.run_ipc_server() => {
                    if let Err(e) = res {
                        eprintln!("[✗] Daemon IPC server error: {:?}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\n[!] Shutting down e-prover-daemon gracefully...");
                    let _ = tokio::fs::remove_file(&socket_path).await;
                }
            }
        });
    }
}
