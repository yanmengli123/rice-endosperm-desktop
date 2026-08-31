use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::{WorkflowAgentCompletion, WorkflowArtifact, WorkflowProject};

const MAX_CHANGED_PATHS: usize = 512;

#[derive(Serialize)]
struct AgentManifest<'a> {
    schema: &'static str,
    workflow: &'static str,
    workflow_version: &'static str,
    run_id: &'a str,
    project_id: &'a str,
    created_at: String,
    engine: AgentEngine<'a>,
    usage: AgentUsage,
    outputs: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct AgentEngine<'a> {
    name: &'static str,
    protocol: &'static str,
    provider: &'a str,
    model: &'a str,
    turn_id: &'a str,
    session_id: Option<&'a str>,
}

#[derive(Serialize)]
struct AgentUsage {
    input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
}

#[derive(Serialize)]
struct ManifestFile {
    path: String,
    size_bytes: u64,
    sha256: String,
    media_type: String,
}

pub fn sha256_file(path: &Path) -> AppResult<(u64, String)> {
    let mut file = File::open(path)
        .map_err(|error| AppError::Internal(format!("无法读取工作流产物：{error}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| AppError::Internal(format!("无法校验工作流产物：{error}")))?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    Ok((size, format!("{digest:x}")))
}

pub fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "pdf" => "application/pdf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

fn authorized_output(root: &Path, raw: &str) -> Option<(PathBuf, String)> {
    let canonical_root = std::fs::canonicalize(root).ok()?;
    let candidate = Path::new(raw);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        canonical_root.join(candidate)
    };
    let canonical = std::fs::canonicalize(candidate).ok()?;
    if !canonical.is_file() || !canonical.starts_with(&canonical_root) {
        return None;
    }
    let relative = canonical.strip_prefix(&canonical_root).ok()?;
    let mut components = relative.components();
    let top = components.next()?.as_os_str().to_string_lossy();
    if top != "results" && top != "reports" {
        return None;
    }
    let relative = relative.to_string_lossy().replace('\\', "/");
    Some((canonical, relative))
}

pub fn register_agent_outputs(
    project: &WorkflowProject,
    run_id: &str,
    provider: &str,
    model: &str,
    completion: &WorkflowAgentCompletion,
) -> AppResult<(String, Vec<WorkflowArtifact>)> {
    let root = Path::new(&project.root);
    let mut selected = BTreeSet::new();
    for raw in completion.changed_paths.iter().take(MAX_CHANGED_PATHS) {
        if let Some((_, relative)) = authorized_output(root, raw) {
            selected.insert(relative);
        }
    }

    let created_at = Utc::now().to_rfc3339();
    let mut artifacts = Vec::with_capacity(selected.len());
    let mut outputs = Vec::with_capacity(selected.len());
    for relative in selected {
        let Some((real, normalized)) = authorized_output(root, &relative) else {
            continue;
        };
        let (size_bytes, sha256) = sha256_file(&real)?;
        let kind = media_type(&real).to_owned();
        let name = real
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("artifact")
            .to_owned();
        artifacts.push(WorkflowArtifact {
            id: format!("wfa_{}", Uuid::new_v4().simple()),
            run_id: run_id.to_owned(),
            project_id: project.id.clone(),
            name,
            relative_path: normalized.clone(),
            media_type: kind.clone(),
            size_bytes: i64::try_from(size_bytes).unwrap_or(i64::MAX),
            sha256: sha256.clone(),
            created_at: created_at.clone(),
        });
        outputs.push(ManifestFile {
            path: normalized,
            size_bytes,
            sha256,
            media_type: kind,
        });
    }

    let manifest_relative = format!(".rice-workflow/runs/{run_id}/workflow-manifest.json");
    let manifest = AgentManifest {
        schema: "rice.workflow.manifest.v1",
        workflow: "wisp-agent",
        workflow_version: "1.0.0",
        run_id,
        project_id: &project.id,
        created_at,
        engine: AgentEngine {
            name: "wisp",
            protocol: "wisp.agent-rpc.v1",
            provider,
            model,
            turn_id: &completion.turn_id,
            session_id: completion.session_id.as_deref(),
        },
        usage: AgentUsage {
            input_tokens: completion.input_tokens,
            output_tokens: completion.output_tokens,
            reasoning_tokens: completion.reasoning_tokens,
        },
        outputs,
    };
    let manifest_path = root.join(&manifest_relative);
    let parent = manifest_path
        .parent()
        .ok_or_else(|| AppError::Internal("工作流清单目录无效".into()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| AppError::Internal(format!("无法创建工作流清单目录：{error}")))?;
    let temporary = parent.join(format!(".manifest-{}.tmp", Uuid::new_v4().simple()));
    let encoded = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| AppError::Internal(format!("无法编码工作流清单：{error}")))?;
    File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(&encoded)?;
            file.sync_all()
        })
        .map_err(|error| AppError::Internal(format!("无法保存工作流清单：{error}")))?;
    std::fs::rename(&temporary, &manifest_path)
        .map_err(|error| AppError::Internal(format!("无法提交工作流清单：{error}")))?;
    Ok((manifest_relative, artifacts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_only_results_and_reports() {
        let root = std::env::temp_dir().join(format!("rice-artifacts-{}", Uuid::new_v4()));
        for directory in [
            "input",
            "work",
            "results",
            "reports",
            "scripts",
            ".rice-workflow",
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        std::fs::write(root.join("results/out.csv"), "gene,value\nOs01g,1\n").unwrap();
        std::fs::write(root.join("work/private.txt"), "not an artifact").unwrap();
        let stamp = Utc::now().to_rfc3339();
        let project = WorkflowProject {
            id: "p1".into(),
            name: "P".into(),
            root: root.to_string_lossy().into_owned(),
            created_at: stamp.clone(),
            updated_at: stamp,
        };
        let completion = WorkflowAgentCompletion {
            turn_id: "t1".into(),
            text: "done".into(),
            session_id: None,
            input_tokens: 1,
            output_tokens: 2,
            reasoning_tokens: 0,
            changed_paths: vec!["results/out.csv".into(), "work/private.txt".into()],
        };
        let (manifest, artifacts) =
            register_agent_outputs(&project, "r1", "openai", "m", &completion).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].relative_path, "results/out.csv");
        assert!(root.join(manifest).is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
