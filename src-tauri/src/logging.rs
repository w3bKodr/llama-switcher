//! One log file per server run, stored under the app data `logs/` directory.

use crate::script_scanner::Profile;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub filename: String,
    pub path: String,
    pub modified_at: String,
    pub size_bytes: u64,
}

/// Create a fresh log file for a run and write the header. Returns its path.
pub fn create_run_log(logs_dir: &Path, profile: &Profile, pid: Option<u32>) -> PathBuf {
    let _ = std::fs::create_dir_all(logs_dir);
    let ts = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let path = logs_dir.join(format!("{}-{}.log", profile.id, ts));

    if let Ok(mut f) = File::create(&path) {
        let _ = writeln!(f, "==== Llama Switcher run log ====");
        let _ = writeln!(f, "alias:            {}", profile.alias);
        let _ = writeln!(f, "profileId:        {}", profile.id);
        let _ = writeln!(f, "model:            {}", profile.pretty_model);
        let _ = writeln!(f, "feature:          {}", profile.pretty_feature);
        let _ = writeln!(f, "scriptPath:       {}", profile.script_path);
        let _ = writeln!(f, "workingDirectory: {}", profile.working_directory);
        let _ = writeln!(
            f,
            "startTime:        {}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
        );
        if let Some(pid) = pid {
            let _ = writeln!(f, "pid:              {}", pid);
        }
        let _ = writeln!(f, "================================");
        let _ = writeln!(f, "---- process output ----");
    }
    path
}

/// Append a labelled line to a run log (used for stop/exit/health/errors).
pub fn append_line(path: &Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            f,
            "[{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            line
        );
    }
}

/// Open the log file for appending child stdout/stderr directly.
pub fn open_for_append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn list_logs(logs_dir: &Path) -> Vec<LogEntry> {
    let mut entries: Vec<LogEntry> = vec![];
    if let Ok(rd) = std::fs::read_dir(logs_dir) {
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("log") {
                continue;
            }
            let meta = match e.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified_at = meta
                .modified()
                .ok()
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Local> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_default();
            entries.push(LogEntry {
                filename: e.file_name().to_string_lossy().to_string(),
                path: path.to_string_lossy().to_string(),
                modified_at,
                size_bytes: meta.len(),
            });
        }
    }
    // Newest first.
    entries.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    entries
}

pub fn read_log(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

pub fn latest_log_path(logs_dir: &Path) -> Option<PathBuf> {
    list_logs(logs_dir).first().map(|e| PathBuf::from(&e.path))
}

/// Delete every log except the most recent one. Returns the count removed.
pub fn clear_old(logs_dir: &Path) -> usize {
    let logs = list_logs(logs_dir);
    let mut removed = 0;
    for entry in logs.into_iter().skip(1) {
        if std::fs::remove_file(&entry.path).is_ok() {
            removed += 1;
        }
    }
    removed
}
