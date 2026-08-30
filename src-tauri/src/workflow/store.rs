use std::{path::Path, str::FromStr, time::Duration};

use chrono::Utc;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::error::{AppError, AppResult};

use super::{WorkflowArtifact, WorkflowProject, WorkflowRun};

#[derive(Clone)]
pub struct WorkflowStore {
    pool: SqlitePool,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

impl WorkflowStore {
    pub async fn open(app_data_dir: &Path) -> AppResult<Self> {
        let workflow_dir = app_data_dir.join("workflow");
        std::fs::create_dir_all(&workflow_dir)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let database_path = workflow_dir.join("registry.sqlite");
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
        sqlx::migrate!("./workflow-migrations")
            .run(&pool)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(Self { pool })
    }

    pub async fn interrupt_orphaned_runs(&self) -> AppResult<()> {
        sqlx::query(
            "UPDATE workflow_runs SET status='interrupted', error=COALESCE(error, ?), finished_at=? \
             WHERE status IN ('queued', 'running')",
        )
        .bind("应用上次退出时运行尚未结束，请确认输入和输出后重新运行")
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_project(&self, project: &WorkflowProject) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO workflow_projects(id,name,root,created_at,updated_at) VALUES(?,?,?,?,?)",
        )
        .bind(&project.id)
        .bind(&project.name)
        .bind(&project.root)
        .bind(&project.created_at)
        .bind(&project.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(ref database) if database.is_unique_violation() => {
                AppError::Internal("该目录已经注册为科研工作流项目".into())
            }
            other => other.into(),
        })?;
        Ok(())
    }

    pub async fn list_projects(&self) -> AppResult<Vec<WorkflowProject>> {
        Ok(sqlx::query_as::<_, WorkflowProject>(
            "SELECT id,name,root,created_at,updated_at FROM workflow_projects ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn project(&self, id: &str) -> AppResult<WorkflowProject> {
        sqlx::query_as::<_, WorkflowProject>(
            "SELECT id,name,root,created_at,updated_at FROM workflow_projects WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::Internal("找不到科研工作流项目".into()))
    }

    pub async fn delete_project(&self, id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM workflow_projects WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_run(&self, run: &WorkflowRun) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO workflow_runs(id,project_id,workflow_kind,status,input_path,manifest_path,summary_json,error,created_at,started_at,finished_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&run.id)
        .bind(&run.project_id)
        .bind(&run.workflow_kind)
        .bind(&run.status)
        .bind(&run.input_path)
        .bind(&run.manifest_path)
        .bind(&run.summary_json)
        .bind(&run.error)
        .bind(&run.created_at)
        .bind(&run.started_at)
        .bind(&run.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run(
        &self,
        id: &str,
        status: &str,
        manifest_path: Option<&str>,
        summary_json: &str,
        error: Option<&str>,
        finished: bool,
    ) -> AppResult<()> {
        let finished_at = finished.then(now);
        sqlx::query(
            "UPDATE workflow_runs SET status=?, manifest_path=?, summary_json=?, error=?, finished_at=? WHERE id=?",
        )
        .bind(status)
        .bind(manifest_path)
        .bind(summary_json)
        .bind(error)
        .bind(finished_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn run(&self, id: &str) -> AppResult<WorkflowRun> {
        sqlx::query_as::<_, WorkflowRun>(
            "SELECT id,project_id,workflow_kind,status,input_path,manifest_path,summary_json,error,created_at,started_at,finished_at FROM workflow_runs WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::Internal("找不到科研工作流运行".into()))
    }

    pub async fn list_runs(&self, project_id: &str) -> AppResult<Vec<WorkflowRun>> {
        Ok(sqlx::query_as::<_, WorkflowRun>(
            "SELECT id,project_id,workflow_kind,status,input_path,manifest_path,summary_json,error,created_at,started_at,finished_at \
             FROM workflow_runs WHERE project_id=? ORDER BY created_at DESC LIMIT 100",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn insert_artifact(&self, artifact: &WorkflowArtifact) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO workflow_artifacts(id,run_id,project_id,name,relative_path,media_type,size_bytes,sha256,created_at) \
             VALUES(?,?,?,?,?,?,?,?,?)",
        )
        .bind(&artifact.id)
        .bind(&artifact.run_id)
        .bind(&artifact.project_id)
        .bind(&artifact.name)
        .bind(&artifact.relative_path)
        .bind(&artifact.media_type)
        .bind(artifact.size_bytes)
        .bind(&artifact.sha256)
        .bind(&artifact.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_artifacts(&self, project_id: &str) -> AppResult<Vec<WorkflowArtifact>> {
        Ok(sqlx::query_as::<_, WorkflowArtifact>(
            "SELECT id,run_id,project_id,name,relative_path,media_type,size_bytes,sha256,created_at \
             FROM workflow_artifacts WHERE project_id=? ORDER BY created_at DESC LIMIT 200",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn artifact(&self, id: &str) -> AppResult<WorkflowArtifact> {
        sqlx::query_as::<_, WorkflowArtifact>(
            "SELECT id,run_id,project_id,name,relative_path,media_type,size_bytes,sha256,created_at FROM workflow_artifacts WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::Internal("找不到科研工作流产物".into()))
    }

    pub async fn setting(&self, key: &str) -> AppResult<Option<String>> {
        sqlx::query_scalar("SELECT value FROM workflow_settings WHERE key=?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn save_setting(&self, key: &str, value: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO workflow_settings(key,value,updated_at) VALUES(?,?,?) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM workflow_settings WHERE key=?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn orphaned_runs_become_interrupted_without_touching_completed_runs() {
        let root =
            std::env::temp_dir().join(format!("rice-workflow-store-{}", uuid::Uuid::new_v4()));
        let store = WorkflowStore::open(&root).await.unwrap();
        let stamp = now();
        let project = WorkflowProject {
            id: "project-1".into(),
            name: "Project".into(),
            root: root.join("project").to_string_lossy().into_owned(),
            created_at: stamp.clone(),
            updated_at: stamp.clone(),
        };
        store.insert_project(&project).await.unwrap();
        for (id, status) in [("running", "running"), ("done", "completed")] {
            store
                .insert_run(&WorkflowRun {
                    id: id.into(),
                    project_id: project.id.clone(),
                    workflow_kind: "test".into(),
                    status: status.into(),
                    input_path: None,
                    manifest_path: None,
                    summary_json: "{}".into(),
                    error: None,
                    created_at: stamp.clone(),
                    started_at: Some(stamp.clone()),
                    finished_at: (status == "completed").then(|| stamp.clone()),
                })
                .await
                .unwrap();
        }
        store.interrupt_orphaned_runs().await.unwrap();
        assert_eq!(store.run("running").await.unwrap().status, "interrupted");
        assert_eq!(store.run("done").await.unwrap().status, "completed");
        let _ = std::fs::remove_dir_all(root);
    }
}
