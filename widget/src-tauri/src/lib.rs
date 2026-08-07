use serde::Deserialize;
use serde_json::Value;
#[cfg(debug_assertions)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn hidden_windows_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitcherSettings {
    agent_api_port: u16,
    agent_api_token: String,
}

fn switcher_settings_path() -> Result<PathBuf, String> {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows roaming app-data folder is unavailable.".to_string())?;
    Ok(roaming.join("com.llamaswitcher.app").join("settings.json"))
}

fn read_switcher_settings() -> Result<SwitcherSettings, String> {
    let path = switcher_settings_path()?;
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Llama Switcher settings were not found. Start the main app once to initialize {}.",
            path.display()
        )
    })?;
    serde_json::from_str(&text)
        .map_err(|error| format!("Could not read Llama Switcher settings: {error}"))
}

fn authorized_request(method: &str, endpoint: &str) -> Result<Value, String> {
    let settings = read_switcher_settings()?;
    let url = format!("http://127.0.0.1:{}{}", settings.agent_api_port, endpoint);
    let request = match method {
        "POST" => ureq::post(&url),
        _ => ureq::get(&url),
    }
    .set(
        "Authorization",
        &format!("Bearer {}", settings.agent_api_token),
    )
    // `/status` performs live process, server and GPU probes in the main app.
    // Give a busy probe enough time to finish instead of treating a momentary
    // queue behind another request as though Llama Switcher exited.
    .timeout(Duration::from_secs(15));

    let response = request.call().map_err(|error| match error {
        ureq::Error::Status(code, response) => {
            let detail = response.into_string().unwrap_or_default();
            format!("Llama Switcher returned HTTP {code}: {detail}")
        }
        ureq::Error::Transport(_) => {
            "Llama Switcher is not running. Start the main app to enable live telemetry."
                .to_string()
        }
    })?;
    let body = response
        .into_string()
        .map_err(|error| format!("Could not read the Llama Switcher response: {error}"))?;
    serde_json::from_str(&body)
        .map_err(|error| format!("Llama Switcher returned invalid status data: {error}"))
}

fn switcher_process_is_running() -> bool {
    #[cfg(windows)]
    {
        let output = hidden_windows_command("tasklist")
            .args([
                "/FI",
                "IMAGENAME eq llama-switcher.exe",
                "/FO",
                "CSV",
                "/NH",
            ])
            .output();
        return output
            .ok()
            .filter(|result| result.status.success())
            .map(|result| {
                String::from_utf8_lossy(&result.stdout)
                    .to_ascii_lowercase()
                    .contains("llama-switcher.exe")
            })
            .unwrap_or(false);
    }

    #[cfg(not(windows))]
    false
}

#[tauri::command]
async fn is_switcher_running() -> bool {
    tauri::async_runtime::spawn_blocking(switcher_process_is_running)
        .await
        .unwrap_or(false)
}

#[tauri::command]
async fn get_llama_status() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| authorized_request("GET", "/status"))
        .await
        .map_err(|error| format!("Status worker stopped unexpectedly: {error}"))?
}

fn launch_switcher_fallback() -> Result<(), String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("LLAMA_SWITCHER_PATH") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(path) = switcher_settings_path() {
        if let Ok(executable) = std::fs::read_to_string(
            path.with_file_name("main-executable-path.txt"),
        ) {
            candidates.push(PathBuf::from(executable.trim()));
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            candidates.push(parent.join("llama-switcher.exe"));
            candidates.push(parent.join("Llama Switcher.exe"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        candidates.push(local.join("Llama Switcher").join("llama-switcher.exe"));
        candidates.push(local.join("Llama Switcher").join("Llama Switcher.exe"));
        candidates.push(
            local
                .join("Programs")
                .join("Llama Switcher")
                .join("llama-switcher.exe"),
        );
        candidates.push(
            local
                .join("Programs")
                .join("Llama Switcher")
                .join("Llama Switcher.exe"),
        );
    }
    #[cfg(debug_assertions)]
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("src-tauri")
            .join("target")
            .join("release")
            .join("llama-switcher.exe"),
    );

    for candidate in candidates {
        if candidate.is_file() {
            return Command::new(&candidate)
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("Could not launch {}: {error}", candidate.display()));
        }
    }
    Err("Llama Switcher could not be found. Set LLAMA_SWITCHER_PATH to its executable if it is installed in a custom location.".to_string())
}

#[tauri::command]
async fn open_switcher() -> Result<(), String> {
    let opened = tauri::async_runtime::spawn_blocking(|| authorized_request("POST", "/open-dashboard"))
        .await
        .map_err(|error| format!("Open worker stopped unexpectedly: {error}"))?;
    if opened.is_ok() {
        return Ok(());
    }
    launch_switcher_fallback()
}

fn show_widget(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show widget").build(app)?;
    let refresh = MenuItemBuilder::with_id("refresh", "Refresh telemetry").build(app)?;
    let open = MenuItemBuilder::with_id("open", "Open Llama Switcher").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit widget").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &refresh, &open, &quit])
        .build()?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("Llama Switcher Widget")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_widget(app),
            "refresh" => {
                let _ = app.emit("widget://refresh", ());
            }
            "open" => {
                tauri::async_runtime::spawn(async move {
                    let _ = open_switcher().await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            match event {
                TrayIconEvent::Click { button: MouseButton::Left, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => {
                    show_widget(tray.app_handle());
                }
                _ => {}
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Llama Switcher Widget")
                .build(),
        )
        .setup(|app| {
            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::App("index.html".into()),
            )
            .title("Llama Switcher Widget")
            .inner_size(390.0, 520.0)
            .min_inner_size(390.0, 520.0)
            .max_inner_size(390.0, 520.0)
            .resizable(false)
            .fullscreen(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .skip_taskbar(true)
            .always_on_top(false)
            .visible(true)
            .center()
            .build()?;
            build_tray(app)?;
            window.show()?;
            window.set_focus()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_llama_status,
            is_switcher_running,
            open_switcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running Llama Switcher Widget");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_local_api_credentials_needed_by_the_widget() {
        let settings: SwitcherSettings = serde_json::from_str(
            r#"{"agentApiPort":47891,"agentApiToken":"secret","scriptsFolder":"D:\\llama"}"#,
        )
        .unwrap();
        assert_eq!(settings.agent_api_port, 47_891);
        assert_eq!(settings.agent_api_token, "secret");
    }
}
