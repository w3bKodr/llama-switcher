//! Benchmark runner: cycle selected models through a set of prompts and save
//! each model's actioned output. Model switching goes through
//! `process_manager::activate_profile`; prompts are sent to the server's OpenAI
//! chat endpoint with the reused API key. Runs on a background thread and emits
//! `benchmark-progress` events so the UI can render a live grid.

use crate::process_manager;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPrompt {
    pub id: String,
    pub title: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkConfig {
    pub profile_ids: Vec<String>,
    pub prompts: Vec<BenchmarkPrompt>,
    pub output_dir: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    600
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    /// "run" | "model" | "prompt"
    kind: String,
    /// running | done | error | switching | finished | cancelled
    status: String,
    profile_id: Option<String>,
    alias: Option<String>,
    prompt_id: Option<String>,
    output_path: Option<String>,
    message: Option<String>,
    /// Wall-clock seconds the prompt took (set on a "done" prompt event).
    duration_seconds: Option<f64>,
    /// Average generation tokens/sec reported by the server for this prompt.
    tokens_per_second: Option<f64>,
}

fn emit(app: &AppHandle, p: Progress) {
    let _ = app.emit("benchmark-progress", p);
}

// ---------------------------------------------------------------------------
// Config persistence (app-data/benchmark.json)
// ---------------------------------------------------------------------------

fn config_path(state: &Arc<AppState>) -> PathBuf {
    state
        .settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("benchmark.json")
}

pub fn load_config(state: &Arc<AppState>) -> BenchmarkConfig {
    if let Ok(text) = std::fs::read_to_string(config_path(state)) {
        if let Ok(cfg) = serde_json::from_str::<BenchmarkConfig>(&text) {
            return cfg;
        }
    }
    default_config()
}

pub fn save_config(state: &Arc<AppState>, config: &BenchmarkConfig) -> Result<(), String> {
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(config_path(state), text).map_err(|e| e.to_string())
}

pub fn default_config() -> BenchmarkConfig {
    BenchmarkConfig {
        profile_ids: vec![],
        prompts: vec![
            BenchmarkPrompt {
                id: "prompt1".into(),
                title: "Chess PGN → SVG".into(),
                text: CHESS_PROMPT.into(),
            },
            BenchmarkPrompt {
                id: "prompt2".into(),
                title: "Parallax car canvas".into(),
                text: CAR_PROMPT.into(),
            },
        ],
        output_dir: String::new(),
        timeout_seconds: 600,
    }
}

const CHESS_PROMPT: &str = "Given this PGN string of a chess game:\n\n1. b3 e5 2. Nf3 h5 3. d4 exd4 4. Nxd4 Nf6 5. f4 Ke7 6. Qd3 d5 7. h4 *\n\nFigure out the current state of the chessboard, create an image in SVG code, also highlight the last move.";

const CAR_PROMPT: &str = "Write a single HTML file with a full-page canvas and no libraries. Simulate a realistic side-view of a moving car as the main subject. Keep the car visible in the foreground while the background landscape scrolls continuously to create the feeling that the car is driving forward. Use layered scenery for depth: nearby ground, roadside elements, trees, poles, and distant hills or mountains should move at different speeds for a natural parallax effect. Animate the wheels spinning realistically and add subtle body motion so the car feels connected to the road. Let the environment pass smoothly behind it, with repeating but varied scenery that makes the movement feel believable. Use cinematic lighting and a cohesive sky, such as sunset, dusk, or daylight, to enhance atmosphere. The overall motion should feel calm, immersive, and realistic, with a seamless looping animation";

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

pub fn is_running(state: &Arc<AppState>) -> bool {
    *state.benchmark_running.lock().unwrap()
}

pub fn cancel(state: &Arc<AppState>) {
    *state.benchmark_cancel.lock().unwrap() = true;
}

fn cancelled(state: &Arc<AppState>) -> bool {
    *state.benchmark_cancel.lock().unwrap()
}

/// Validate + start a run on a background thread. Returns an error if a run is
/// already in progress or the config is invalid.
pub fn start(app: AppHandle, state: Arc<AppState>, config: BenchmarkConfig) -> Result<(), String> {
    if is_running(&state) {
        return Err("A benchmark is already running.".into());
    }
    if config.profile_ids.is_empty() {
        return Err("Select at least one model.".into());
    }
    if config.prompts.is_empty() {
        return Err("Add at least one prompt.".into());
    }
    if config.output_dir.trim().is_empty() {
        return Err("Choose an output folder.".into());
    }
    let _ = save_config(&state, &config);
    std::thread::spawn(move || run_inner(&app, &state, config));
    Ok(())
}

fn run_inner(app: &AppHandle, state: &Arc<AppState>, config: BenchmarkConfig) {
    *state.benchmark_running.lock().unwrap() = true;
    *state.benchmark_cancel.lock().unwrap() = false;
    emit(app, Progress {
        kind: "run".into(),
        status: "running".into(),
        profile_id: None,
        alias: None,
        prompt_id: None,
        output_path: None,
        message: None,
        duration_seconds: None,
        tokens_per_second: None,
    });

    let previous = state.status().current_profile_id.clone();
    let settings = state.settings_snapshot();

    if let Err(e) = std::fs::create_dir_all(&config.output_dir) {
        run_finished(app, state, previous, format!("Cannot create output folder: {}", e));
        return;
    }

    'models: for profile_id in &config.profile_ids {
        if cancelled(state) {
            break;
        }
        let profile = match state.find_profile(profile_id) {
            Some(p) => p,
            None => {
                emit_model(app, profile_id, None, "error", Some("Unknown profile".into()));
                continue;
            }
        };

        // If this model is already the running, healthy one, use it as-is —
        // no need to stop and relaunch the same server.
        let live = process_manager::status_with_probe(app, state);
        let already_active =
            live.current_profile_id.as_deref() == Some(profile_id.as_str()) && live.healthy;

        if !already_active {
            emit_model(app, profile_id, Some(&profile.alias), "switching", None);
            if let Err(e) = process_manager::activate_profile(app, state, profile_id) {
                emit_model(app, profile_id, Some(&profile.alias), "error", Some(e));
                continue;
            }
            if !wait_healthy(app, state, settings.health_check_timeout_seconds) {
                emit_model(
                    app,
                    profile_id,
                    Some(&profile.alias),
                    "error",
                    Some("Server did not become healthy in time.".into()),
                );
                continue;
            }
        }
        emit_model(app, profile_id, Some(&profile.alias), "running", None);

        let api_key = process_manager::resolve_api_key(state, Some(&profile.script_path));
        let origin = process_manager::server_origin(&settings.health_url, settings.server_port);
        let model_dir = Path::new(&config.output_dir).join(sanitize_alias(&profile.alias));

        for (i, prompt) in config.prompts.iter().enumerate() {
            if cancelled(state) {
                break 'models;
            }
            let prompt_dir = model_dir.join(format!("prompt{}", i + 1));
            emit_prompt(
                app, profile_id, &profile.alias, &prompt.id, "running", &prompt_dir, None, None,
                None,
            );

            match run_prompt(
                &origin,
                api_key.as_deref(),
                prompt,
                config.timeout_seconds,
                &prompt_dir,
                &profile.alias,
            ) {
                Ok((elapsed, tps)) => emit_prompt(
                    app,
                    profile_id,
                    &profile.alias,
                    &prompt.id,
                    "done",
                    &prompt_dir,
                    None,
                    Some(elapsed),
                    tps,
                ),
                Err(e) => emit_prompt(
                    app,
                    profile_id,
                    &profile.alias,
                    &prompt.id,
                    "error",
                    &prompt_dir,
                    Some(e),
                    None,
                    None,
                ),
            }
        }
    }

    let msg = if cancelled(state) {
        "cancelled".to_string()
    } else {
        "finished".to_string()
    };
    run_finished(app, state, previous, msg);
}

fn run_finished(app: &AppHandle, state: &Arc<AppState>, previous: Option<String>, status: String) {
    // Restore whatever was running before the benchmark — but only if it isn't
    // already the running model (e.g. the last benchmarked model was the
    // original), to avoid a pointless stop/relaunch.
    let current = state.status().current_profile_id;
    match previous {
        Some(id) if state.find_profile(&id).is_some() => {
            if current.as_deref() != Some(id.as_str()) {
                let _ = process_manager::activate_profile(app, state, &id);
            }
        }
        _ => {
            let _ = process_manager::stop_server(app, state);
        }
    }
    *state.benchmark_running.lock().unwrap() = false;
    *state.benchmark_cancel.lock().unwrap() = false;
    emit(app, Progress {
        kind: "run".into(),
        status,
        profile_id: None,
        alias: None,
        prompt_id: None,
        output_path: None,
        message: None,
        duration_seconds: None,
        tokens_per_second: None,
    });
    process_manager::notify(app, state);
}

fn wait_healthy(app: &AppHandle, state: &Arc<AppState>, timeout_s: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(timeout_s.max(1));
    while Instant::now() < deadline {
        if cancelled(state) {
            return false;
        }
        if process_manager::status_with_probe(app, state).healthy {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

fn emit_model(app: &AppHandle, profile_id: &str, alias: Option<&str>, status: &str, message: Option<String>) {
    emit(app, Progress {
        kind: "model".into(),
        status: status.into(),
        profile_id: Some(profile_id.into()),
        alias: alias.map(|a| a.to_string()),
        prompt_id: None,
        output_path: None,
        message,
        duration_seconds: None,
        tokens_per_second: None,
    });
}

#[allow(clippy::too_many_arguments)]
fn emit_prompt(
    app: &AppHandle,
    profile_id: &str,
    alias: &str,
    prompt_id: &str,
    status: &str,
    dir: &Path,
    message: Option<String>,
    duration_seconds: Option<f64>,
    tokens_per_second: Option<f64>,
) {
    emit(app, Progress {
        kind: "prompt".into(),
        status: status.into(),
        profile_id: Some(profile_id.into()),
        alias: Some(alias.into()),
        prompt_id: Some(prompt_id.into()),
        output_path: Some(dir.to_string_lossy().to_string()),
        message,
        duration_seconds,
        tokens_per_second,
    });
}

// ---------------------------------------------------------------------------
// One prompt against one model
// ---------------------------------------------------------------------------

fn run_prompt(
    origin: &str,
    api_key: Option<&str>,
    prompt: &BenchmarkPrompt,
    timeout_s: u64,
    prompt_dir: &Path,
    alias: &str,
) -> Result<(f64, Option<f64>), String> {
    std::fs::create_dir_all(prompt_dir).map_err(|e| e.to_string())?;

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_s.max(1)))
        .build();
    let url = format!("{}/v1/chat/completions", origin);
    let mut req = agent.post(&url);
    if let Some(key) = api_key {
        req = req.set("Authorization", &format!("Bearer {}", key));
    }

    let started = Instant::now();
    let payload = json!({
        "messages": [{ "role": "user", "content": prompt.text }],
        "stream": false
    });
    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let response = req
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| format!("Request failed: {}", e))?;
    let text = response.into_string().map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let elapsed = started.elapsed().as_secs_f64();

    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    // Raw reply.
    std::fs::write(prompt_dir.join("response.md"), &content).map_err(|e| e.to_string())?;

    // Extracted code artifacts.
    for (name, code) in map_code_files(&extract_code_blocks(&content)) {
        std::fs::write(prompt_dir.join(&name), code).map_err(|e| e.to_string())?;
    }

    // Metadata.
    let timings = &value["timings"];
    let usage = &value["usage"];
    let tokens_per_second = timings["predicted_per_second"].as_f64();
    let meta = json!({
        "alias": alias,
        "promptId": prompt.id,
        "promptTitle": prompt.title,
        "predictedTokens": timings["predicted_n"].as_f64().or_else(|| usage["completion_tokens"].as_f64()),
        "promptTokens": timings["prompt_n"].as_f64().or_else(|| usage["prompt_tokens"].as_f64()),
        "tokensPerSecond": tokens_per_second,
        "durationSeconds": elapsed,
        "finishReason": value["choices"][0]["finish_reason"].as_str(),
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    std::fs::write(
        prompt_dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;

    Ok((elapsed, tokens_per_second))
}

// ---------------------------------------------------------------------------
// Helpers: alias sanitizing + code-block extraction
// ---------------------------------------------------------------------------

/// `Qwen3.6-35B MTP` -> `Qwen3.6-35B-MTP`; strips filesystem-illegal chars.
pub fn sanitize_alias(alias: &str) -> String {
    let mut out = String::new();
    for c in alias.chars() {
        if c.is_whitespace() || "<>:\"/\\|?*".contains(c) {
            out.push('-');
        } else {
            out.push(c);
        }
    }
    out.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Extract ``` fenced code blocks as (lang, code) pairs.
fn extract_code_blocks(md: &str) -> Vec<(String, String)> {
    let mut blocks = vec![];
    let mut in_block = false;
    let mut lang = String::new();
    let mut buf = String::new();
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                blocks.push((lang.clone(), buf.clone()));
                in_block = false;
                buf.clear();
            } else {
                in_block = true;
                lang = trimmed.trim_start_matches('`').trim().to_string();
                buf.clear();
            }
        } else if in_block {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    blocks
}

/// Map code blocks to output filenames (html -> index.html, svg -> image.svg,
/// otherwise block{n}.<ext>), de-duplicating repeats.
fn map_code_files(blocks: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut used: HashSet<String> = HashSet::new();
    for (i, (lang, code)) in blocks.iter().enumerate() {
        let l = lang.to_lowercase();
        let head = code.trim_start().to_lowercase();
        let base = if l == "html" || l == "htm" || head.starts_with("<!doctype html") || head.starts_with("<html") {
            "index.html".to_string()
        } else if l == "svg" || head.starts_with("<svg") {
            "image.svg".to_string()
        } else {
            let ext: String = l.chars().filter(|c| c.is_alphanumeric()).collect();
            let ext = if ext.is_empty() { "txt".to_string() } else { ext };
            format!("block{}.{}", i + 1, ext)
        };
        out.push((dedupe_name(base, &mut used), code.clone()));
    }
    out
}

fn dedupe_name(name: String, used: &mut HashSet<String>) -> String {
    if used.insert(name.clone()) {
        return name;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{}", e)),
        None => (name.clone(), String::new()),
    };
    let mut n = 2;
    loop {
        let candidate = format!("{}_{}{}", stem, n, ext);
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_alias_for_folder() {
        assert_eq!(sanitize_alias("Qwen3.6-35B MTP"), "Qwen3.6-35B-MTP");
        assert_eq!(sanitize_alias("Gemma4-31B  MTP"), "Gemma4-31B-MTP");
        assert_eq!(sanitize_alias("a/b:c"), "a-b-c");
    }

    #[test]
    fn extracts_and_names_code_blocks() {
        let md = "intro\n```html\n<html></html>\n```\nmid\n```svg\n<svg></svg>\n```\n```html\n<html>2</html>\n```";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 3);
        let files = map_code_files(&blocks);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["index.html", "image.svg", "index_2.html"]);
    }

    #[test]
    fn infers_svg_without_lang() {
        let files = map_code_files(&[(String::new(), "<svg>x</svg>".into())]);
        assert_eq!(files[0].0, "image.svg");
    }
}
