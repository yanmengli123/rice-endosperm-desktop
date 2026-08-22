use std::{collections::HashMap, path::Path, sync::Mutex};

use tokio_util::sync::CancellationToken;

use crate::{
    credentials::CredentialStore,
    database::Database,
    error::{AppError, AppResult},
    yuxi::YuxiClient,
};

pub struct AppState {
    pub database: Database,
    pub credentials: CredentialStore,
    pub yuxi: YuxiClient,
    active_requests: Mutex<HashMap<String, ActiveRequest>>,
}

struct ActiveRequest {
    cancellation: CancellationToken,
    run_id: Option<String>,
}

impl AppState {
    pub async fn open(app_data_dir: &Path, app_version: &str) -> AppResult<Self> {
        Ok(Self {
            database: Database::open(app_data_dir).await?,
            credentials: CredentialStore::open(app_data_dir)?,
            yuxi: YuxiClient::new(app_version)?,
            active_requests: Mutex::new(HashMap::new()),
        })
    }

    pub fn register_request(&self, request_id: &str) -> AppResult<CancellationToken> {
        let mut requests = self
            .active_requests
            .lock()
            .map_err(|_| AppError::Internal("取消任务状态锁已损坏".into()))?;
        if requests.contains_key(request_id) {
            // 无条件覆盖会让旧请求脱离跟踪：既不能被取消，其 run 行也会被
            // ON CONFLICT(request_id) 改挂到新 run 上导致远端 run 失联。
            return Err(AppError::Protocol("相同的请求仍在处理中".into()));
        }
        let token = CancellationToken::new();
        requests.insert(
            request_id.to_owned(),
            ActiveRequest {
                cancellation: token.clone(),
                run_id: None,
            },
        );
        Ok(token)
    }

    pub fn set_request_run_id(&self, request_id: &str, run_id: &str) -> AppResult<()> {
        let mut requests = self
            .active_requests
            .lock()
            .map_err(|_| AppError::Internal("运行状态锁已损坏".into()))?;
        if let Some(request) = requests.get_mut(request_id) {
            request.run_id = Some(run_id.to_owned());
        }
        Ok(())
    }

    pub fn cancel_request(&self, request_id: &str) -> AppResult<Option<String>> {
        let requests = self
            .active_requests
            .lock()
            .map_err(|_| AppError::Internal("取消任务状态锁已损坏".into()))?;
        let Some(request) = requests.get(request_id) else {
            return Ok(None);
        };
        request.cancellation.cancel();
        Ok(request.run_id.clone())
    }

    pub fn finish_request(&self, request_id: &str) {
        if let Ok(mut requests) = self.active_requests.lock() {
            requests.remove(request_id);
        }
    }
}
