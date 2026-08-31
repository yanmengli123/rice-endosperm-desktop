use std::{path::Path, str::FromStr, time::Duration};

use chrono::Utc;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::error::{AppError, AppResult};

use super::{WorkflowAgentTurn, WorkflowArtifact, WorkflowProject, WorkflowRun};

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
        let stamp = now();
        sqlx::query(
            "UPDATE workflow_runs SET status='interrupted', error=COALESCE(error, ?), finished_at=? \
             WHERE status IN ('queued', 'running')",
        )
        .bind("应用上次退出时运行尚未结束，请确认输入和输出后重新运行")
        .bind(&stamp)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE workflow_agent_turns SET status='interrupted', error=COALESCE(error, ?), finished_at=? \
             WHERE status='running'",
        )
        .bind("应用上次退出时本地智能体尚未完成，请核验项目文件后重新执行")
        .bind(stamp)
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
        let result = sqlx::query(
            "DELETE FROM workflow_projects WHERE id=? AND NOT EXISTS (\
             SELECT 1 FROM workflow_runs WHERE project_id=? AND status IN ('queued','running'))",
        )
        .bind(id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_projects WHERE id=?)")
                    .bind(id)
                    .fetch_one(&self.pool)
                    .await?;
            return Err(AppError::Internal(if exists {
                "项目仍有运行中的任务，请先停止并等待任务结束".into()
            } else {
                "找不到科研工作流项目".into()
            }));
        }
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

    pub async fn begin_agent_run(
        &self,
        run: &WorkflowRun,
        turn: &WorkflowAgentTurn,
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
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
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO workflow_agent_turns(id,run_id,project_id,engine_turn_id,engine_session_id,provider,model,prompt,response,status,error,input_tokens,output_tokens,reasoning_tokens,created_at,finished_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&turn.id)
        .bind(&turn.run_id)
        .bind(&turn.project_id)
        .bind(&turn.engine_turn_id)
        .bind(&turn.engine_session_id)
        .bind(&turn.provider)
        .bind(&turn.model)
        .bind(&turn.prompt)
        .bind(&turn.response)
        .bind(&turn.status)
        .bind(&turn.error)
        .bind(turn.input_tokens)
        .bind(turn.output_tokens)
        .bind(turn.reasoning_tokens)
        .bind(&turn.created_at)
        .bind(&turn.finished_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
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

    pub async fn complete_run(
        &self,
        run_id: &str,
        manifest_path: &str,
        summary_json: &str,
        artifacts: &[WorkflowArtifact],
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        for artifact in artifacts {
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
            .execute(&mut *transaction)
            .await?;
        }
        let result = sqlx::query(
            "UPDATE workflow_runs SET status='completed',manifest_path=?,summary_json=?,error=NULL,finished_at=? WHERE id=? AND status='running'",
        )
        .bind(manifest_path)
        .bind(summary_json)
        .bind(now())
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::Database(
                "科研工作流状态已变化，拒绝重复完成".into(),
            ));
        }
        transaction.commit().await?;
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

    #[allow(clippy::too_many_arguments)]
    pub async fn complete_agent_run(
        &self,
        turn_id: &str,
        run_id: &str,
        engine_turn_id: &str,
        engine_session_id: Option<&str>,
        response: &str,
        input_tokens: i64,
        output_tokens: i64,
        reasoning_tokens: i64,
        manifest_path: &str,
        summary_json: &str,
        artifacts: &[WorkflowArtifact],
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        for artifact in artifacts {
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
            .execute(&mut *transaction)
            .await?;
        }
        let finished_at = now();
        let turn_result = sqlx::query(
            "UPDATE workflow_agent_turns SET status='completed',engine_turn_id=?,engine_session_id=?,response=?,error=NULL,input_tokens=?,output_tokens=?,reasoning_tokens=?,finished_at=? WHERE id=? AND run_id=? AND status='running'",
        )
        .bind(engine_turn_id)
        .bind(engine_session_id)
        .bind(response)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(reasoning_tokens)
        .bind(&finished_at)
        .bind(turn_id)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        if turn_result.rows_affected() != 1 {
            return Err(AppError::Database(
                "科研智能体回合状态已变化，拒绝重复完成".into(),
            ));
        }
        let run_result = sqlx::query(
            "UPDATE workflow_runs SET status='completed',manifest_path=?,summary_json=?,error=NULL,finished_at=? WHERE id=? AND status='running'",
        )
        .bind(manifest_path)
        .bind(summary_json)
        .bind(finished_at)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        if run_result.rows_affected() != 1 {
            return Err(AppError::Database(
                "科研工作流状态已变化，拒绝重复完成".into(),
            ));
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn fail_agent_run(
        &self,
        turn_id: &str,
        run_id: &str,
        status: &str,
        error: &str,
    ) -> AppResult<()> {
        if !matches!(status, "failed" | "cancelled" | "interrupted") {
            return Err(AppError::Internal("无效的工作流终止状态".into()));
        }
        let mut transaction = self.pool.begin().await?;
        let finished_at = now();
        sqlx::query(
            "UPDATE workflow_agent_turns SET status=?,error=?,finished_at=? WHERE id=? AND run_id=? AND status='running'",
        )
        .bind(status)
        .bind(error)
        .bind(&finished_at)
        .bind(turn_id)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE workflow_runs SET status=?,error=?,finished_at=? WHERE id=? AND status='running'",
        )
        .bind(status)
        .bind(error)
        .bind(finished_at)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn list_agent_turns(&self, project_id: &str) -> AppResult<Vec<WorkflowAgentTurn>> {
        Ok(sqlx::query_as::<_, WorkflowAgentTurn>(
            "SELECT id,run_id,project_id,engine_turn_id,engine_session_id,provider,model,prompt,response,status,error,input_tokens,output_tokens,reasoning_tokens,created_at,finished_at \
             FROM workflow_agent_turns WHERE project_id=? ORDER BY created_at DESC LIMIT 100",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn start_bridge_event(
        &self,
        id: &str,
        project_id: &str,
        artifact_id: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO workflow_bridge_events(id,project_id,artifact_id,direction,status,created_at) \
             VALUES(?,?,?,'workflow_to_qa','initiated',?)",
        )
        .bind(id)
        .bind(project_id)
        .bind(artifact_id)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish_bridge_event(
        &self,
        id: &str,
        status: &str,
        remote_ref: Option<&str>,
        error: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE workflow_bridge_events SET status=?,remote_ref=?,error=?,finished_at=? WHERE id=?",
        )
        .bind(status)
        .bind(remote_ref)
        .bind(error)
        .bind(now())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
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

    fn test_project(root: &Path, id: &str, stamp: &str) -> WorkflowProject {
        WorkflowProject {
            id: id.into(),
            name: "Project".into(),
            root: root.join(id).to_string_lossy().into_owned(),
            created_at: stamp.into(),
            updated_at: stamp.into(),
        }
    }

    fn running_agent_records(
        project: &WorkflowProject,
        run_id: &str,
        turn_id: &str,
        stamp: &str,
    ) -> (WorkflowRun, WorkflowAgentTurn) {
        (
            WorkflowRun {
                id: run_id.into(),
                project_id: project.id.clone(),
                workflow_kind: "wisp-agent".into(),
                status: "running".into(),
                input_path: None,
                manifest_path: None,
                summary_json: "{}".into(),
                error: None,
                created_at: stamp.into(),
                started_at: Some(stamp.into()),
                finished_at: None,
            },
            WorkflowAgentTurn {
                id: turn_id.into(),
                run_id: run_id.into(),
                project_id: project.id.clone(),
                engine_turn_id: None,
                engine_session_id: None,
                provider: "openai".into(),
                model: "test".into(),
                prompt: "test".into(),
                response: String::new(),
                status: "running".into(),
                error: None,
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: 0,
                created_at: stamp.into(),
                finished_at: None,
            },
        )
    }

    #[tokio::test]
    async fn orphaned_runs_become_interrupted_without_touching_completed_runs() {
        let root =
            std::env::temp_dir().join(format!("rice-workflow-store-{}", uuid::Uuid::new_v4()));
        let store = WorkflowStore::open(&root).await.unwrap();
        let stamp = now();
        let project = test_project(&root, "project-1", &stamp);
        store.insert_project(&project).await.unwrap();
        let (running, turn) = running_agent_records(&project, "running", "turn-1", &stamp);
        store.begin_agent_run(&running, &turn).await.unwrap();
        store
            .insert_run(&WorkflowRun {
                id: "done".into(),
                project_id: project.id.clone(),
                workflow_kind: "test".into(),
                status: "completed".into(),
                input_path: None,
                manifest_path: None,
                summary_json: "{}".into(),
                error: None,
                created_at: stamp.clone(),
                started_at: Some(stamp.clone()),
                finished_at: Some(stamp),
            })
            .await
            .unwrap();
        assert!(store.delete_project(&project.id).await.is_err());
        store.interrupt_orphaned_runs().await.unwrap();
        assert_eq!(store.run("running").await.unwrap().status, "interrupted");
        assert_eq!(store.run("done").await.unwrap().status, "completed");
        assert_eq!(
            store.list_agent_turns(&project.id).await.unwrap()[0].status,
            "interrupted"
        );
        let bridge_run = WorkflowRun {
            id: "bridge-run".into(),
            project_id: project.id.clone(),
            workflow_kind: "counts-pca".into(),
            status: "running".into(),
            input_path: None,
            manifest_path: None,
            summary_json: "{}".into(),
            error: None,
            created_at: now(),
            started_at: Some(now()),
            finished_at: None,
        };
        store.insert_run(&bridge_run).await.unwrap();
        store
            .complete_run(
                &bridge_run.id,
                "manifest.json",
                "{}",
                &[WorkflowArtifact {
                    id: "artifact-1".into(),
                    run_id: bridge_run.id.clone(),
                    project_id: project.id.clone(),
                    name: "report.md".into(),
                    relative_path: "reports/report.md".into(),
                    media_type: "text/markdown".into(),
                    size_bytes: 4,
                    sha256: "abcd".into(),
                    created_at: now(),
                }],
            )
            .await
            .unwrap();
        store
            .start_bridge_event("bridge-1", &project.id, "artifact-1")
            .await
            .unwrap();
        store
            .finish_bridge_event("bridge-1", "completed", Some("tmp-1"), None)
            .await
            .unwrap();
        let bridge_status: String =
            sqlx::query_scalar("SELECT status FROM workflow_bridge_events WHERE id='bridge-1'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(bridge_status, "completed");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_completion_is_atomic_when_artifact_registration_fails() {
        let root = std::env::temp_dir().join(format!(
            "rice-workflow-transaction-{}",
            uuid::Uuid::new_v4()
        ));
        let store = WorkflowStore::open(&root).await.unwrap();
        let stamp = now();
        let project = test_project(&root, "project-atomic", &stamp);
        store.insert_project(&project).await.unwrap();
        let (run, turn) = running_agent_records(&project, "run-atomic", "turn-atomic", &stamp);
        store.begin_agent_run(&run, &turn).await.unwrap();
        let artifact = WorkflowArtifact {
            id: "duplicate-artifact".into(),
            run_id: run.id.clone(),
            project_id: project.id.clone(),
            name: "report.md".into(),
            relative_path: "reports/report.md".into(),
            media_type: "text/markdown".into(),
            size_bytes: 4,
            sha256: "abcd".into(),
            created_at: stamp,
        };
        let result = store
            .complete_agent_run(
                &turn.id,
                &run.id,
                "engine-turn",
                Some("engine-session"),
                "answer",
                1,
                2,
                0,
                "manifest.json",
                "{}",
                &[artifact.clone(), artifact],
            )
            .await;
        assert!(result.is_err());
        assert_eq!(store.run(&run.id).await.unwrap().status, "running");
        assert_eq!(
            store.list_agent_turns(&project.id).await.unwrap()[0].status,
            "running"
        );
        assert!(store.list_artifacts(&project.id).await.unwrap().is_empty());
        store
            .fail_agent_run(&turn.id, &run.id, "failed", "persistence failed")
            .await
            .unwrap();
        assert_eq!(store.run(&run.id).await.unwrap().status, "failed");
        let _ = std::fs::remove_dir_all(root);
    }
}
