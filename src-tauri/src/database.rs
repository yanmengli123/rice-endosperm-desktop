use std::{path::Path, str::FromStr, time::Duration};

use chrono::Utc;
use serde::Serialize;
use sqlx::{
    FromRow, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use uuid::Uuid;

use crate::{
    config::{agent_slug, default_gateway_url},
    error::{AppError, AppResult},
};

const NEW_THREAD_TITLE: &str = "新对话";

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
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
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
    ) -> AppResult<()> {
        let timestamp = now();
        sqlx::query(
            "INSERT INTO runs(run_id, request_id, thread_id, status, created_at, updated_at) \
             VALUES(?, ?, ?, ?, ?, ?) \
             ON CONFLICT(request_id) DO UPDATE SET run_id = excluded.run_id, status = excluded.status, updated_at = excluded.updated_at",
        )
        .bind(run_id)
        .bind(request_id)
        .bind(thread_id)
        .bind(status)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
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

fn now() -> String {
    Utc::now().to_rfc3339()
}
