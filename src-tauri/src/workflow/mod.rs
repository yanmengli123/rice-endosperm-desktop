mod commands;
mod pca;
mod project;
mod protocol;
mod store;
mod supervisor;

pub use commands::{
    cancel_workflow_agent, cancel_workflow_run, create_workflow_project,
    delete_workflow_model_settings, delete_workflow_project, get_workflow_engine_status,
    get_workflow_model_settings, list_workflow_artifacts, list_workflow_projects,
    list_workflow_runs, open_workflow_artifact, pick_workflow_directory, respond_workflow_approval,
    run_counts_pca_workflow, run_workflow_agent, save_workflow_model_settings,
};
pub use protocol::*;
pub use store::WorkflowStore;
pub use supervisor::WorkflowSupervisor;

use std::{collections::HashMap, path::Path, sync::Mutex};

use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

pub struct WorkflowState {
    pub store: WorkflowStore,
    pub supervisor: WorkflowSupervisor,
    active_runs: Mutex<HashMap<String, CancellationToken>>,
}

impl WorkflowState {
    pub async fn open(app_data_dir: &Path) -> AppResult<Self> {
        let store = WorkflowStore::open(app_data_dir).await?;
        store.interrupt_orphaned_runs().await?;
        Ok(Self {
            store,
            supervisor: WorkflowSupervisor::new(app_data_dir),
            active_runs: Mutex::new(HashMap::new()),
        })
    }

    pub fn register_run(&self, run_id: &str) -> AppResult<CancellationToken> {
        let mut active = self
            .active_runs
            .lock()
            .map_err(|_| AppError::Internal("工作流运行状态锁已损坏".into()))?;
        if active.contains_key(run_id) {
            return Err(AppError::Internal("工作流运行已经存在".into()));
        }
        let cancellation = CancellationToken::new();
        active.insert(run_id.to_owned(), cancellation.clone());
        Ok(cancellation)
    }

    pub fn cancel_run(&self, run_id: &str) -> AppResult<bool> {
        let active = self
            .active_runs
            .lock()
            .map_err(|_| AppError::Internal("工作流运行状态锁已损坏".into()))?;
        let Some(cancellation) = active.get(run_id) else {
            return Ok(false);
        };
        cancellation.cancel();
        Ok(true)
    }

    pub fn finish_run(&self, run_id: &str) {
        if let Ok(mut active) = self.active_runs.lock() {
            active.remove(run_id);
        }
    }
}
