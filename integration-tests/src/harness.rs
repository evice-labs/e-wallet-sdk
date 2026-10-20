use async_trait::async_trait;
use sdk_engine::approval::{ApprovalDecision, ApprovalHandler, ApprovalPrompt};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Test approval handler that records prompts and auto-approves or rejects based on configuration.
#[derive(Clone)]
pub struct MockApprovalHandler {
    decision: Arc<Mutex<ApprovalDecision>>,
    prompts: Arc<Mutex<Vec<ApprovalPrompt>>>,
}

impl MockApprovalHandler {
    pub fn new(default_decision: ApprovalDecision) -> Self {
        Self {
            decision: Arc::new(Mutex::new(default_decision)),
            prompts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn set_decision(&self, decision: ApprovalDecision) {
        let mut d = self.decision.lock().await;
        *d = decision;
    }

    pub async fn recorded_prompts(&self) -> Vec<ApprovalPrompt> {
        self.prompts.lock().await.clone()
    }
}

#[async_trait]
impl ApprovalHandler for MockApprovalHandler {
    async fn request_approval(
        &self,
        prompt: ApprovalPrompt,
        _timeout: std::time::Duration,
    ) -> ApprovalDecision {
        self.prompts.lock().await.push(prompt);
        self.decision.lock().await.clone()
    }
}

/// Creates a unique isolated temporary directory for test wallet storage.
pub struct TestEnv {
    pub data_dir: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("lez_test_env_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("failed to create temporary test directory");
        Self { data_dir: dir }
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}
