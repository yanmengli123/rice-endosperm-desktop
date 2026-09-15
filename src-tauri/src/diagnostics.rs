use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::Utc;

const MAX_LOG_SIZE: u64 = 1024 * 1024;
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static PANIC_REPORTED: AtomicBool = AtomicBool::new(false);

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|value| format!("{}:{}", value.file(), value.line()))
            .unwrap_or_else(|| "unknown".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|value| (*value).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        log("ERROR", "panic", &format!("{location}: {payload}"));

        if !PANIC_REPORTED.swap(true, Ordering::SeqCst) {
            show_fatal_dialog("程序遇到了意外错误，已安全停止。", current_log_path());
        }
    }));
}

pub fn initialize(app_data_dir: &Path, version: &str) -> Result<(), std::io::Error> {
    let log_dir = app_data_dir.join("logs");
    fs::create_dir_all(&log_dir)?;
    let path = log_dir.join("startup.log");
    rotate_if_needed(&path)?;
    let _ = LOG_PATH.set(path);
    log("INFO", "startup", &format!("version={version}"));
    Ok(())
}

pub fn log(level: &str, event: &str, detail: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let detail = redact_api_keys(detail).replace(['\r', '\n'], " ");
    let _ = writeln!(
        file,
        "{} [{level}] {event}: {detail}",
        Utc::now().to_rfc3339()
    );
}

pub fn report_startup_error(error: &str) {
    log("ERROR", "startup_failed", error);
    show_fatal_dialog(
        "启动初始化失败。程序没有删除你的会话数据，请查看诊断日志或重新安装最新版。",
        current_log_path(),
    );
}

fn current_log_path() -> Option<&'static Path> {
    LOG_PATH.get().map(PathBuf::as_path)
}

fn rotate_if_needed(path: &Path) -> Result<(), std::io::Error> {
    if path.metadata().map(|value| value.len()).unwrap_or(0) <= MAX_LOG_SIZE {
        return Ok(());
    }
    let previous = path.with_file_name("startup.previous.log");
    if previous.exists() {
        fs::remove_file(&previous)?;
    }
    fs::rename(path, previous)
}

/// 凭据脱敏：覆盖静态 Key（`yxkey_`）、刷新令牌（`yxrt_`）、激活码（`yxact_`）
/// 与 JWT（`Bearer` 头或 JSON 值中的三段 base64url 令牌）。
/// JWT 按「三段、每段 ≥8 个 base64url 字符」识别，避免误伤普通点分文本。
fn redact_api_keys(value: &str) -> String {
    let without_prefixed = redact_prefixed_tokens(value, &["yxkey_", "yxrt_", "yxact_"]);
    redact_jwt_shaped_runs(&without_prefixed)
}

fn redact_prefixed_tokens(value: &str, prefixes: &[&str]) -> String {
    let mut result = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(index) = find_earliest(remaining, prefixes) {
        let prefix = prefixes
            .iter()
            .find(|prefix| remaining[index..].starts_with(*prefix))
            .copied()
            .unwrap_or(prefixes[0]);
        result.push_str(&remaining[..index]);
        result.push_str(prefix);
        result.push_str("[REDACTED]");
        let secret = &remaining[index + prefix.len()..];
        let end = secret
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || character == '_' || character == '-')
            })
            .unwrap_or(secret.len());
        remaining = &secret[end..];
    }
    result.push_str(remaining);
    result
}

fn find_earliest(value: &str, prefixes: &[&str]) -> Option<usize> {
    prefixes
        .iter()
        .filter_map(|prefix| value.find(prefix))
        .min()
}

fn redact_jwt_shaped_runs(value: &str) -> String {
    fn is_token_char(character: char) -> bool {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
    }

    let mut result = String::with_capacity(value.len());
    let mut run = String::new();
    for character in value.chars() {
        if is_token_char(character) {
            run.push(character);
            continue;
        }
        flush_jwt_run(&mut result, &mut run);
        result.push(character);
    }
    flush_jwt_run(&mut result, &mut run);
    result
}

fn flush_jwt_run(result: &mut String, run: &mut String) {
    if is_jwt_shaped(run) {
        result.push_str("[REDACTED]");
    } else {
        result.push_str(run);
    }
    run.clear();
}

fn is_jwt_shaped(run: &str) -> bool {
    // 字符校验在段内进行：点由外层 split 排除，若对整段 run 校验，
    // 分隔点永远不在 base64url 白名单里，任何真 JWT 都会被漏放。
    let segments: Vec<&str> = run.split('.').collect();
    segments.len() == 3
        && segments.iter().all(|segment| {
            (8..=4096).contains(&segment.len())
                && segment.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        })
}

#[cfg(windows)]
fn show_fatal_dialog(message: &str, log_path: Option<&Path>) {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    const MB_OK: u32 = 0;
    const MB_ICONERROR: u32 = 0x10;
    const MB_SETFOREGROUND: u32 = 0x0001_0000;

    let detail = match log_path {
        Some(path) => format!("{message}\n\n诊断日志：{}", path.display()),
        None => message.to_owned(),
    };
    let text = std::ffi::OsStr::new(&detail)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let caption = std::ffi::OsStr::new("稻芯智析 - 启动失败")
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both strings are NUL-terminated and remain alive for the duration
    // of this synchronous Win32 call. A null owner is valid for a fatal dialog.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(windows))]
fn show_fatal_dialog(message: &str, log_path: Option<&Path>) {
    eprintln!("{message}");
    if let Some(path) = log_path {
        eprintln!("diagnostic log: {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::redact_api_keys;

    #[test]
    fn redacts_api_keys_from_diagnostics() {
        assert_eq!(
            redact_api_keys("request yxkey_1234567890abcdef failed"),
            "request yxkey_[REDACTED] failed"
        );
        assert_eq!(redact_api_keys("ordinary error"), "ordinary error");
    }

    #[test]
    fn redacts_refresh_and_activation_tokens() {
        assert_eq!(
            redact_api_keys("refresh yxrt_deadbeef1234 rejected"),
            "refresh yxrt_[REDACTED] rejected"
        );
        assert_eq!(
            redact_api_keys("code yxact_99887766 invalid"),
            "code yxact_[REDACTED] invalid"
        );
    }

    #[test]
    fn redacts_bearer_jwt_without_touching_plain_text() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI3Iiwic2lkIjoiZjEifQ.c2lnbmF0dXJlLXNlZ21lbnQ";
        assert_eq!(
            redact_api_keys(&format!("Bearer {jwt} dropped")),
            "Bearer [REDACTED] dropped"
        );
        // JSON 值中的令牌同样被清除
        assert_eq!(
            redact_api_keys(&format!(r#"{{"access_token":"{jwt}"}}"#)),
            r#"{"access_token":"[REDACTED]"}"#
        );
        // 普通点分文本（域名/版本号）不得误伤
        assert_eq!(
            redact_api_keys("connect api.example.cn.v1 failed"),
            "connect api.example.cn.v1 failed"
        );
        assert_eq!(redact_api_keys("a.b.c"), "a.b.c");
    }
}
