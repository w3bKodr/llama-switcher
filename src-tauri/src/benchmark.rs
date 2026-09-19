//! Benchmark runner: cycle selected models through a set of prompts and save
//! each model's actioned output. Model switching goes through
//! `process_manager::activate_profile`; prompts are sent to the server's OpenAI
//! chat endpoint with the reused API key. Runs on a background thread and emits
//! `benchmark-progress` events so the UI can render a live grid.

use crate::benchmark_catalog::{BenchmarkCase, GradeResult};
use crate::benchmark_report::BenchmarkReportRecord;
use crate::process_manager;
use crate::state::AppState;
use crate::{benchmark_catalog, benchmark_report};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPrompt {
    pub id: String,
    pub title: String,
    pub text: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkConfig {
    pub profile_ids: Vec<String>,
    /// Legacy/custom editable prompts. Kept under the old field name so
    /// existing benchmark.json files continue to load.
    pub prompts: Vec<BenchmarkPrompt>,
    #[serde(default)]
    pub professional_case_ids: Vec<String>,
    pub output_dir: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_model_start_timeout")]
    pub model_start_timeout_seconds: u64,
    #[serde(default = "default_grading_timeout")]
    pub grading_timeout_seconds: u64,
    #[serde(default = "default_runs_per_prompt")]
    pub runs_per_prompt: u32,
    #[serde(default = "default_generate_html_report")]
    pub generate_html_report: bool,
    #[serde(default)]
    pub open_report_when_complete: bool,
    /// Reuse only fully written, fingerprint-matching run metadata. This makes
    /// long benchmarks crash-safe without accepting stale prompt results.
    #[serde(default = "default_resume_completed_runs")]
    pub resume_completed_runs: bool,
}

fn default_timeout() -> u64 {
    300
}

fn default_model_start_timeout() -> u64 {
    300
}

fn default_grading_timeout() -> u64 {
    30
}

fn default_runs_per_prompt() -> u32 {
    1
}

fn default_generate_html_report() -> bool {
    true
}

fn default_resume_completed_runs() -> bool {
    true
}
#[derive(Serialize, Clone, Default)]
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
    /// One-based repetition currently running or just completed.
    run_index: Option<u32>,
    run_count: Option<u32>,
    /// Raw totals let the UI calculate a correctly weighted rate across cells.
    draft_tokens: Option<u64>,
    accepted_draft_tokens: Option<u64>,
    speculative_acceptance_rate: Option<f64>,
    score: Option<f64>,
    passed_tests: Option<usize>,
    total_tests: Option<usize>,
    grade_status: Option<String>,
    report_path: Option<String>,
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
        if let Ok(mut cfg) = serde_json::from_str::<BenchmarkConfig>(&text) {
            // Older files had only editable prompts. Select the bundled suite
            // once during migration, while preserving an intentional empty
            // selection in newer files.
            let has_professional_ids = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value.get("professionalCaseIds").cloned())
                .is_some();
            if !has_professional_ids {
                cfg.professional_case_ids = benchmark_catalog::catalog()
                    .into_iter()
                    .map(|case| case.id)
                    .collect();
            } else {
                let available = benchmark_catalog::catalog()
                    .into_iter()
                    .map(|case| case.id)
                    .collect::<HashSet<_>>();
                let had_retired_cases = cfg
                    .professional_case_ids
                    .iter()
                    .any(|id| !available.contains(id));
                cfg.professional_case_ids
                    .retain(|id| available.contains(id));
                if had_retired_cases && cfg.professional_case_ids.is_empty() {
                    cfg.professional_case_ids = available.into_iter().collect();
                    cfg.professional_case_ids.sort();
                }
            }
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
                enabled: true,
            },
            BenchmarkPrompt {
                id: "prompt2".into(),
                title: "Parallax car canvas".into(),
                text: CAR_PROMPT.into(),
                enabled: true,
            },
        ],
        professional_case_ids: benchmark_catalog::catalog()
            .into_iter()
            .map(|case| case.id)
            .collect(),
        output_dir: String::new(),
        timeout_seconds: default_timeout(),
        model_start_timeout_seconds: default_model_start_timeout(),
        grading_timeout_seconds: default_grading_timeout(),
        runs_per_prompt: default_runs_per_prompt(),
        generate_html_report: default_generate_html_report(),
        open_report_when_complete: false,
        resume_completed_runs: default_resume_completed_runs(),
    }
}

/// Safe frontend-facing catalog metadata. Hidden inputs and expected values
/// never leave the Rust process.
pub fn professional_catalog() -> Vec<benchmark_catalog::ProfessionalBenchmarkSummary> {
    benchmark_catalog::summaries()
}

const CHESS_PROMPT: &str = "Given this PGN string of a chess game:\n\n1. b3 e5 2. Nf3 h5 3. d4 exd4 4. Nxd4 Nf6 5. f4 Ke7 6. Qd3 d5 7. h4 *\n\nFigure out the current state of the chessboard, create an image in SVG code, also highlight the last move.";

const CAR_PROMPT: &str = "Write a single HTML file with a full-page canvas and no libraries. Simulate a realistic side-view of a moving car as the main subject. Keep the car visible in the foreground while the background landscape scrolls continuously to create the feeling that the car is driving forward. Use layered scenery for depth: nearby ground, roadside elements, trees, poles, and distant hills or mountains should move at different speeds for a natural parallax effect. Animate the wheels spinning realistically and add subtle body motion so the car feels connected to the road. Let the environment pass smoothly behind it, with repeating but varied scenery that makes the movement feel believable. Use cinematic lighting and a cohesive sky, such as sunset, dusk, or daylight, to enhance atmosphere. The overall motion should feel calm, immersive, and realistic, with a seamless looping animation";

#[derive(Clone)]
struct BenchmarkItem {
    id: String,
    title: String,
    text: String,
    kind: String,
    difficulty: Option<String>,
    grading_timeout_seconds: u64,
    case: Option<BenchmarkCase>,
}

fn selected_items(config: &BenchmarkConfig) -> Vec<BenchmarkItem> {
    let mut items = config
        .prompts
        .iter()
        .filter(|prompt| prompt.enabled)
        .map(|prompt| BenchmarkItem {
            id: prompt.id.clone(),
            title: prompt.title.clone(),
            text: prompt.text.clone(),
            kind: "custom".into(),
            difficulty: None,
            grading_timeout_seconds: config.grading_timeout_seconds,
            case: None,
        })
        .collect::<Vec<_>>();
    let selected_professional = config.professional_case_ids.iter().collect::<HashSet<_>>();
    for case in benchmark_catalog::catalog() {
        if selected_professional.contains(&case.id) {
            items.push(BenchmarkItem {
                id: case.id.clone(),
                title: case.title.clone(),
                text: case.prompt.clone(),
                kind: "professional".into(),
                difficulty: Some(case.difficulty.clone()),
                grading_timeout_seconds: config.grading_timeout_seconds,
                case: Some(case),
            });
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

pub fn is_running(state: &Arc<AppState>) -> bool {
    *state.benchmark_running.lock().unwrap()
}

pub fn resumable_run_count(state: &Arc<AppState>, config: &BenchmarkConfig) -> usize {
    let items = selected_items(config);
    config
        .profile_ids
        .iter()
        .filter_map(|profile_id| {
            state
                .find_profile(profile_id)
                .map(|profile| (profile_id, profile))
        })
        .map(|(profile_id, profile)| {
            let model_dir = Path::new(&config.output_dir).join(sanitize_alias(&profile.alias));
            items
                .iter()
                .map(|item| {
                    let prompt_dir = model_dir.join(output_folder_name(item));
                    (1..=config.runs_per_prompt)
                        .filter(|run_index| {
                            let run_dir = if config.runs_per_prompt == 1 {
                                prompt_dir.clone()
                            } else {
                                prompt_dir.join(format!("run-{run_index:02}"))
                            };
                            load_completed_result(
                                &run_dir,
                                profile_id,
                                &profile.alias,
                                item,
                                *run_index,
                            )
                            .is_some()
                        })
                        .count()
                })
                .sum::<usize>()
        })
        .sum()
}

pub fn cancel(app: &AppHandle, state: &Arc<AppState>) {
    invalidate_run(app, state);
}

pub fn pause(app: &AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    if !is_running(state) {
        return Err("No benchmark is running.".into());
    }
    *state.benchmark_paused.lock().unwrap() = true;
    emit_run_state(
        app,
        "pausing",
        Some("Finishing the active evaluation before pausing.".into()),
    );
    Ok(())
}

pub fn resume(app: &AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    if !is_running(state) {
        return Err("No benchmark is running.".into());
    }
    *state.benchmark_paused.lock().unwrap() = false;
    emit_run_state(app, "resumed", None);
    Ok(())
}

fn emit_run_state(app: &AppHandle, status: &str, message: Option<String>) {
    emit(
        app,
        Progress {
            kind: "run".into(),
            status: status.into(),
            message,
            ..Default::default()
        },
    );
}

/// Emergency cancellation used by the Status-page Stop button. Unlike the
/// normal benchmark Cancel action, this leaves the server stopped.
pub fn cancel_and_stop(app: &AppHandle, state: &Arc<AppState>) {
    invalidate_run(app, state);
}

fn invalidate_run(app: &AppHandle, state: &Arc<AppState>) {
    let mut running = state.benchmark_running.lock().unwrap();
    let was_running = *running;
    *running = false;
    *state.benchmark_cancel.lock().unwrap() = true;
    *state.benchmark_paused.lock().unwrap() = false;
    *state.benchmark_generation.lock().unwrap() += 1;
    drop(running);
    if was_running {
        emit(
            app,
            Progress {
                kind: "run".into(),
                status: "cancelled".into(),
                profile_id: None,
                alias: None,
                prompt_id: None,
                output_path: None,
                message: None,
                duration_seconds: None,
                tokens_per_second: None,
                run_index: None,
                run_count: None,
                draft_tokens: None,
                accepted_draft_tokens: None,
                speculative_acceptance_rate: None,
                ..Default::default()
            },
        );
        process_manager::notify(app, state);
    }
}

fn cancelled(state: &Arc<AppState>, generation: u64) -> bool {
    *state.benchmark_cancel.lock().unwrap()
        || *state.benchmark_generation.lock().unwrap() != generation
}

fn wait_if_paused(app: &AppHandle, state: &Arc<AppState>, generation: u64) -> bool {
    if !*state.benchmark_paused.lock().unwrap() {
        return !cancelled(state, generation);
    }
    emit_run_state(
        app,
        "paused",
        Some("Benchmark paused between evaluations.".into()),
    );
    while *state.benchmark_paused.lock().unwrap() {
        if cancelled(state, generation) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !cancelled(state, generation)
}

/// Validate + start a run on a background thread. Returns an error if a run is
/// already in progress or the config is invalid.
pub fn start(
    app: AppHandle,
    state: Arc<AppState>,
    config: BenchmarkConfig,
    start_fresh: bool,
) -> Result<(), String> {
    if config.profile_ids.is_empty() {
        return Err("Select at least one model.".into());
    }
    if selected_items(&config).is_empty() {
        return Err("Select at least one custom or professional benchmark.".into());
    }
    if config.output_dir.trim().is_empty() {
        return Err("Choose an output folder.".into());
    }
    if !(1..=100).contains(&config.runs_per_prompt) {
        return Err("Runs per prompt must be between 1 and 100.".into());
    }
    if !(1..=300).contains(&config.grading_timeout_seconds) {
        return Err("Grading timeout must be between 1 and 300 seconds.".into());
    }
    let generation = {
        let mut running = state.benchmark_running.lock().unwrap();
        if *running {
            return Err("A benchmark is already running.".into());
        }
        *state.benchmark_cancel.lock().unwrap() = false;
        *state.benchmark_paused.lock().unwrap() = false;
        let mut generation = state.benchmark_generation.lock().unwrap();
        *generation += 1;
        *running = true;
        *generation
    };
    // Persist the user's crash-recovery preference, but allow this particular
    // launch to explicitly ignore checkpoints when "Start new" was chosen.
    let _ = save_config(&state, &config);
    let mut run_config = config;
    if start_fresh {
        run_config.resume_completed_runs = false;
    }
    std::thread::spawn(move || run_inner(&app, &state, run_config, generation));
    Ok(())
}

fn run_inner(app: &AppHandle, state: &Arc<AppState>, config: BenchmarkConfig, generation: u64) {
    if cancelled(state, generation) {
        return;
    }
    emit(
        app,
        Progress {
            kind: "run".into(),
            status: "running".into(),
            profile_id: None,
            alias: None,
            prompt_id: None,
            output_path: None,
            message: None,
            duration_seconds: None,
            tokens_per_second: None,
            run_index: None,
            run_count: Some(config.runs_per_prompt),
            draft_tokens: None,
            accepted_draft_tokens: None,
            speculative_acceptance_rate: None,
            ..Default::default()
        },
    );

    let previous = state.status().current_profile_id.clone();
    let settings = state.settings_snapshot();

    if let Err(e) = std::fs::create_dir_all(&config.output_dir) {
        run_finished(
            app,
            state,
            previous,
            format!("Cannot create output folder: {}", e),
            None,
        );
        return;
    }

    let items = selected_items(&config);
    let mut report_records = Vec::new();
    'models: for profile_id in &config.profile_ids {
        if !wait_if_paused(app, state, generation) {
            break;
        }
        let profile = match state.find_profile(profile_id) {
            Some(p) => p,
            None => {
                emit_model(
                    app,
                    profile_id,
                    None,
                    "error",
                    Some("Unknown profile".into()),
                );
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
            if !wait_healthy(app, state, config.model_start_timeout_seconds, generation) {
                emit_model(
                    app,
                    profile_id,
                    Some(&profile.alias),
                    "error",
                    Some(format!(
                        "Server did not become healthy within {} seconds.",
                        config.model_start_timeout_seconds.max(1)
                    )),
                );
                continue;
            }
        }
        emit_model(app, profile_id, Some(&profile.alias), "running", None);

        let api_key = process_manager::resolve_api_key(state, Some(&profile.script_path));
        let origin = process_manager::server_origin(&settings.health_url, settings.server_port);
        let model_dir = Path::new(&config.output_dir).join(sanitize_alias(&profile.alias));

        for item in &items {
            if !wait_if_paused(app, state, generation) {
                break 'models;
            }
            let prompt_dir = model_dir.join(output_folder_name(item));
            let mut results = Vec::new();
            let mut errors = Vec::new();

            for run_index in 1..=config.runs_per_prompt {
                if !wait_if_paused(app, state, generation) {
                    break 'models;
                }
                let run_dir = if config.runs_per_prompt == 1 {
                    prompt_dir.clone()
                } else {
                    prompt_dir.join(format!("run-{run_index:02}"))
                };
                emit_prompt(
                    app,
                    profile_id,
                    &profile.alias,
                    &item.id,
                    "running",
                    &run_dir,
                    None,
                    None,
                    None,
                    Some(run_index),
                    Some(config.runs_per_prompt),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                );

                if config.resume_completed_runs {
                    if let Some(result) =
                        load_completed_result(&run_dir, profile_id, &profile.alias, item, run_index)
                    {
                        emit_prompt(
                            app,
                            profile_id,
                            &profile.alias,
                            &item.id,
                            "iteration_done",
                            &run_dir,
                            Some("Resumed from completed checkpoint.".into()),
                            Some(result.elapsed_seconds),
                            result.tokens_per_second,
                            Some(run_index),
                            Some(config.runs_per_prompt),
                            result.draft_tokens,
                            result.accepted_draft_tokens,
                            result.grade.as_ref().map(|grade| grade.score),
                            result.grade.as_ref().map(|grade| grade.passed),
                            result.grade.as_ref().map(|grade| grade.total),
                            result.grade.as_ref().map(|grade| grade.status.clone()),
                        );
                        report_records.push(report_record(
                            profile_id,
                            &profile.alias,
                            item,
                            run_index,
                            &run_dir,
                            &result,
                        ));
                        results.push(result);
                        continue;
                    }
                }

                // This is a fresh attempt, so artifacts from an older run must
                // never be mistaken for the outcome of the request below.
                clear_stale_run_artifacts(&run_dir);

                let result = run_prompt_cancellable(
                    state,
                    generation,
                    &origin,
                    api_key.as_deref(),
                    item,
                    config.timeout_seconds,
                    &run_dir,
                    profile_id,
                    &profile.alias,
                    run_index,
                    config.runs_per_prompt,
                );
                if cancelled(state, generation) {
                    break 'models;
                }
                match result {
                    Ok(result) => {
                        if result
                            .grade
                            .as_ref()
                            .is_some_and(|grade| grade.status == "timeout")
                        {
                            // ureq closes the socket at the deadline, but
                            // llama-server releases the cancelled slot
                            // asynchronously. Do not queue the next benchmark
                            // while that generation is still winding down.
                            process_manager::wait_for_server_idle(state, Duration::from_secs(30));
                        }
                        emit_prompt(
                            app,
                            profile_id,
                            &profile.alias,
                            &item.id,
                            "iteration_done",
                            &run_dir,
                            None,
                            Some(result.elapsed_seconds),
                            result.tokens_per_second,
                            Some(run_index),
                            Some(config.runs_per_prompt),
                            result.draft_tokens,
                            result.accepted_draft_tokens,
                            result.grade.as_ref().map(|grade| grade.score),
                            result.grade.as_ref().map(|grade| grade.passed),
                            result.grade.as_ref().map(|grade| grade.total),
                            result.grade.as_ref().map(|grade| grade.status.clone()),
                        );
                        report_records.push(report_record(
                            profile_id,
                            &profile.alias,
                            item,
                            run_index,
                            &run_dir,
                            &result,
                        ));
                        results.push(result);
                    }
                    Err(e) => {
                        emit_prompt(
                            app,
                            profile_id,
                            &profile.alias,
                            &item.id,
                            "iteration_error",
                            &run_dir,
                            Some(e.clone()),
                            None,
                            None,
                            Some(run_index),
                            Some(config.runs_per_prompt),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                        );
                        report_records.push(BenchmarkReportRecord {
                            profile_id: profile_id.clone(),
                            alias: profile.alias.clone(),
                            benchmark_id: item.id.clone(),
                            benchmark_title: item.title.clone(),
                            benchmark_kind: item.kind.clone(),
                            difficulty: item.difficulty.clone(),
                            weight: item.case.as_ref().map(|case| case.weight),
                            attempt: run_index,
                            status: "error".into(),
                            score: None,
                            passed: None,
                            total: None,
                            duration_seconds: None,
                            tokens_per_second: None,
                            draft_tokens: None,
                            accepted_draft_tokens: None,
                            speculative_acceptance_rate: None,
                            output_path: run_dir.to_string_lossy().to_string(),
                            feedback: vec![e.clone()],
                        });
                        errors.push(format!("Run {run_index}: {e}"));
                    }
                }
            }

            let aggregate = PromptAggregate::from_results(&results);
            let _ = write_prompt_summary(
                &prompt_dir,
                &profile.alias,
                item,
                config.runs_per_prompt,
                &aggregate,
                &errors,
            );
            let final_status = if results.is_empty() { "error" } else { "done" };
            let message = if errors.is_empty() {
                None
            } else {
                Some(format!(
                    "{} of {} repetitions failed: {}",
                    errors.len(),
                    config.runs_per_prompt,
                    errors.join("; ")
                ))
            };
            emit_prompt(
                app,
                profile_id,
                &profile.alias,
                &item.id,
                final_status,
                &prompt_dir,
                message,
                aggregate.average_duration_seconds,
                aggregate.average_tokens_per_second,
                None,
                Some(config.runs_per_prompt),
                aggregate.draft_tokens,
                aggregate.accepted_draft_tokens,
                aggregate.average_score,
                aggregate.passed_tests,
                aggregate.total_tests,
                aggregate.grade_status.clone(),
            );
        }
    }

    if *state.benchmark_generation.lock().unwrap() != generation {
        return;
    }
    let msg = if cancelled(state, generation) {
        "cancelled".to_string()
    } else {
        "finished".to_string()
    };
    let report_path = if config.generate_html_report {
        benchmark_report::write_html_report(Path::new(&config.output_dir), &report_records)
            .ok()
            .map(|path| path.to_string_lossy().to_string())
    } else {
        None
    };
    run_finished(app, state, previous, msg, report_path);
}

fn run_finished(
    app: &AppHandle,
    state: &Arc<AppState>,
    previous: Option<String>,
    status: String,
    report_path: Option<String>,
) {
    let notification = benchmark_completion_notification(&status);

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
    *state.benchmark_paused.lock().unwrap() = false;
    emit(
        app,
        Progress {
            kind: "run".into(),
            status,
            profile_id: None,
            alias: None,
            prompt_id: None,
            output_path: None,
            message: None,
            duration_seconds: None,
            tokens_per_second: None,
            run_index: None,
            run_count: None,
            draft_tokens: None,
            accepted_draft_tokens: None,
            speculative_acceptance_rate: None,
            report_path,
            ..Default::default()
        },
    );
    process_manager::notify(app, state);
    if let Some((title, body)) = notification {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

fn benchmark_completion_notification(status: &str) -> Option<(&'static str, &'static str)> {
    (status == "finished").then_some((
        "Benchmark complete",
        "All selected model and prompt runs have finished.",
    ))
}

fn wait_healthy(app: &AppHandle, state: &Arc<AppState>, timeout_s: u64, generation: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(timeout_s.max(1));
    while Instant::now() < deadline {
        if cancelled(state, generation) {
            return false;
        }
        if process_manager::status_with_probe(app, state).healthy {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

fn emit_model(
    app: &AppHandle,
    profile_id: &str,
    alias: Option<&str>,
    status: &str,
    message: Option<String>,
) {
    emit(
        app,
        Progress {
            kind: "model".into(),
            status: status.into(),
            profile_id: Some(profile_id.into()),
            alias: alias.map(|a| a.to_string()),
            prompt_id: None,
            output_path: None,
            message,
            duration_seconds: None,
            tokens_per_second: None,
            run_index: None,
            run_count: None,
            draft_tokens: None,
            accepted_draft_tokens: None,
            speculative_acceptance_rate: None,
            ..Default::default()
        },
    );
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
    run_index: Option<u32>,
    run_count: Option<u32>,
    draft_tokens: Option<u64>,
    accepted_draft_tokens: Option<u64>,
    score: Option<f64>,
    passed_tests: Option<usize>,
    total_tests: Option<usize>,
    grade_status: Option<String>,
) {
    let speculative_acceptance_rate = weighted_acceptance(draft_tokens, accepted_draft_tokens);
    emit(
        app,
        Progress {
            kind: "prompt".into(),
            status: status.into(),
            profile_id: Some(profile_id.into()),
            alias: Some(alias.into()),
            prompt_id: Some(prompt_id.into()),
            output_path: Some(dir.to_string_lossy().to_string()),
            message,
            duration_seconds,
            tokens_per_second,
            run_index,
            run_count,
            draft_tokens,
            accepted_draft_tokens,
            speculative_acceptance_rate,
            score,
            passed_tests,
            total_tests,
            grade_status,
            ..Default::default()
        },
    );
}

// ---------------------------------------------------------------------------
// One prompt against one model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct PromptResult {
    elapsed_seconds: f64,
    tokens_per_second: Option<f64>,
    draft_tokens: Option<u64>,
    accepted_draft_tokens: Option<u64>,
    grade: Option<GradeResult>,
}

#[derive(Default)]
struct PromptAggregate {
    successful_runs: usize,
    average_duration_seconds: Option<f64>,
    average_tokens_per_second: Option<f64>,
    draft_tokens: Option<u64>,
    accepted_draft_tokens: Option<u64>,
    speculative_acceptance_rate: Option<f64>,
    average_score: Option<f64>,
    passed_tests: Option<usize>,
    total_tests: Option<usize>,
    grade_status: Option<String>,
}

impl PromptAggregate {
    fn from_results(results: &[PromptResult]) -> Self {
        let successful_runs = results.len();
        let average_duration_seconds = average(results.iter().map(|result| result.elapsed_seconds));
        let average_tokens_per_second =
            average(results.iter().filter_map(|result| result.tokens_per_second));
        let draft_tokens = optional_sum(results.iter().filter_map(|result| result.draft_tokens));
        let accepted_draft_tokens = optional_sum(
            results
                .iter()
                .filter_map(|result| result.accepted_draft_tokens),
        );
        let speculative_acceptance_rate = weighted_acceptance(draft_tokens, accepted_draft_tokens);
        let grades = results
            .iter()
            .filter_map(|result| result.grade.as_ref())
            .collect::<Vec<_>>();
        let average_score = average(grades.iter().map(|grade| grade.score));
        let passed_tests =
            (!grades.is_empty()).then(|| grades.iter().map(|grade| grade.passed).sum());
        let total_tests =
            (!grades.is_empty()).then(|| grades.iter().map(|grade| grade.total).sum());
        let grade_status = if grades.is_empty() {
            None
        } else if grades.iter().all(|grade| grade.status == "passed") {
            Some("passed".into())
        } else {
            Some("failed".into())
        };
        Self {
            successful_runs,
            average_duration_seconds,
            average_tokens_per_second,
            draft_tokens,
            accepted_draft_tokens,
            speculative_acceptance_rate,
            average_score,
            passed_tests,
            total_tests,
            grade_status,
        }
    }
}

fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0_u64), |(sum, count), value| (sum + value, count + 1));
    (count > 0).then_some(sum / count as f64)
}

fn optional_sum(values: impl Iterator<Item = u64>) -> Option<u64> {
    let values = values.collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.into_iter().sum())
}

fn weighted_acceptance(draft_tokens: Option<u64>, accepted: Option<u64>) -> Option<f64> {
    match (draft_tokens, accepted) {
        (Some(drafted), Some(accepted)) if drafted > 0 => Some(accepted as f64 / drafted as f64),
        _ => None,
    }
}

fn value_as_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| *number >= 0.0)
            .map(|number| number as u64)
    })
}

fn resume_key(item: &BenchmarkItem) -> String {
    // Stable FNV-1a fingerprint: selected case/prompt content and suite version
    // must match before an on-disk iteration is accepted as a checkpoint.
    let source = format!(
        "{}\0{}\0{}\0{}",
        item.id,
        item.kind,
        item.text,
        benchmark_catalog::SUITE_VERSION
    );
    let hash = source
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_string_pretty(value).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

fn load_completed_result(
    run_dir: &Path,
    profile_id: &str,
    alias: &str,
    item: &BenchmarkItem,
    run_index: u32,
) -> Option<PromptResult> {
    if !run_dir.join("response.md").is_file() {
        return None;
    }
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("meta.json")).ok()?).ok()?;
    if meta["checkpointComplete"] != Value::Bool(true)
        || meta["resumeKey"].as_str()? != resume_key(item)
        || meta["profileId"].as_str()? != profile_id
        || meta["alias"].as_str()? != alias
        || meta["promptId"].as_str()? != item.id
        || meta["runIndex"].as_u64()? != u64::from(run_index)
    {
        return None;
    }
    Some(PromptResult {
        elapsed_seconds: meta["durationSeconds"].as_f64()?,
        tokens_per_second: meta["tokensPerSecond"].as_f64(),
        draft_tokens: value_as_u64(&meta["draftTokens"]),
        accepted_draft_tokens: value_as_u64(&meta["acceptedDraftTokens"]),
        grade: if meta["grade"].is_null() {
            None
        } else {
            serde_json::from_value(meta["grade"].clone()).ok()
        },
    })
}

fn is_timeout_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("timed out")
        || message.contains("timeout")
        || message.contains("10060")
        || message.contains("wsaetimedout")
        || message.contains("failed to respond")
}

fn clear_stale_run_artifacts(run_dir: &Path) {
    for name in ["meta.json", "response.md"] {
        let path = run_dir.join(name);
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn report_record(
    profile_id: &str,
    alias: &str,
    item: &BenchmarkItem,
    attempt: u32,
    output_path: &Path,
    result: &PromptResult,
) -> BenchmarkReportRecord {
    let grade = result.grade.as_ref();
    BenchmarkReportRecord {
        profile_id: profile_id.into(),
        alias: alias.into(),
        benchmark_id: item.id.clone(),
        benchmark_title: item.title.clone(),
        benchmark_kind: item.kind.clone(),
        difficulty: item.difficulty.clone(),
        weight: item.case.as_ref().map(|case| case.weight),
        attempt,
        status: grade
            .map(|value| value.status.clone())
            .unwrap_or_else(|| "complete".into()),
        score: grade.map(|value| value.score),
        passed: grade.map(|value| value.passed),
        total: grade.map(|value| value.total),
        duration_seconds: Some(result.elapsed_seconds),
        tokens_per_second: result.tokens_per_second,
        draft_tokens: result.draft_tokens,
        accepted_draft_tokens: result.accepted_draft_tokens,
        speculative_acceptance_rate: weighted_acceptance(
            result.draft_tokens,
            result.accepted_draft_tokens,
        ),
        output_path: output_path.to_string_lossy().to_string(),
        feedback: grade
            .map(|value| value.feedback.clone())
            .unwrap_or_default(),
    }
}

fn run_prompt(
    origin: &str,
    api_key: Option<&str>,
    item: &BenchmarkItem,
    timeout_s: u64,
    prompt_dir: &Path,
    profile_id: &str,
    alias: &str,
    run_index: u32,
    run_count: u32,
) -> Result<PromptResult, String> {
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
    let mut payload = json!({
        "messages": [{ "role": "user", "content": item.text }],
        "stream": false
    });
    if item.case.is_some() {
        if let Some(object) = payload.as_object_mut() {
            object.insert("temperature".into(), json!(0));
            object.insert("top_p".into(), json!(1));
            object.insert("seed".into(), json!(1));
        }
    }
    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let response = match req
        .set("Content-Type", "application/json")
        .send_string(&body)
    {
        Ok(response) => response,
        Err(error) => {
            let message = format!("Request failed: {error}");
            if item.case.is_some() && is_timeout_error(&message) {
                let elapsed = started.elapsed().as_secs_f64();
                let case = item.case.as_ref().expect("professional case");
                let grade = GradeResult {
                    benchmark_id: case.id.clone(),
                    suite_version: benchmark_catalog::SUITE_VERSION,
                    status: "timeout".into(),
                    score: 0.0,
                    passed: 0,
                    failed: case.tests.len(),
                    total: case.tests.len(),
                    feedback: vec![format!(
                        "Generation exceeded the {} second per-test timeout.",
                        timeout_s.max(1)
                    )],
                };
                std::fs::write(prompt_dir.join("response.md"), "").map_err(|e| e.to_string())?;
                let meta = json!({
                    "checkpointComplete": true,
                    "resumeKey": resume_key(item),
                    "profileId": profile_id,
                    "alias": alias,
                    "promptId": item.id,
                    "promptTitle": item.title,
                    "benchmarkKind": item.kind,
                    "difficulty": item.difficulty,
                    "grade": grade,
                    "tokensPerSecond": Value::Null,
                    "runIndex": run_index,
                    "runCount": run_count,
                    "durationSeconds": elapsed,
                    "finishReason": "timeout",
                    "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                });
                write_json_atomic(&prompt_dir.join("meta.json"), &meta)?;
                return Ok(PromptResult {
                    elapsed_seconds: elapsed,
                    tokens_per_second: None,
                    draft_tokens: None,
                    accepted_draft_tokens: None,
                    grade: Some(grade),
                });
            }
            return Err(message);
        }
    };
    let text = response.into_string().map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let elapsed = started.elapsed().as_secs_f64();

    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    // Persist the raw reply before evaluating model-generated JavaScript. Even
    // if the isolated grader is terminated, crash recovery retains the answer.
    std::fs::write(prompt_dir.join("response.md"), &content).map_err(|e| e.to_string())?;

    // Extracted code artifacts.
    for (name, code) in map_code_files(&extract_code_blocks(&content)) {
        std::fs::write(prompt_dir.join(&name), code).map_err(|e| e.to_string())?;
    }

    let grade = item.case.as_ref().map(|case| {
        benchmark_catalog::grade_submission_isolated(case, &content, item.grading_timeout_seconds)
    });

    // Metadata.
    let timings = &value["timings"];
    let usage = &value["usage"];
    let tokens_per_second = timings["predicted_per_second"].as_f64();
    let draft_tokens = value_as_u64(&timings["draft_n"]);
    let accepted_draft_tokens = value_as_u64(&timings["draft_n_accepted"]);
    let speculative_acceptance_rate = weighted_acceptance(draft_tokens, accepted_draft_tokens);
    let meta = json!({
        "checkpointComplete": true,
        "resumeKey": resume_key(item),
        "profileId": profile_id,
        "alias": alias,
        "promptId": item.id,
        "promptTitle": item.title,
        "benchmarkKind": item.kind,
        "difficulty": item.difficulty,
        "grade": grade,
        "predictedTokens": timings["predicted_n"].as_f64().or_else(|| usage["completion_tokens"].as_f64()),
        "promptTokens": timings["prompt_n"].as_f64().or_else(|| usage["prompt_tokens"].as_f64()),
        "tokensPerSecond": tokens_per_second,
        "runIndex": run_index,
        "runCount": run_count,
        "draftTokens": draft_tokens,
        "acceptedDraftTokens": accepted_draft_tokens,
        "speculativeAcceptanceRate": speculative_acceptance_rate,
        "durationSeconds": elapsed,
        "finishReason": value["choices"][0]["finish_reason"].as_str(),
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    write_json_atomic(&prompt_dir.join("meta.json"), &meta)?;

    Ok(PromptResult {
        elapsed_seconds: elapsed,
        tokens_per_second,
        draft_tokens,
        accepted_draft_tokens,
        grade,
    })
}

fn write_prompt_summary(
    prompt_dir: &Path,
    alias: &str,
    item: &BenchmarkItem,
    runs_requested: u32,
    aggregate: &PromptAggregate,
    errors: &[String],
) -> Result<(), String> {
    std::fs::create_dir_all(prompt_dir).map_err(|e| e.to_string())?;
    let summary = json!({
        "alias": alias,
        "promptId": item.id,
        "promptTitle": item.title,
        "benchmarkKind": item.kind,
        "difficulty": item.difficulty,
        "runsRequested": runs_requested,
        "successfulRuns": aggregate.successful_runs,
        "failedRuns": errors.len(),
        "averageTokensPerSecond": aggregate.average_tokens_per_second,
        "averageDurationSeconds": aggregate.average_duration_seconds,
        "draftTokens": aggregate.draft_tokens,
        "acceptedDraftTokens": aggregate.accepted_draft_tokens,
        "weightedSpeculativeAcceptanceRate": aggregate.speculative_acceptance_rate,
        "averageScore": aggregate.average_score,
        "passedTests": aggregate.passed_tests,
        "totalTests": aggregate.total_tests,
        "gradeStatus": aggregate.grade_status,
        "errors": errors,
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    std::fs::write(
        prompt_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())
}

/// Run the blocking HTTP request on a disposable worker and let the benchmark
/// coordinator observe cancellation while that request is in flight. This is
/// important when Stop closes the server socket before ureq returns.
fn run_prompt_cancellable(
    state: &Arc<AppState>,
    generation: u64,
    origin: &str,
    api_key: Option<&str>,
    item: &BenchmarkItem,
    timeout_s: u64,
    prompt_dir: &Path,
    profile_id: &str,
    alias: &str,
    run_index: u32,
    run_count: u32,
) -> Result<PromptResult, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let origin = origin.to_string();
    let api_key = api_key.map(str::to_string);
    let item = item.clone();
    let prompt_dir = prompt_dir.to_path_buf();
    let profile_id = profile_id.to_string();
    let alias = alias.to_string();
    std::thread::spawn(move || {
        let result = run_prompt(
            &origin,
            api_key.as_deref(),
            &item,
            timeout_s,
            &prompt_dir,
            &profile_id,
            &alias,
            run_index,
            run_count,
        );
        let _ = sender.send(result);
    });

    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) if cancelled(state, generation) => {
                return Err("Benchmark cancelled.".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Benchmark request worker stopped unexpectedly.".into());
            }
        }
    }
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

fn output_folder_name(item: &BenchmarkItem) -> PathBuf {
    match (&item.kind[..], item.difficulty.as_deref()) {
        ("professional", Some(difficulty)) => {
            let parent = format!(
                "{}-v{}-{}",
                benchmark_catalog::SUITE_ID,
                benchmark_catalog::SUITE_VERSION,
                difficulty
            );
            let prefix = format!(
                "{}-v{}-{}-",
                benchmark_catalog::SUITE_ID,
                benchmark_catalog::SUITE_VERSION,
                difficulty
            );
            let leaf = item.id.strip_prefix(&prefix).unwrap_or(&item.id);
            PathBuf::from(sanitize_alias(&parent)).join(sanitize_alias(leaf))
        }
        ("custom", _) => {
            let title = sanitize_alias(&item.title);
            let title = if title.is_empty() {
                "untitled".into()
            } else {
                title
            };
            PathBuf::from(format!("custom-{}-{}", title, sanitize_alias(&item.id)))
        }
        _ => PathBuf::from(sanitize_alias(&item.id)),
    }
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
        let base = if l == "html"
            || l == "htm"
            || head.starts_with("<!doctype html")
            || head.starts_with("<html")
        {
            "index.html".to_string()
        } else if l == "svg" || head.starts_with("<svg") {
            "image.svg".to_string()
        } else {
            let ext: String = l.chars().filter(|c| c.is_alphanumeric()).collect();
            let ext = if ext.is_empty() {
                "txt".to_string()
            } else {
                ext
            };
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
    fn professional_output_folders_use_difficulty() {
        let item = BenchmarkItem {
            id: "professional-js-v2-hard-01-build-batches".into(),
            title: "Hard".into(),
            text: "test".into(),
            kind: "professional".into(),
            difficulty: Some("hard".into()),
            grading_timeout_seconds: 30,
            case: None,
        };
        assert_eq!(
            output_folder_name(&item),
            PathBuf::from("professional-js-v2-hard").join("01-build-batches")
        );
    }

    #[test]
    fn selected_professional_items_follow_catalog_order() {
        let catalog = benchmark_catalog::catalog();
        let mut config = default_config();
        config
            .prompts
            .iter_mut()
            .for_each(|prompt| prompt.enabled = false);
        config.professional_case_ids = catalog.iter().rev().map(|case| case.id.clone()).collect();

        let items = selected_items(&config);
        let difficulties = items
            .iter()
            .map(|item| item.difficulty.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(&difficulties[..6], &["easy"; 6]);
        assert_eq!(&difficulties[6..12], &["medium"; 6]);
        assert_eq!(&difficulties[12..], &["hard"; 6]);
    }

    #[test]
    fn custom_output_folder_uses_the_test_title() {
        let item = BenchmarkItem {
            id: "prompt1".into(),
            title: "Chess PGN to SVG".into(),
            text: "test".into(),
            kind: "custom".into(),
            difficulty: None,
            grading_timeout_seconds: 30,
            case: None,
        };
        assert_eq!(
            output_folder_name(&item),
            PathBuf::from("custom-Chess-PGN-to-SVG-prompt1")
        );
    }

    #[test]
    fn completed_checkpoint_requires_matching_prompt_fingerprint() {
        let root =
            std::env::temp_dir().join(format!("llama-switcher-resume-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let item = BenchmarkItem {
            id: "custom-1".into(),
            title: "Resume me".into(),
            text: "original prompt".into(),
            kind: "custom".into(),
            difficulty: None,
            grading_timeout_seconds: 30,
            case: None,
        };
        std::fs::write(root.join("response.md"), "complete").unwrap();
        write_json_atomic(
            &root.join("meta.json"),
            &json!({
                "checkpointComplete": true,
                "resumeKey": resume_key(&item),
                "profileId": "profile-1",
                "alias": "Model",
                "promptId": item.id,
                "runIndex": 1,
                "durationSeconds": 12.5,
                "tokensPerSecond": 42.0,
                "draftTokens": 10,
                "acceptedDraftTokens": 8,
                "grade": Value::Null,
            }),
        )
        .unwrap();
        assert!(load_completed_result(&root, "profile-1", "Model", &item, 1).is_some());
        let mut changed = item.clone();
        changed.text = "changed prompt".into();
        assert!(load_completed_result(&root, "profile-1", "Model", &changed, 1).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recognizes_transport_timeout_messages() {
        assert!(is_timeout_error("Request failed: Network timed out"));
        assert!(is_timeout_error("request timeout"));
        assert!(is_timeout_error("socket error WSAETIMEDOUT"));
        assert!(is_timeout_error(
            "connected host has failed to respond. (os error 10060)"
        ));
        assert!(!is_timeout_error("Request failed: connection refused"));
    }

    #[test]
    fn fresh_attempt_removes_stale_checkpoint_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "llama-switcher-stale-run-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("meta.json"), "old").unwrap();
        std::fs::write(root.join("response.md"), "old").unwrap();

        clear_stale_run_artifacts(&root);

        assert!(!root.join("meta.json").exists());
        assert!(!root.join("response.md").exists());
        let _ = std::fs::remove_dir_all(root);
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

    #[test]
    fn legacy_config_gets_large_model_start_timeout() {
        let config: BenchmarkConfig = serde_json::from_str(
            r#"{"profileIds":[],"prompts":[{"id":"legacy","title":"Legacy","text":"test"}],"outputDir":"","timeoutSeconds":600}"#,
        )
        .expect("legacy benchmark config should deserialize");

        assert_eq!(config.model_start_timeout_seconds, 300);
        assert_eq!(config.runs_per_prompt, 1);
        assert!(config.prompts[0].enabled);
        assert_eq!(default_config().model_start_timeout_seconds, 300);
        assert_eq!(default_config().runs_per_prompt, 1);
        assert!(default_config().prompts.iter().all(|prompt| prompt.enabled));
    }

    #[test]
    fn aggregates_average_speed_and_weighted_spec_acceptance() {
        let aggregate = PromptAggregate::from_results(&[
            PromptResult {
                elapsed_seconds: 10.0,
                tokens_per_second: Some(20.0),
                draft_tokens: Some(100),
                accepted_draft_tokens: Some(90),
                grade: None,
            },
            PromptResult {
                elapsed_seconds: 14.0,
                tokens_per_second: Some(30.0),
                draft_tokens: Some(300),
                accepted_draft_tokens: Some(150),
                grade: None,
            },
        ]);

        assert_eq!(aggregate.successful_runs, 2);
        assert_eq!(aggregate.average_duration_seconds, Some(12.0));
        assert_eq!(aggregate.average_tokens_per_second, Some(25.0));
        assert_eq!(aggregate.draft_tokens, Some(400));
        assert_eq!(aggregate.accepted_draft_tokens, Some(240));
        assert_eq!(aggregate.speculative_acceptance_rate, Some(0.6));
    }

    #[test]
    fn omits_acceptance_when_speculative_decoding_is_inactive() {
        let aggregate = PromptAggregate::from_results(&[PromptResult {
            elapsed_seconds: 4.0,
            tokens_per_second: Some(42.0),
            draft_tokens: None,
            accepted_draft_tokens: None,
            grade: None,
        }]);

        assert_eq!(aggregate.average_tokens_per_second, Some(42.0));
        assert_eq!(aggregate.speculative_acceptance_rate, None);
    }

    #[test]
    fn completion_notification_only_shows_for_finished_runs() {
        assert_eq!(
            benchmark_completion_notification("finished"),
            Some((
                "Benchmark complete",
                "All selected model and prompt runs have finished."
            ))
        );
        assert_eq!(benchmark_completion_notification("cancelled"), None);
        assert_eq!(benchmark_completion_notification("error"), None);
    }
}
