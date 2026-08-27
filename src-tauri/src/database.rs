use std::{path::Path, str::FromStr, time::Duration};

use chrono::Utc;
use serde::Serialize;
use sqlx::{
    FromRow, SqlitePool,
    migrate::{MigrateError, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use uuid::Uuid;

use crate::{
    config::{agent_slug, default_gateway_url},
    diagnostics,
    error::{AppError, AppResult},
};

const NEW_THREAD_TITLE: &str = "新对话";
const LEGACY_ACCOUNT_SCOPE: &str = "legacy";

/// 切换器可见的账号摘要。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub account_scope: String,
    pub display_name: String,
    pub gateway_url: String,
    #[serde(default)]
    pub is_active: bool,
}

const MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const V1_LF_CHECKSUM: &str = "6F2F8974DC2BC853D4B6273B0F0947A92164C68855109BF463515E9F20D440F8685DC005927DD751E2B1E863AF645738";
const V1_LEGACY_CRLF_CHECKSUM: &str = "40B0BBD1ADEC82DD375EAA5DB004CC46DA34D740C3B6012DE181E8F692A2D2C0DE03A5B0B50C1777070F697489636415";

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub preview: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSettings {
    pub gateway_url: String,
    pub agent_slug: String,
    pub has_api_key: bool,
    pub api_key_hint: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PendingRun {
    pub run_id: String,
    pub thread_id: String,
}

impl Database {
    pub async fn open(app_data_dir: &Path) -> AppResult<Self> {
        std::fs::create_dir_all(app_data_dir)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let database_path = app_data_dir.join("rice-endosperm.db");
        let options =
            SqliteConnectOptions::from_str(&format!("sqlite:{}", database_path.display()))
                .map_err(|error| AppError::Database(error.to_string()))?
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        run_migrations(&pool, app_data_dir, &database_path).await?;
        Ok(Self { pool })
    }

    pub async fn setting(&self, key: &str) -> AppResult<Option<String>> {
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn save_setting(&self, key: &str, value: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO app_settings(key, value, updated_at) VALUES(?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn gateway_url(&self) -> AppResult<String> {
        Ok(self
            .setting("gateway_url")
            .await?
            .unwrap_or_else(|| default_gateway_url().to_owned()))
    }

    /// 最近一次由服务端确认的默认智能体。编译期值仅用于首次连接前展示，
    /// 真正创建远端会话时仍会向服务端重新解析并固化线程绑定。
    pub async fn server_agent_slug(&self) -> AppResult<String> {
        Ok(self
            .setting("server_agent_slug")
            .await?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| agent_slug().to_owned()))
    }

    pub async fn save_server_agent_slug(&self, value: &str) -> AppResult<()> {
        let normalized = value.trim();
        if normalized.is_empty() {
            return Err(AppError::Internal("服务端默认智能体不能为空".into()));
        }
        self.save_setting("server_agent_slug", normalized).await
    }

    pub async fn api_key_hint(&self) -> AppResult<Option<String>> {
        self.setting("api_key_hint").await
    }

    pub async fn current_account_scope(&self) -> AppResult<String> {
        Ok(self
            .setting("current_account_scope")
            .await?
            .unwrap_or_else(|| LEGACY_ACCOUNT_SCOPE.to_owned()))
    }

    /// P2b 多账号：登记/更新账号目录行（凭据本体在 Stronghold，按作用域隔离）。
    pub async fn upsert_account(
        &self,
        account_scope: &str,
        display_name: &str,
        gateway_url: &str,
    ) -> AppResult<()> {
        let now = now();
        sqlx::query(
            "INSERT INTO accounts(account_scope, display_name, gateway_url, created_at)              VALUES(?, ?, ?, ?)              ON CONFLICT(account_scope) DO UPDATE SET                display_name = excluded.display_name, gateway_url = excluded.gateway_url",
        )
        .bind(account_scope)
        .bind(display_name)
        .bind(gateway_url)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// P2b 多账号：列出全部已登录账号（按创建时间）。
    pub async fn list_accounts(&self) -> AppResult<Vec<AccountSummary>> {
        let rows = sqlx::query(
            "SELECT account_scope, display_name, gateway_url FROM accounts ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut accounts = Vec::with_capacity(rows.len());
        for row in rows {
            use sqlx::Row as _;
            accounts.push(AccountSummary {
                account_scope: row.try_get(0)?,
                display_name: row.try_get(1)?,
                gateway_url: row.try_get(2)?,
                is_active: false,
            });
        }
        Ok(accounts)
    }

    /// P2b 多账号：移除账号目录行（历史会话保留但不再出现在切换列表）。
    pub async fn delete_account(&self, account_scope: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM accounts WHERE account_scope = ?")
            .bind(account_scope)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn activate_account(
        &self,
        gateway_url: &str,
        principal: &str,
        api_key_hint: &str,
        api_key_name: Option<&str>,
    ) -> AppResult<()> {
        let account_scope = format!("{}|{}", gateway_url.trim_end_matches('/'), principal);
        let timestamp = now();
        let mut transaction = self.pool.begin().await?;
        for (key, value) in [
            ("gateway_url", gateway_url),
            ("current_account_scope", account_scope.as_str()),
            ("api_key_hint", api_key_hint),
        ] {
            sqlx::query(
                "INSERT INTO app_settings(key, value, updated_at) VALUES(?, ?, ?) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            )
            .bind(key)
            .bind(value)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await?;
        }
        if let Some(name) = api_key_name {
            sqlx::query(
                "INSERT INTO app_settings(key, value, updated_at) VALUES('api_key_name', ?, ?) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            )
            .bind(name)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn create_thread(&self) -> AppResult<ThreadSummary> {
        let id = Uuid::new_v4().to_string();
        let timestamp = now();
        let account_scope = self.current_account_scope().await?;
        let server_agent_slug = self.server_agent_slug().await?;
        sqlx::query(
            "INSERT INTO threads(id, title, agent_slug, account_scope, created_at, updated_at) \
             VALUES(?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(NEW_THREAD_TITLE)
        .bind(server_agent_slug)
        .bind(account_scope)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&self.pool)
        .await?;
        Ok(ThreadSummary {
            id,
            title: NEW_THREAD_TITLE.into(),
            updated_at: timestamp,
            preview: String::new(),
        })
    }

    pub async fn ensure_thread(&self, thread_id: &str) -> AppResult<()> {
        let account_scope = self.current_account_scope().await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ? AND account_scope = ?)",
        )
        .bind(thread_id)
        .bind(account_scope)
        .fetch_one(&self.pool)
        .await?;
        if exists {
            Ok(())
        } else {
            Err(AppError::ThreadNotFound)
        }
    }

    pub async fn list_threads(&self) -> AppResult<Vec<ThreadSummary>> {
        let account_scope = self.current_account_scope().await?;
        sqlx::query_as::<_, ThreadSummary>(
            "SELECT t.id, t.title, t.updated_at, \
             COALESCE((SELECT content FROM messages m WHERE m.thread_id = t.id ORDER BY position DESC LIMIT 1), '') AS preview \
             FROM threads t WHERE t.account_scope = ? ORDER BY t.updated_at DESC",
        )
        .bind(account_scope)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn rename_thread(&self, thread_id: &str, title: &str) -> AppResult<()> {
        let cleaned = title.trim().chars().take(80).collect::<String>();
        if cleaned.is_empty() {
            return Err(AppError::Internal("会话标题不能为空".into()));
        }
        let account_scope = self.current_account_scope().await?;
        let result = sqlx::query(
            "UPDATE threads SET title = ?, updated_at = ? WHERE id = ? AND account_scope = ?",
        )
        .bind(cleaned)
        .bind(now())
        .bind(thread_id)
        .bind(account_scope)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::ThreadNotFound);
        }
        Ok(())
    }

    pub async fn delete_thread(&self, thread_id: &str) -> AppResult<()> {
        let account_scope = self.current_account_scope().await?;
        sqlx::query("DELETE FROM threads WHERE id = ? AND account_scope = ?")
            .bind(thread_id)
            .bind(account_scope)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn load_messages(&self, thread_id: &str) -> AppResult<Vec<LocalMessage>> {
        self.ensure_thread(thread_id).await?;
        sqlx::query_as::<_, LocalMessage>(
            "SELECT id, role, content, created_at FROM messages WHERE thread_id = ? ORDER BY position",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn append_message(
        &self,
        id: &str,
        thread_id: &str,
        role: &str,
        content: &str,
    ) -> AppResult<()> {
        self.ensure_thread(thread_id).await?;
        let mut transaction = self.pool.begin().await?;
        // position 在 INSERT 内用标量子查询计算：单条语句在 SQLite 写锁下
        // 原生原子，避免"先查 MAX 再插入"在并发下撞 UNIQUE(thread_id, position)。
        sqlx::query(
            "INSERT INTO messages(id, thread_id, role, content, position, created_at) \
             VALUES(?, ?, ?, ?, COALESCE((SELECT MAX(position) FROM messages WHERE thread_id = ?), 0) + 1, ?) \
             ON CONFLICT(id) DO UPDATE SET content = excluded.content",
        )
        .bind(id)
        .bind(thread_id)
        .bind(role)
        .bind(content)
        .bind(thread_id)
        .bind(now())
        .execute(&mut *transaction)
        .await?;

        let current_title: String = sqlx::query_scalar("SELECT title FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(&mut *transaction)
            .await?;
        let title = if role == "user" && current_title == NEW_THREAD_TITLE {
            content.trim().chars().take(28).collect::<String>()
        } else {
            current_title
        };
        sqlx::query("UPDATE threads SET title = ?, updated_at = ? WHERE id = ?")
            .bind(title)
            .bind(now())
            .bind(thread_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn yuxi_thread_id(&self, thread_id: &str) -> AppResult<Option<String>> {
        let account_scope = self.current_account_scope().await?;
        sqlx::query_scalar("SELECT yuxi_thread_id FROM threads WHERE id = ? AND account_scope = ?")
            .bind(thread_id)
            .bind(account_scope)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(AppError::ThreadNotFound)
    }

    pub async fn thread_agent_slug(&self, thread_id: &str) -> AppResult<String> {
        let account_scope = self.current_account_scope().await?;
        sqlx::query_scalar("SELECT agent_slug FROM threads WHERE id = ? AND account_scope = ?")
            .bind(thread_id)
            .bind(account_scope)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(AppError::ThreadNotFound)
    }

    /// 原子固化服务端会话 ID 与该会话实际绑定的智能体，避免客户端重启后
    /// 用新的默认智能体错误续写旧线程。
    pub async fn bind_server_thread(
        &self,
        thread_id: &str,
        yuxi_thread_id: &str,
        server_agent_slug: &str,
    ) -> AppResult<()> {
        let account_scope = self.current_account_scope().await?;
        let result = sqlx::query(
            "UPDATE threads SET yuxi_thread_id = ?, agent_slug = ?, updated_at = ? \
             WHERE id = ? AND account_scope = ?",
        )
        .bind(yuxi_thread_id)
        .bind(server_agent_slug)
        .bind(now())
        .bind(thread_id)
        .bind(account_scope)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::ThreadNotFound);
        }
        Ok(())
    }

    pub async fn insert_run(
        &self,
        run_id: &str,
        request_id: &str,
        thread_id: &str,
        status: &str,
        server_context: &str,
    ) -> AppResult<()> {
        self.ensure_thread(thread_id).await?;
        let timestamp = now();
        sqlx::query(
            "INSERT INTO runs(run_id, request_id, thread_id, status, server_context, created_at, updated_at) \
             VALUES(?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(request_id) DO UPDATE SET run_id = excluded.run_id, status = excluded.status, \
             server_context = excluded.server_context, updated_at = excluded.updated_at",
        )
        .bind(run_id)
        .bind(request_id)
        .bind(thread_id)
        .bind(status)
        .bind(server_context)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run_context(&self, run_id: &str, server_context: &str) -> AppResult<()> {
        sqlx::query("UPDATE runs SET server_context = ?, updated_at = ? WHERE run_id = ?")
            .bind(server_context)
            .bind(now())
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_run_status(&self, run_id: &str, status: &str) -> AppResult<()> {
        // 镜像服务端非终态时不得覆盖本地已写入的终态（对账与存活流并发的守卫）。
        sqlx::query(
            "UPDATE runs SET status = ?, updated_at = ? WHERE run_id = ? \
             AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted')",
        )
        .bind(status)
        .bind(now())
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn latest_run_context(&self, thread_id: &str) -> AppResult<Option<String>> {
        self.ensure_thread(thread_id).await?;
        sqlx::query_scalar(
            "SELECT server_context FROM runs WHERE thread_id = ? AND server_context IS NOT NULL \
             AND server_context != '' ORDER BY created_at DESC LIMIT 1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn list_pending_runs(&self) -> AppResult<Vec<PendingRun>> {
        let account_scope = self.current_account_scope().await?;
        sqlx::query_as::<_, PendingRun>(
            "SELECT r.run_id, r.thread_id FROM runs r \
             JOIN threads t ON t.id = r.thread_id \
             WHERE t.account_scope = ? \
             AND r.status NOT IN ('completed', 'failed', 'cancelled', 'interrupted') \
             ORDER BY r.created_at ASC",
        )
        .bind(account_scope)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn record_empty_completed_poll(&self, run_id: &str) -> AppResult<i64> {
        sqlx::query_scalar(
            "UPDATE runs SET status = 'awaiting_output', result_poll_count = result_poll_count + 1, \
             updated_at = ? WHERE run_id = ? RETURNING result_poll_count",
        )
        .bind(now())
        .bind(run_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_run_progress(
        &self,
        run_id: &str,
        status: &str,
        event_id: Option<&str>,
        accumulated_text: &str,
        error_code: Option<&str>,
        terminal: bool,
    ) -> AppResult<()> {
        let timestamp = now();
        // 非终态进度写入不得降级已终态的行：sync_pending_runs 与存活流可能
        // 并发更新同一 run，否则会出现 completed 被改回 running 的僵尸状态。
        sqlx::query(
            "UPDATE runs SET status = ?, last_event_id = COALESCE(?, last_event_id), accumulated_text = ?, \
             error_code = ?, updated_at = ?, finished_at = CASE WHEN ? THEN ? ELSE finished_at END \
             WHERE run_id = ? AND (? OR status NOT IN ('completed', 'failed', 'cancelled', 'interrupted'))",
        )
        .bind(status)
        .bind(event_id)
        .bind(accumulated_text)
        .bind(error_code)
        .bind(&timestamp)
        .bind(terminal)
        .bind(&timestamp)
        .bind(run_id)
        .bind(!terminal)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

async fn run_migrations(
    pool: &SqlitePool,
    app_data_dir: &Path,
    database_path: &Path,
) -> AppResult<()> {
    match MIGRATOR.run(pool).await {
        Ok(()) => Ok(()),
        Err(MigrateError::VersionMismatch(1)) => {
            repair_v1_line_ending_mismatch(pool, app_data_dir, database_path).await?;
            MIGRATOR
                .run(pool)
                .await
                .map_err(|error| AppError::Database(error.to_string()))
        }
        Err(error) => Err(AppError::Database(error.to_string())),
    }
}

async fn repair_v1_line_ending_mismatch(
    pool: &SqlitePool,
    app_data_dir: &Path,
    database_path: &Path,
) -> AppResult<()> {
    let stored_checksum: Option<String> = sqlx::query_scalar(
        "SELECT hex(checksum) FROM _sqlx_migrations WHERE version = 1 AND success = 1",
    )
    .fetch_optional(pool)
    .await?;
    let current_checksum = MIGRATOR
        .iter()
        .find(|migration| migration.version == 1)
        .map(|migration| uppercase_hex(migration.checksum.as_ref()))
        .ok_or_else(|| AppError::Database("找不到内置的数据库迁移版本 1".into()))?;

    if stored_checksum.as_deref() != Some(V1_LEGACY_CRLF_CHECKSUM)
        || current_checksum != V1_LF_CHECKSUM
        || !is_v1_schema_compatible(pool).await?
    {
        return Err(AppError::Database(
            "数据库迁移版本 1 的校验值不匹配，且不属于可安全修复的 v0.1.0 换行符兼容问题".into(),
        ));
    }

    let backup_dir = app_data_dir.join("backups");
    std::fs::create_dir_all(&backup_dir).map_err(|error| AppError::Database(error.to_string()))?;
    let backup_path = backup_dir.join(format!(
        "rice-endosperm-before-v1-repair-{}.db",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    create_consistent_backup(pool, &backup_path).await?;

    let migration = MIGRATOR
        .iter()
        .find(|migration| migration.version == 1)
        .ok_or_else(|| AppError::Database("找不到内置的数据库迁移版本 1".into()))?;
    let result = sqlx::query(
        "UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1 AND success = 1 AND hex(checksum) = ?",
    )
    .bind(migration.checksum.as_ref())
    .bind(V1_LEGACY_CRLF_CHECKSUM)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Database(
            "数据库迁移兼容修复未更新预期记录".into(),
        ));
    }

    diagnostics::log(
        "WARN",
        "migration_v1_repaired",
        &format!(
            "normalized legacy CRLF checksum; backup={}; database={}",
            backup_path.display(),
            database_path.display()
        ),
    );
    Ok(())
}

async fn is_v1_schema_compatible(pool: &SqlitePool) -> AppResult<bool> {
    const EXPECTED: &[(&str, &[&str])] = &[
        ("app_settings", &["key", "value", "updated_at"]),
        (
            "threads",
            &[
                "id",
                "yuxi_thread_id",
                "title",
                "agent_slug",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "messages",
            &[
                "id",
                "thread_id",
                "role",
                "content",
                "position",
                "created_at",
            ],
        ),
        (
            "runs",
            &[
                "run_id",
                "request_id",
                "thread_id",
                "status",
                "last_event_id",
                "accumulated_text",
                "error_code",
                "created_at",
                "updated_at",
                "finished_at",
            ],
        ),
    ];

    let latest_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(pool)
    .await?;
    if latest_version != 1 {
        return Ok(false);
    }

    for (table, expected_columns) in EXPECTED {
        let actual_columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(table)
                .fetch_all(pool)
                .await?;
        if actual_columns.len() != expected_columns.len()
            || !actual_columns
                .iter()
                .zip(*expected_columns)
                .all(|(actual, expected)| actual == expected)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn create_consistent_backup(pool: &SqlitePool, backup_path: &Path) -> AppResult<()> {
    sqlx::query("VACUUM main INTO ?")
        .bind(backup_path.to_string_lossy().as_ref())
        .execute(pool)
        .await
        .map_err(|error| AppError::Database(format!("创建迁移前备份失败：{error}")))?;
    Ok(())
}

fn uppercase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            let _ = write!(output, "{byte:02X}");
            output
        },
    )
}

fn now() -> String {
    // 统一带毫秒：to_rfc3339() 在纳秒为 0 时省略小数秒，字典序会在同一秒内
    // 颠倒（'+' < '.'），影响 latest_run_context / list_threads 的排序。
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod migration_tests {
    use super::{Database, V1_LEGACY_CRLF_CHECKSUM, V1_LF_CHECKSUM};
    use uuid::Uuid;

    #[tokio::test]
    async fn repairs_the_published_v1_crlf_checksum_without_losing_data() {
        let root = std::env::temp_dir().join(format!("daoxin-migration-test-{}", Uuid::new_v4()));
        let database = Database::open(&root).await.expect("create test database");
        database
            .save_setting("migration_test", "preserved")
            .await
            .expect("insert test data");
        let legacy_checksum = decode_hex(V1_LEGACY_CRLF_CHECKSUM);
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version >= 2")
            .execute(&database.pool)
            .await
            .expect("simulate published v1 migration state");
        sqlx::query("DROP INDEX IF EXISTS idx_runs_status_updated")
            .execute(&database.pool)
            .await
            .expect("remove v2 index");
        sqlx::query("DROP INDEX IF EXISTS idx_threads_account_updated")
            .execute(&database.pool)
            .await
            .expect("remove v4 index");
        sqlx::query("ALTER TABLE threads DROP COLUMN account_scope")
            .execute(&database.pool)
            .await
            .expect("remove v4 column");
        sqlx::query("ALTER TABLE runs DROP COLUMN server_context")
            .execute(&database.pool)
            .await
            .expect("remove v2 column");
        sqlx::query("ALTER TABLE runs DROP COLUMN result_poll_count")
            .execute(&database.pool)
            .await
            .expect("remove v3 column");
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
            .bind(legacy_checksum)
            .execute(&database.pool)
            .await
            .expect("simulate v0.1.0 checksum");
        database.pool.close().await;

        let repaired = Database::open(&root).await.expect("repair legacy checksum");
        let checksum: String =
            sqlx::query_scalar("SELECT hex(checksum) FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&repaired.pool)
                .await
                .expect("read repaired checksum");
        assert_eq!(checksum, V1_LF_CHECKSUM);
        let latest_version: i64 =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&repaired.pool)
                .await
                .expect("read latest migration");
        assert_eq!(latest_version, 5);
        assert_eq!(
            repaired
                .setting("migration_test")
                .await
                .expect("read test data")
                .as_deref(),
            Some("preserved")
        );
        repaired.pool.close().await;
        let backup_count = std::fs::read_dir(root.join("backups"))
            .expect("backup directory")
            .count();
        assert_eq!(backup_count, 1);
        drop(repaired);
        drop(database);
        remove_test_directory(&root).await;
    }

    #[tokio::test]
    async fn persists_server_context_and_finds_only_unfinished_runs() {
        let root = std::env::temp_dir().join(format!("daoxin-run-context-test-{}", Uuid::new_v4()));
        let database = Database::open(&root).await.expect("create test database");
        let thread = database.create_thread().await.expect("create thread");
        let context = r#"{"protocolVersion":"1.1","modelSpec":"provider:model"}"#;
        database
            .insert_run("run-1", "request-1", &thread.id, "pending", context)
            .await
            .expect("insert pending run");

        let pending = database
            .list_pending_runs()
            .await
            .expect("list pending runs");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].run_id, "run-1");
        assert_eq!(
            database
                .latest_run_context(&thread.id)
                .await
                .expect("read context")
                .as_deref(),
            Some(context)
        );

        assert_eq!(
            database
                .record_empty_completed_poll("run-1")
                .await
                .expect("record empty result"),
            1
        );
        assert_eq!(
            database
                .record_empty_completed_poll("run-1")
                .await
                .expect("record second empty result"),
            2
        );
        assert_eq!(
            database
                .list_pending_runs()
                .await
                .expect("list awaiting-output run")
                .len(),
            1
        );

        database
            .update_run_progress("run-1", "completed", None, "answer", None, true)
            .await
            .expect("complete run");
        assert!(
            database
                .list_pending_runs()
                .await
                .expect("list completed runs")
                .is_empty()
        );
        database.pool.close().await;
        drop(database);
        remove_test_directory(&root).await;
    }

    #[tokio::test]
    async fn isolates_local_threads_between_server_accounts() {
        let root =
            std::env::temp_dir().join(format!("daoxin-account-scope-test-{}", Uuid::new_v4()));
        let database = Database::open(&root).await.expect("create test database");
        database
            .activate_account("https://api.example.cn", "user-a", "yxkey_a", Some("A"))
            .await
            .expect("activate account A");
        let thread_a = database
            .create_thread()
            .await
            .expect("create account A thread");

        database
            .activate_account("https://api.example.cn", "user-b", "yxkey_b", Some("B"))
            .await
            .expect("activate account B");
        assert!(
            database
                .list_threads()
                .await
                .expect("list B threads")
                .is_empty()
        );
        assert!(database.ensure_thread(&thread_a.id).await.is_err());

        database
            .activate_account("https://api.example.cn", "user-a", "yxkey_a", Some("A"))
            .await
            .expect("restore account A");
        assert_eq!(
            database.list_threads().await.expect("list A threads").len(),
            1
        );

        database.pool.close().await;
        drop(database);
        remove_test_directory(&root).await;
    }

    async fn remove_test_directory(path: &std::path::Path) {
        let mut last_error = None;
        for _ in 0..20 {
            match std::fs::remove_dir_all(path) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("remove test directory: {:?}", last_error.unwrap());
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).expect("ASCII checksum");
                u8::from_str_radix(pair, 16).expect("hex checksum")
            })
            .collect()
    }
}
