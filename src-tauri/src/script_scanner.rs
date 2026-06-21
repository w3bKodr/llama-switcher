//! Scan a folder and turn `start - {model} - {feature}.ext` files into profiles.

use crate::alias_formatter::{build_alias, build_id, pretty_feature, pretty_model};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub raw_model: String,
    pub raw_feature: String,
    pub pretty_model: String,
    pub pretty_feature: String,
    pub alias: String,
    pub display_name: String,
    pub script_path: String,
    pub working_directory: String,
    pub extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoredFile {
    pub filename: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub profiles: Vec<Profile>,
    pub ignored_files: Vec<IgnoredFile>,
    pub scanned_at: String,
}

impl Default for ScanResult {
    fn default() -> Self {
        ScanResult {
            profiles: vec![],
            ignored_files: vec![],
            scanned_at: now_iso(),
        }
    }
}

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Split a `start - {model} - {feature}` style pattern into the literal parts
/// surrounding the placeholders.
struct PatternParts {
    before: String,
    between: String,
    after: String,
}

fn parse_pattern(pattern: &str) -> Option<PatternParts> {
    let model_idx = pattern.find("{model}")?;
    let feature_idx = pattern.find("{feature}")?;
    if feature_idx < model_idx {
        return None;
    }
    let before = pattern[..model_idx].to_string();
    let between = pattern[model_idx + "{model}".len()..feature_idx].to_string();
    let after = pattern[feature_idx + "{feature}".len()..].to_string();
    Some(PatternParts {
        before,
        between,
        after,
    })
}

/// Try to extract (raw_model, raw_feature) from a filename stem.
fn match_stem(stem: &str, parts: &PatternParts) -> Option<(String, String)> {
    let mut inner = stem;
    if !parts.before.is_empty() {
        inner = inner.strip_prefix(&parts.before)?;
    }
    if !parts.after.is_empty() {
        inner = inner.strip_suffix(&parts.after)?;
    }
    let between = if parts.between.is_empty() { " - " } else { &parts.between };
    let sep = inner.find(between)?;
    let model = inner[..sep].trim().to_string();
    let feature = inner[sep + between.len()..].trim().to_string();
    if model.is_empty() || feature.is_empty() {
        return None;
    }
    Some((model, feature))
}

pub fn scan(settings: &Settings) -> ScanResult {
    let folder = Path::new(&settings.scripts_folder);
    let mut profiles: Vec<Profile> = vec![];
    let mut ignored: Vec<IgnoredFile> = vec![];

    let parts = match parse_pattern(&settings.scan_pattern) {
        Some(p) => p,
        None => {
            return ScanResult {
                profiles,
                ignored_files: vec![IgnoredFile {
                    filename: settings.scan_pattern.clone(),
                    reason: "Invalid scan pattern (need {model} and {feature})".into(),
                }],
                scanned_at: now_iso(),
            };
        }
    };

    let allowed: Vec<String> = settings
        .allowed_extensions
        .iter()
        .map(|e| e.to_lowercase())
        .collect();

    let read_dir = match std::fs::read_dir(folder) {
        Ok(rd) => rd,
        Err(e) => {
            return ScanResult {
                profiles,
                ignored_files: vec![IgnoredFile {
                    filename: settings.scripts_folder.clone(),
                    reason: format!("Cannot read scripts folder: {}", e),
                }],
                scanned_at: now_iso(),
            };
        }
    };

    // Track ids to safely de-duplicate aliases across files.
    let mut id_counts: HashMap<String, u32> = HashMap::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();

        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();

        if !allowed.contains(&ext) {
            ignored.push(IgnoredFile {
                filename,
                reason: "Unsupported extension".into(),
            });
            continue;
        }

        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        match match_stem(&stem, &parts) {
            Some((raw_model, raw_feature)) => {
                let pm = pretty_model(&raw_model);
                let pf = pretty_feature(&raw_feature);
                let alias = build_alias(&pm, &pf);
                let base_id = build_id(&raw_model, &raw_feature);

                // De-duplicate identical ids by appending a short hash suffix.
                let count = id_counts.entry(base_id.clone()).or_insert(0);
                let id = if *count == 0 {
                    base_id.clone()
                } else {
                    format!("{}-{}", base_id, short_hash(&path.to_string_lossy()))
                };
                *count += 1;

                profiles.push(Profile {
                    id,
                    raw_model,
                    raw_feature,
                    pretty_model: pm,
                    pretty_feature: pf,
                    alias: alias.clone(),
                    display_name: alias,
                    script_path: path.to_string_lossy().to_string(),
                    working_directory: settings.scripts_folder.clone(),
                    extension: ext,
                });
            }
            None => {
                ignored.push(IgnoredFile {
                    filename,
                    reason: "Does not match naming pattern".into(),
                });
            }
        }
    }

    // Stable, human-friendly ordering: by model then feature.
    profiles.sort_by(|a, b| {
        a.pretty_model
            .to_lowercase()
            .cmp(&b.pretty_model.to_lowercase())
            .then(a.pretty_feature.to_lowercase().cmp(&b.pretty_feature.to_lowercase()))
    });

    ScanResult {
        profiles,
        ignored_files: ignored,
        scanned_at: now_iso(),
    }
}

/// Tiny non-cryptographic hash rendered as 6 hex chars, for id disambiguation.
fn short_hash(s: &str) -> String {
    let mut hash: u64 = 1469598103934665603; // FNV-1a offset basis
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{:06x}", (hash & 0xffffff))
}
