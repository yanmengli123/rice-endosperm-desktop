use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::WorkflowProject;

pub const PROJECT_DIRECTORIES: [&str; 6] = [
    "input",
    "work",
    "results",
    "reports",
    "scripts",
    ".rice-workflow",
];

fn canonical_home_directories() -> Vec<PathBuf> {
    ["USERPROFILE", "HOME"]
        .into_iter()
        .filter_map(std::env::var_os)
        .filter_map(|value| std::fs::canonicalize(PathBuf::from(value)).ok())
        .collect()
}

pub fn validate_project_root(root: &Path) -> AppResult<PathBuf> {
    if !root.is_dir() {
        return Err(AppError::Internal("请选择已经存在的普通文件夹".into()));
    }
    let metadata = std::fs::symlink_metadata(root)
        .map_err(|error| AppError::Internal(format!("无法读取项目目录：{error}")))?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::Internal("项目根目录不能是符号链接".into()));
    }
    let canonical = std::fs::canonicalize(root)
        .map_err(|error| AppError::Internal(format!("无法解析项目目录：{error}")))?;
    if canonical.parent().is_none() || canonical == Path::new("/") {
        return Err(AppError::Internal("不能把磁盘根目录设为科研项目".into()));
    }
    if canonical_home_directories()
        .iter()
        .any(|home| home == &canonical)
    {
        return Err(AppError::Internal("不能把用户主目录设为科研项目".into()));
    }
    Ok(canonical)
}

pub fn initialize_project(root: &Path, name: Option<&str>) -> AppResult<WorkflowProject> {
    let canonical = validate_project_root(root)?;
    for directory in PROJECT_DIRECTORIES {
        std::fs::create_dir_all(canonical.join(directory)).map_err(|error| {
            AppError::Internal(format!("无法创建项目目录 {directory}：{error}"))
        })?;
    }
    let fallback_name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("科研项目");
    let display_name = name.unwrap_or(fallback_name).trim();
    if display_name.is_empty() || display_name.chars().count() > 80 {
        return Err(AppError::Internal("项目名称必须为 1–80 个字符".into()));
    }
    let stamp = Utc::now().to_rfc3339();
    Ok(WorkflowProject {
        id: format!("wfp_{}", Uuid::new_v4().simple()),
        name: display_name.to_owned(),
        root: canonical.to_string_lossy().into_owned(),
        created_at: stamp.clone(),
        updated_at: stamp,
    })
}

pub fn resolve_project_relative(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let path = Path::new(relative);
    if relative.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AppError::Internal("项目相对路径无效".into()));
    }
    let canonical_root = validate_project_root(root)?;
    let candidate = std::fs::canonicalize(canonical_root.join(path))
        .map_err(|error| AppError::Internal(format!("找不到项目文件：{error}")))?;
    if !candidate.starts_with(&canonical_root) {
        return Err(AppError::Internal("文件路径越过了项目边界".into()));
    }
    Ok(candidate)
}

pub fn resolve_input_file(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let canonical_root = validate_project_root(root)?;
    let input_root = std::fs::canonicalize(canonical_root.join("input"))
        .map_err(|error| AppError::Internal(format!("无法读取 input 目录：{error}")))?;
    let file = resolve_project_relative(&canonical_root, relative)?;
    if !file.starts_with(&input_root) || !file.is_file() {
        return Err(AppError::Internal(
            "分析输入必须是项目 input/ 目录中的普通文件".into(),
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_layout_and_rejects_parent_escape() {
        let root = std::env::temp_dir().join(format!("rice-project-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let project = initialize_project(&root, Some("Endosperm PCA")).unwrap();
        assert_eq!(project.name, "Endosperm PCA");
        for directory in PROJECT_DIRECTORIES {
            assert!(root.join(directory).is_dir());
        }
        assert!(resolve_project_relative(&root, "../outside.csv").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn input_resolver_refuses_files_outside_input() {
        let root = std::env::temp_dir().join(format!("rice-project-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        initialize_project(&root, None).unwrap();
        std::fs::write(root.join("input/counts.csv"), "gene,a,b\ng1,1,2\n").unwrap();
        std::fs::write(root.join("work/other.csv"), "x").unwrap();
        assert!(resolve_input_file(&root, "input/counts.csv").is_ok());
        assert!(resolve_input_file(&root, "work/other.csv").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
