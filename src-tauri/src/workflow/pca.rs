use std::{fs::File, io::Write, path::Path};

use chrono::Utc;
use nalgebra::DMatrix;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::{PcaSummary, WorkflowArtifact};

const MAX_INPUT_BYTES: u64 = 200 * 1024 * 1024;
const MAX_SAMPLES: usize = 512;

pub struct PcaExecutionResult {
    pub summary: PcaSummary,
    pub manifest_relative_path: String,
    pub artifacts: Vec<WorkflowArtifact>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    schema: &'static str,
    workflow: &'static str,
    workflow_version: &'static str,
    run_id: &'a str,
    project_id: &'a str,
    created_at: String,
    input: ManifestFile,
    parameters: ManifestParameters,
    summary: &'a PcaSummary,
    outputs: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct ManifestParameters {
    transform: &'static str,
    centering: &'static str,
    decomposition: &'static str,
}

fn file_digest(path: &Path) -> AppResult<ManifestFile> {
    let bytes = std::fs::read(path)
        .map_err(|error| AppError::Internal(format!("无法读取产物：{error}")))?;
    let digest = Sha256::digest(&bytes);
    Ok(ManifestFile {
        path: path.to_string_lossy().replace('\\', "/"),
        size_bytes: bytes.len() as u64,
        sha256: format!("{digest:x}"),
    })
}

fn delimiter(path: &Path) -> AppResult<u8> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "tsv" | "txt") {
        return Ok(b'\t');
    }
    if extension != "csv" {
        return Err(AppError::Internal(
            "PCA 输入仅支持 .csv、.tsv 或制表符 .txt".into(),
        ));
    }
    Ok(b',')
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn deterministic_component(mut values: Vec<f64>) -> Vec<f64> {
    let pivot = values
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.abs()
                .partial_cmp(&right.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index)
        .unwrap_or(0);
    if values.get(pivot).is_some_and(|value| *value < 0.0) {
        for value in &mut values {
            *value = -*value;
        }
    }
    values
}

fn render_svg(
    path: &Path,
    samples: &[String],
    pc1: &[f64],
    pc2: &[f64],
    pc1_percent: f64,
    pc2_percent: f64,
) -> AppResult<()> {
    let width = 920.0;
    let height = 620.0;
    let margin = 86.0;
    let min_x = pc1.iter().copied().fold(f64::INFINITY, f64::min);
    let max_x = pc1.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min_y = pc2.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = pc2.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span_x = (max_x - min_x).abs().max(1e-9);
    let span_y = (max_y - min_y).abs().max(1e-9);
    let scale_x = |value: f64| margin + (value - min_x) / span_x * (width - 2.0 * margin);
    let scale_y = |value: f64| height - margin - (value - min_y) / span_y * (height - 2.0 * margin);
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<rect width="100%" height="100%" fill="#ffffff"/>
<text x="{margin}" y="42" font-family="Segoe UI,Arial" font-size="24" font-weight="700" fill="#173f2c">Counts matrix PCA</text>
<line x1="{margin}" y1="{}" x2="{}" y2="{}" stroke="#8ea399"/>
<line x1="{margin}" y1="{margin}" x2="{margin}" y2="{}" stroke="#8ea399"/>
<text x="{}" y="{}" text-anchor="middle" font-family="Segoe UI,Arial" font-size="14" fill="#476154">PC1 ({pc1_percent:.2}%)</text>
<text x="24" y="{}" text-anchor="middle" transform="rotate(-90 24 {})" font-family="Segoe UI,Arial" font-size="14" fill="#476154">PC2 ({pc2_percent:.2}%)</text>
"##,
        height - margin,
        width - margin,
        height - margin,
        height - margin,
        width / 2.0,
        height - 24.0,
        height / 2.0,
        height / 2.0,
    );
    for ((sample, x), y) in samples.iter().zip(pc1).zip(pc2) {
        let px = scale_x(*x);
        let py = scale_y(*y);
        svg.push_str(&format!(
            "<circle cx=\"{px:.2}\" cy=\"{py:.2}\" r=\"7\" fill=\"#2d7a50\" stroke=\"#ffffff\" stroke-width=\"2\"/><text x=\"{:.2}\" y=\"{:.2}\" font-family=\"Segoe UI,Arial\" font-size=\"12\" fill=\"#294638\">{}</text>\n",
            px + 10.0,
            py - 9.0,
            xml_escape(sample)
        ));
    }
    svg.push_str("</svg>\n");
    std::fs::write(path, svg)
        .map_err(|error| AppError::Internal(format!("无法写入 PCA 图：{error}")))
}

pub fn execute_counts_pca(
    project_id: &str,
    project_root: &Path,
    input_path: &Path,
    input_relative_path: &str,
    run_id: &str,
    cancellation: &CancellationToken,
) -> AppResult<PcaExecutionResult> {
    let metadata = std::fs::metadata(input_path)
        .map_err(|error| AppError::Internal(format!("无法读取输入文件：{error}")))?;
    if metadata.len() == 0 || metadata.len() > MAX_INPUT_BYTES {
        return Err(AppError::Internal(
            "输入矩阵必须非空且不能超过 200 MB".into(),
        ));
    }
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter(input_path)?)
        .flexible(false)
        .from_path(input_path)
        .map_err(|error| AppError::Internal(format!("无法解析表达矩阵：{error}")))?;
    let headers = reader
        .headers()
        .map_err(|error| AppError::Internal(format!("无法读取矩阵表头：{error}")))?
        .clone();
    if headers.len() < 3 {
        return Err(AppError::Internal(
            "表达矩阵至少需要一列基因标识和两列样本".into(),
        ));
    }
    let samples = headers
        .iter()
        .skip(1)
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if samples.len() > MAX_SAMPLES
        || samples.iter().any(|sample| sample.is_empty())
        || samples
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != samples.len()
    {
        return Err(AppError::Internal(
            "样本名称必须唯一且非空，单次最多支持 512 个样本".into(),
        ));
    }
    let sample_count = samples.len();
    let mut gram = vec![0.0_f64; sample_count * sample_count];
    let mut feature_count = 0_usize;
    let mut variable_feature_count = 0_usize;
    for record in reader.records() {
        if feature_count.is_multiple_of(512) && cancellation.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let record =
            record.map_err(|error| AppError::Internal(format!("矩阵记录格式错误：{error}")))?;
        let gene = record.get(0).unwrap_or_default().trim();
        if gene.is_empty() {
            return Err(AppError::Internal(format!(
                "第 {} 个数据行缺少基因标识",
                feature_count + 2
            )));
        }
        let mut values = Vec::with_capacity(sample_count);
        for (sample_index, raw) in record.iter().skip(1).enumerate() {
            let count = raw.trim().parse::<f64>().map_err(|_| {
                AppError::Internal(format!(
                    "基因 {gene} 在样本 {} 中不是有效数字",
                    samples[sample_index]
                ))
            })?;
            if !count.is_finite() || count < 0.0 {
                return Err(AppError::Internal(format!(
                    "基因 {gene} 含负数或非有限计数"
                )));
            }
            values.push((count + 1.0).log2());
        }
        let mean = values.iter().sum::<f64>() / sample_count as f64;
        for value in &mut values {
            *value -= mean;
        }
        if values.iter().map(|value| value * value).sum::<f64>() > 1e-12 {
            variable_feature_count += 1;
            for row in 0..sample_count {
                for column in 0..sample_count {
                    gram[row * sample_count + column] += values[row] * values[column];
                }
            }
        }
        feature_count += 1;
    }
    if feature_count < 2 || variable_feature_count < 2 {
        return Err(AppError::Internal(
            "矩阵至少需要两个有效且在样本间存在变化的基因".into(),
        ));
    }
    let divisor = (variable_feature_count - 1) as f64;
    for value in &mut gram {
        *value /= divisor;
    }
    let eigen = DMatrix::from_row_slice(sample_count, sample_count, &gram).symmetric_eigen();
    let mut order = (0..sample_count).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        eigen.eigenvalues[*right]
            .partial_cmp(&eigen.eigenvalues[*left])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = eigen
        .eigenvalues
        .iter()
        .filter(|value| **value > 0.0)
        .sum::<f64>()
        .max(1e-12);
    let component = |rank: usize| {
        let index = order.get(rank).copied().unwrap_or(order[0]);
        let eigenvalue = eigen.eigenvalues[index].max(0.0);
        let values = (0..sample_count)
            .map(|row| eigen.eigenvectors[(row, index)] * eigenvalue.sqrt())
            .collect::<Vec<_>>();
        (deterministic_component(values), eigenvalue / total * 100.0)
    };
    let (pc1, pc1_percent) = component(0);
    let (pc2, pc2_percent) = component(1);
    let summary = PcaSummary {
        sample_count,
        feature_count,
        variable_feature_count,
        pc1_explained_percent: pc1_percent,
        pc2_explained_percent: pc2_percent,
    };

    let result_relative = format!("results/{run_id}");
    let report_relative = format!("reports/{run_id}");
    let result_dir = project_root.join(&result_relative);
    let report_dir = project_root.join(&report_relative);
    std::fs::create_dir_all(&result_dir)
        .and_then(|_| std::fs::create_dir_all(&report_dir))
        .map_err(|error| AppError::Internal(format!("无法创建运行输出目录：{error}")))?;

    let csv_path = result_dir.join("PCA.csv");
    let mut csv_writer = csv::Writer::from_path(&csv_path)
        .map_err(|error| AppError::Internal(format!("无法创建 PCA.csv：{error}")))?;
    csv_writer
        .write_record(["sample", "PC1", "PC2"])
        .map_err(|error| AppError::Internal(error.to_string()))?;
    for ((sample, x), y) in samples.iter().zip(&pc1).zip(&pc2) {
        csv_writer
            .write_record([sample, &format!("{x:.8}"), &format!("{y:.8}")])
            .map_err(|error| AppError::Internal(error.to_string()))?;
    }
    csv_writer
        .flush()
        .map_err(|error| AppError::Internal(error.to_string()))?;

    let svg_path = result_dir.join("PCA.svg");
    render_svg(&svg_path, &samples, &pc1, &pc2, pc1_percent, pc2_percent)?;
    let report_path = report_dir.join("report.md");
    let mut report = File::create(&report_path)
        .map_err(|error| AppError::Internal(format!("无法创建分析报告：{error}")))?;
    writeln!(report, "# 表达矩阵 PCA 报告\n")
        .and_then(|_| writeln!(report, "- 输入：`{input_relative_path}`"))
        .and_then(|_| writeln!(report, "- 样本数：{sample_count}"))
        .and_then(|_| writeln!(report, "- 基因数：{feature_count}"))
        .and_then(|_| writeln!(report, "- 参与 PCA 的变异基因数：{variable_feature_count}"))
        .and_then(|_| writeln!(report, "- PC1 解释率：{pc1_percent:.2}%"))
        .and_then(|_| writeln!(report, "- PC2 解释率：{pc2_percent:.2}%"))
        .and_then(|_| writeln!(report, "\n## 方法\n\n对原始非负计数执行 `log2(count + 1)`，按基因在样本间中心化，然后对样本 Gram 矩阵做对称特征分解。"))
        .map_err(|error| AppError::Internal(format!("无法写入分析报告：{error}")))?;

    let output_paths = [&csv_path, &svg_path, &report_path];
    let output_files = output_paths
        .iter()
        .map(|path| file_digest(path))
        .collect::<AppResult<Vec<_>>>()?;
    let input_file = file_digest(input_path)?;
    let manifest_relative_path = format!(".rice-workflow/runs/{run_id}/workflow-manifest.json");
    let manifest_path = project_root.join(&manifest_relative_path);
    std::fs::create_dir_all(manifest_path.parent().unwrap())
        .map_err(|error| AppError::Internal(format!("无法创建运行清单目录：{error}")))?;
    let manifest = Manifest {
        schema: "rice.workflow-manifest.v1",
        workflow: "counts-pca",
        workflow_version: "1.0.0",
        run_id,
        project_id,
        created_at: Utc::now().to_rfc3339(),
        input: ManifestFile {
            path: input_relative_path.replace('\\', "/"),
            ..input_file
        },
        parameters: ManifestParameters {
            transform: "log2(count + 1)",
            centering: "per-feature across samples",
            decomposition: "symmetric eigendecomposition of sample Gram matrix",
        },
        summary: &summary,
        outputs: output_files,
    };
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| AppError::Internal(error.to_string()))?,
    )
    .map_err(|error| AppError::Internal(format!("无法写入运行清单：{error}")))?;

    let media_types = ["text/csv", "image/svg+xml", "text/markdown"];
    let artifacts = output_paths
        .iter()
        .zip(media_types)
        .map(|(path, media_type)| {
            let digest = file_digest(path)?;
            let relative = path
                .strip_prefix(project_root)
                .map_err(|_| AppError::Internal("产物越过了项目边界".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            Ok(WorkflowArtifact {
                id: format!("wfa_{}", Uuid::new_v4().simple()),
                run_id: run_id.to_owned(),
                project_id: project_id.to_owned(),
                name: path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("artifact")
                    .to_owned(),
                relative_path: relative,
                media_type: media_type.to_owned(),
                size_bytes: i64::try_from(digest.size_bytes)
                    .map_err(|_| AppError::Internal("产物过大".into()))?,
                sha256: digest.sha256,
                created_at: Utc::now().to_rfc3339(),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(PcaExecutionResult {
        summary,
        manifest_relative_path,
        artifacts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pca_creates_reproducible_outputs_and_manifest() {
        let root = std::env::temp_dir().join(format!("rice-pca-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("input")).unwrap();
        std::fs::write(
            root.join("input/counts.csv"),
            "gene,S1,S2,S3\nOs01g1,10,20,12\nOs01g2,3,8,21\nOs01g3,90,45,11\n",
        )
        .unwrap();
        let result = execute_counts_pca(
            "project-1",
            &root,
            &root.join("input/counts.csv"),
            "input/counts.csv",
            "run-1",
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(result.summary.sample_count, 3);
        assert_eq!(result.summary.feature_count, 3);
        assert_eq!(result.artifacts.len(), 3);
        assert!(root.join("results/run-1/PCA.csv").is_file());
        assert!(root.join("results/run-1/PCA.svg").is_file());
        assert!(root.join(&result.manifest_relative_path).is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_run_stops_before_parsing() {
        let root = std::env::temp_dir().join(format!("rice-pca-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("input")).unwrap();
        std::fs::write(root.join("input/counts.csv"), "gene,A,B\ng1,1,2\ng2,2,3\n").unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            execute_counts_pca(
                "p",
                &root,
                &root.join("input/counts.csv"),
                "input/counts.csv",
                "r",
                &cancellation,
            ),
            Err(AppError::Cancelled)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
