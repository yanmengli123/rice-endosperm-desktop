use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, RwLock},
};

use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::{
    credentials::CredentialStore,
    database::Database,
    error::{AppError, AppResult},
    workflow::WorkflowState,
    yuxi::{ProtocolInfo, YuxiClient},
};

pub struct AppState {
    pub database: Database,
    pub credentials: CredentialStore,
    pub yuxi: YuxiClient,
    pub workflow: WorkflowState,
    active_requests: Mutex<HashMap<String, ActiveRequest>>,
    /// 最近一次成功协商的服务端协议能力快照，按网关地址缓存（切换网关/账号后
    /// 自动失效重取）。渲染分支以能力位为准，替代「字段缺席 ⟺ 未发布」的
    /// 隐式判断。
    protocol_info: RwLock<Option<(String, ProtocolInfo)>>,
    /// 会话刷新单飞锁（按账号作用域）：并发命令同时发现访问令牌临期时，
    /// 只允许一个任务发起 HTTP 轮换；等待者拿到锁后双重检查再复用新令牌。
    /// 服务端对已消费的刷新令牌判重放并撤销整个会话族（reuse_detected），
    /// 无互斥时并发刷新会自己把自己踢下线。
    session_refresh_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
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
            workflow: WorkflowState::open(app_data_dir).await?,
            active_requests: Mutex::new(HashMap::new()),
            protocol_info: RwLock::new(None),
            session_refresh_locks: Mutex::new(HashMap::new()),
        })
    }

    /// 缓存某网关地址的协议能力快照（同地址覆盖为最新值）。
    pub fn set_protocol_info(&self, gateway_url: &str, info: ProtocolInfo) {
        if let Ok(mut cache) = self.protocol_info.write() {
            *cache = Some((gateway_url.to_owned(), info));
        }
    }

    /// 读取当前网关地址命中的能力快照；未协商过或网关已切换返回 None。
    pub fn cached_protocol_info(&self, gateway_url: &str) -> Option<ProtocolInfo> {
        let cache = self.protocol_info.read().ok()?;
        let (cached_url, info) = cache.as_ref()?;
        (cached_url == gateway_url).then(|| info.clone())
    }

    /// 取（或创建）某账号作用域的刷新单飞锁。条目随账号数量增长，量级可忽略。
    pub fn session_refresh_lock(&self, scope: &str) -> AppResult<Arc<AsyncMutex<()>>> {
        let mut locks = self
            .session_refresh_locks
            .lock()
            .map_err(|_| AppError::Internal("会话刷新锁已损坏".into()))?;
        Ok(locks.entry(scope.to_owned()).or_default().clone())
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
