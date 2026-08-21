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

    pub async fn api_key_hint(&self) -> AppResult<Option<String>> {
        self.setting("api_key_hint").await
    }

    pub async fn create_thread(&self) -> AppResult<ThreadSummary> {
        let id = Uuid::new_v4().to_string();
        let timestamp = now();
        sqlx::query(
            "INSERT INTO threads(id, title, agent_slug, created_at, updated_at) VALUES(?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(NEW_THREAD_TITLE)
        .bind(agent_slug())
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
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?)")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await?;
        if exists {
            Ok(())
        } else {
            Err(AppError::ThreadNotFound)
        }
    }

    pub async fn list_threads(&self) -> AppResult<Vec<ThreadSummary>> {
        sqlx::query_as::<_, ThreadSummary>(
            "SELECT t.id, t.title, t.updated_at, \
             COALESCE((SELECT content FROM messages m WHERE m.thread_id = t.id ORDER BY position DESC LIMIT 1), '') AS preview \
             FROM threads t ORDER BY t.updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn rename_thread(&self, thread_id: &str, title: &str) -> AppResult<()> {
        let cleaned = title.trim().chars().take(80).collect::<String>();
        if cleaned.is_empty() {
            return Err(AppError::Internal("会话标题不能为空".into()));
        }
        let result = sqlx::query("UPDATE threads SET title = ?, updated_at = ? WHERE id = ?")
            .bind(cleaned)
            .bind(now())
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::ThreadNotFound);
        }
        Ok(())
    }

    pub async fn delete_thread(&self, thread_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM threads WHERE id = ?")
            .bind(thread_id)
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
        let mut transaction = self.pool.begin().await?;
        let next_position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM messages WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO messages(id, thread_id, role, content, position, created_at) VALUES(?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET content = excluded.content",
        )
        .bind(id)
        .bind(thread_id)
        .bind(role)
        .bind(content)
        .bind(next_position)
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
        sqlx::query_scalar("SELECT yuxi_thread_id FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(AppError::ThreadNotFound)
    }

    pub async fn set_yuxi_thread_id(&self, thread_id: &str, yuxi_thread_id: &str) -> AppResult<()> {
        sqlx::query("UPDATE threads SET yuxi_thread_id = ?, updated_at = ? WHERE id = ?")
            .bind(yuxi_thread_id)
            .bind(now())
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
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
        sqlx::query("UPDATE runs SET status = ?, updated_at = ? WHERE run_id = ?")
            .bind(status)
            .bind(now())
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn latest_run_context(&self, thread_id: &str) -> AppResult<Option<String>> {
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
        sqlx::query_as::<_, PendingRun>(
            "SELECT run_id, thread_id FROM runs \
             WHERE status NOT IN ('completed', 'failed', 'cancelled', 'interrupted') \
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
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
        sqlx::query(
            "UPDATE runs SET status = ?, last_event_id = COALESCE(?, last_event_id), accumulated_text = ?, \
             error_code = ?, updated_at = ?, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE run_id = ?",
        )
        .bind(status)
        .bind(event_id)
        .bind(accumulated_text)
        .bind(error_code)
        .bind(&timestamp)
        .bind(terminal)
        .bind(&timestamp)
        .bind(run_id)
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
    Utc::now().to_rfc3339()
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
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 2")
            .execute(&database.pool)
            .await
            .expect("simulate published v1 migration state");
        sqlx::query("DROP INDEX IF EXISTS idx_runs_status_updated")
            .execute(&database.pool)
            .await
            .expect("remove v2 index");
        sqlx::query("ALTER TABLE runs DROP COLUMN server_context")
            .execute(&database.pool)
            .await
            .expect("remove v2 column");
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
        assert_eq!(latest_version, 2);
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
