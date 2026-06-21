//! Llama Switcher — lightweight Windows tray app for llama.cpp profiles.

mod alias_formatter;
mod hermes_install;
mod local_api;
mod logging;
mod process_manager;
mod process_tree;
mod script_scanner;
mod settings;
mod state;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use script_scanner::{Profile, ScanResult};
use settings::Settings;
use state::{AppState, Status};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// Shared helpers (used by tray + local_api)
// ---------------------------------------------------------------------------

pub fn get_state(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

/// Show and focus the dashboard window, optionally navigating to a page.
pub fn show_dashboard(app: &AppHandle, page: Option<&str>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        if let Some(p) = page {
            let _ = app.emit("navigate", p);
        }
    }
}

/// Re-scan the scripts folder, store the result, and refresh tray + UI.
pub fn rescan_and_store(app: &AppHandle, state: &Arc<AppState>) -> ScanResult {
    let settings = state.settings_snapshot();
    let scan = script_scanner::scan(&settings);
    *state.scan.lock().unwrap() = scan.clone();
    tray::rebuild(app, state);
    let _ = app.emit("scan-updated", &scan);
    scan
}

pub fn open_folder(path: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Stop any running server then exit the whole application.
pub fn quit_app(app: &AppHandle) {
    let state = get_state(app);
    let _ = process_manager::stop_server(app, &state);
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_status(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || process_manager::status_with_probe(&app, &st))
        .await
        .map_err(|e| e.to_string())
}

/// Lightweight port probe without takeover side-effects. Used by the UI to
/// decide whether to enable the Stop button.
#[tauri::command]
fn is_server_reachable(state: State<'_, Arc<AppState>>) -> bool {
    let settings = state.settings_snapshot();
    process_manager::probe_reachable(&settings.health_url)
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> Settings {
    state.settings_snapshot()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> Result<Settings, String> {
    {
        let mut s = state.settings.lock().unwrap();
        *s = settings.clone();
        s.save(&state.settings_path)?;
    }
    // Reflect any folder/pattern change immediately.
    let st = state.inner().clone();
    rescan_and_store(&app, &st);
    Ok(settings)
}

#[tauri::command]
fn rescan_scripts(app: AppHandle, state: State<'_, Arc<AppState>>) -> ScanResult {
    let st = state.inner().clone();
    rescan_and_store(&app, &st)
}

#[tauri::command]
fn get_detected_profiles(state: State<'_, Arc<AppState>>) -> Vec<Profile> {
    state.profiles()
}

#[tauri::command]
fn get_scan_result(state: State<'_, Arc<AppState>>) -> ScanResult {
    state.scan.lock().unwrap().clone()
}

#[tauri::command]
async fn start_profile(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profile_id: String,
) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        process_manager::activate_profile(&app, &st, &profile_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn switch_profile(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profile_id: String,
) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        process_manager::activate_profile(&app, &st, &profile_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn switch_profile_by_name(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    model: String,
    feature: String,
) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let id = process_manager::resolve_name(&st, &model, &feature)?;
        process_manager::activate_profile(&app, &st, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn switch_profile_by_alias(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    alias: String,
) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let id = process_manager::resolve_alias(&st, &alias)?;
        process_manager::activate_profile(&app, &st, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn stop_server(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || process_manager::stop_server(&app, &st))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn restart_server(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Status, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || process_manager::restart_server(&app, &st))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn read_latest_log(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    match logging::latest_log_path(&state.logs_dir) {
        Some(p) => logging::read_log(&p),
        None => Ok(String::new()),
    }
}

#[tauri::command]
fn read_log(path: String) -> Result<String, String> {
    logging::read_log(std::path::Path::new(&path))
}

#[tauri::command]
fn list_logs(state: State<'_, Arc<AppState>>) -> Vec<logging::LogEntry> {
    logging::list_logs(&state.logs_dir)
}

#[tauri::command]
fn clear_old_logs(state: State<'_, Arc<AppState>>) -> usize {
    logging::clear_old(&state.logs_dir)
}

#[tauri::command]
fn open_logs_folder(state: State<'_, Arc<AppState>>) {
    open_folder(&state.logs_dir.to_string_lossy());
}

#[tauri::command]
fn open_scripts_folder(state: State<'_, Arc<AppState>>) {
    open_folder(&state.settings_snapshot().scripts_folder);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentApiInfo {
    base_url: String,
    port: u16,
    token: String,
}

#[tauri::command]
fn get_agent_api_info(state: State<'_, Arc<AppState>>) -> AgentApiInfo {
    let s = state.settings_snapshot();
    AgentApiInfo {
        base_url: format!("http://127.0.0.1:{}", s.agent_api_port),
        port: s.agent_api_port,
        token: s.agent_api_token,
    }
}

#[tauri::command]
fn regenerate_agent_api_token(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let token = settings::generate_token();
    let mut s = state.settings.lock().unwrap();
    s.agent_api_token = token.clone();
    s.save(&state.settings_path)?;
    Ok(token)
}

/// Locate the bundled Hermes skill source: the resource dir in a packaged
/// build, or the project's `hermes-skill/` folder during development.
fn resolve_skill_source(app: &AppHandle) -> Option<std::path::PathBuf> {
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("hermes-skill");
        if p.join("SKILL.md").exists() {
            return Some(p);
        }
    }
    // Dev fallback: walk up from the executable to find hermes-skill/.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..6 {
            if let Some(d) = dir {
                let cand = d.join("hermes-skill");
                if cand.join("SKILL.md").exists() {
                    return Some(cand);
                }
                dir = d.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }
    None
}

#[tauri::command]
fn detect_hermes_skill_dirs() -> Vec<String> {
    hermes_install::candidate_dirs()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// Install the Hermes skill into `target_dir` (a Hermes home or skills folder),
/// and configure the active profile's .env. Returns the installed skill path.
#[tauri::command]
fn install_hermes_skill(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    target_dir: String,
) -> Result<String, String> {
    let source = resolve_skill_source(&app)
        .ok_or_else(|| "Could not locate the bundled Hermes skill source.".to_string())?;
    let s = state.settings_snapshot();
    let base_url = format!("http://127.0.0.1:{}", s.agent_api_port);
    let dest = hermes_install::install(
        &source,
        std::path::Path::new(&target_dir),
        &base_url,
        &s.agent_api_token,
    )?;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
async fn browse_folder(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let folder = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(folder.map(|f| f.to_string()))
}

// ---------------------------------------------------------------------------
// App entry
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolve and create the app data + logs directories.
            let data_dir = handle
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).ok();
            let settings_path = data_dir.join("settings.json");
            let logs_dir = data_dir.join("logs");
            std::fs::create_dir_all(&logs_dir).ok();

            // Load settings + initial scan.
            let settings = Settings::load_or_init(&settings_path);
            let scan = if settings.auto_rescan_on_startup {
                script_scanner::scan(&settings)
            } else {
                ScanResult::default()
            };
            let auto_interval = settings.auto_rescan_interval_seconds;

            let state = Arc::new(AppState::new(settings, scan, settings_path, logs_dir));
            app.manage(state.clone());

            // Tray icon + menu.
            tray::create(&handle)?;

            // Local control API on 127.0.0.1.
            local_api::start(handle.clone(), state.clone());

            // Optional, opt-in periodic rescan (the only background loop).
            if let Some(interval) = auto_interval {
                if interval > 0 {
                    let h = handle.clone();
                    let s = state.clone();
                    std::thread::spawn(move || loop {
                        std::thread::sleep(Duration::from_secs(interval));
                        rescan_and_store(&h, &s);
                    });
                }
            }

            // Auto-start configured default profile (off the main thread).
            {
                let h = handle.clone();
                let s = state.clone();
                std::thread::spawn(move || {
                    process_manager::auto_start_if_configured(&h, &s);
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it to tray instead of quitting.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            is_server_reachable,
            get_settings,
            save_settings,
            rescan_scripts,
            get_detected_profiles,
            get_scan_result,
            start_profile,
            switch_profile,
            switch_profile_by_name,
            switch_profile_by_alias,
            stop_server,
            restart_server,
            read_latest_log,
            read_log,
            list_logs,
            clear_old_logs,
            open_logs_folder,
            open_scripts_folder,
            get_agent_api_info,
            regenerate_agent_api_token,
            browse_folder,
            detect_hermes_skill_dirs,
            install_hermes_skill
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
