//! System tray icon + dynamically-built menu.

use crate::state::{AppState, Status, UsageState};
use std::sync::Arc;
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "main-tray";

/// Build the full tray menu from the latest scan result + running state.
pub fn build_menu(app: &AppHandle, status: Option<&Status>) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<Arc<AppState>>();
    let scan = state.scan.lock().unwrap();
    let snapshot = status.cloned().unwrap_or_else(|| state.status());

    let status_text = if snapshot.running {
        format!(
            "Status: {} ({})",
            snapshot.alias.clone().unwrap_or_else(|| "Running".into()),
            usage_label(&snapshot)
        )
    } else if snapshot.server_reachable {
        format!("Status: External server ({})", usage_label(&snapshot))
    } else {
        "Status: Down".to_string()
    };

    let mut menu = MenuBuilder::new(app);
    menu = menu.item(&MenuItemBuilder::with_id("open_dashboard", "Open Dashboard").build(app)?);
    menu = menu.item(
        &MenuItemBuilder::with_id("status_label", status_text)
            .enabled(false)
            .build(app)?,
    );
    menu = menu.separator();

    let mut profile_sub = SubmenuBuilder::new(app, "Start / Switch Profile");
    let profiles = &scan.profiles;
    let mut models: Vec<String> = vec![];
    for p in profiles {
        if !models.contains(&p.pretty_model) {
            models.push(p.pretty_model.clone());
        }
    }
    if models.is_empty() {
        profile_sub = profile_sub.item(
            &MenuItemBuilder::with_id("no_profiles", "(no profiles found)")
                .enabled(false)
                .build(app)?,
        );
    } else {
        for model in &models {
            let mut model_sub = SubmenuBuilder::new(app, model);
            for profile in profiles.iter().filter(|p| &p.pretty_model == model) {
                model_sub = model_sub.item(
                    &MenuItemBuilder::with_id(format!("profile::{}", profile.id), &profile.pretty_feature)
                        .build(app)?,
                );
            }
            profile_sub = profile_sub.item(&model_sub.build()?);
        }
    }
    menu = menu.item(&profile_sub.build()?);
    menu = menu.separator();

    menu = menu.item(&MenuItemBuilder::with_id("restart", "Restart Current").build(app)?);
    menu = menu.item(&MenuItemBuilder::with_id("stop", "Stop Server").build(app)?);
    menu = menu.separator();
    menu = menu.item(&MenuItemBuilder::with_id("rescan", "Rescan Scripts Folder").build(app)?);
    menu = menu.item(&MenuItemBuilder::with_id("open_scripts", "Open Scripts Folder").build(app)?);
    menu = menu.item(&MenuItemBuilder::with_id("open_logs", "Open Logs Folder").build(app)?);
    menu = menu.item(&MenuItemBuilder::with_id("settings", "Settings").build(app)?);
    menu = menu.separator();
    menu = menu.item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?);

    menu.build()
}

/// Create the tray icon once at startup.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let initial_status = app.state::<Arc<AppState>>().status();
    let menu = build_menu(app, Some(&initial_status))?;
    let icon = tray_icon_for_status(&initial_status);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(tray_tooltip(&initial_status))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| handle_menu_event(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_dashboard(tray.app_handle(), None);
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuild and apply the menu (called after rescans and state changes).
pub fn rebuild(app: &AppHandle, _state: &Arc<AppState>) {
    let mut status = app.state::<Arc<AppState>>().status();
    if status.running {
        status.server_reachable = true;
    }
    refresh(app, _state, &status);
}

pub fn refresh(app: &AppHandle, _state: &Arc<AppState>, status: &Status) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app, Some(status)) {
            let _ = tray.set_menu(Some(menu));
        }
        refresh_visual(app, status);
    }
}

pub fn refresh_visual(app: &AppHandle, status: &Status) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(tray_icon_for_status(status)));
        let _ = tray.set_tooltip(Some(tray_tooltip(status)));
    }
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    let app = app.clone();
    if let Some(profile_id) = id.strip_prefix("profile::") {
        let profile_id = profile_id.to_string();
        spawn_op(app, move |app, state| {
            crate::process_manager::activate_profile(&app, &state, &profile_id)
        });
        return;
    }

    match id {
        "open_dashboard" => crate::show_dashboard(&app, None),
        "settings" => crate::show_dashboard(&app, Some("settings")),
        "restart" => spawn_op(app, |app, state| {
            crate::process_manager::restart_server(&app, &state)
        }),
        "stop" => spawn_op(app, |app, state| {
            crate::process_manager::stop_server(&app, &state)
        }),
        "rescan" => {
            let state = crate::get_state(&app);
            crate::rescan_and_store(&app, &state);
        }
        "open_scripts" => {
            let folder = crate::get_state(&app).settings_snapshot().scripts_folder;
            crate::open_folder(&folder);
        }
        "open_logs" => {
            let dir = crate::get_state(&app).logs_dir.to_string_lossy().to_string();
            crate::open_folder(&dir);
        }
        "quit" => crate::quit_app(&app),
        _ => {}
    }
}

fn spawn_op<F>(app: AppHandle, op: F)
where
    F: FnOnce(AppHandle, Arc<AppState>) -> Result<crate::state::Status, String> + Send + 'static,
{
    std::thread::spawn(move || {
        let state = crate::get_state(&app);
        if crate::benchmark::is_running(&state) {
            let _ = tauri::Emitter::emit(
                &app,
                "warning",
                "A benchmark is running; server controls are disabled until it finishes.".to_string(),
            );
            return;
        }
        if let Err(e) = op(app.clone(), state) {
            let _ = tauri::Emitter::emit(&app, "warning", e);
        }
    });
}

fn usage_label(status: &Status) -> &'static str {
    if !status.server_reachable {
        "Down"
    } else {
        match status.usage_state {
            UsageState::Busy => "In use",
            UsageState::Free => "Free",
            UsageState::Unknown => {
                if status.running && !status.healthy {
                    "Starting"
                } else {
                    "Healthy"
                }
            }
        }
    }
}

fn tray_tooltip(status: &Status) -> String {
    let name = status
        .alias
        .clone()
        .or(status.current_profile_name.clone())
        .unwrap_or_else(|| "Llama Switcher".to_string());
    format!("{} - {}", name, usage_label(status))
}

fn tray_icon_for_status(status: &Status) -> Image<'static> {
    let (r, g, b) = if !status.server_reachable {
        (240, 106, 106)
    } else if status.usage_state == UsageState::Busy || (status.running && !status.healthy) {
        (242, 194, 97)
    } else {
        (76, 214, 140)
    };

    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    let outer = 13.0f32;
    let inner = 9.5f32;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;

            let pixel = if dist <= inner {
                let glow = (1.0 - (dist / inner).min(1.0)) * 0.18;
                [
                    ((r as f32) + (255.0 - r as f32) * glow).round() as u8,
                    ((g as f32) + (255.0 - g as f32) * glow).round() as u8,
                    ((b as f32) + (255.0 - b as f32) * glow).round() as u8,
                    255,
                ]
            } else if dist <= outer {
                [255, 255, 255, 235]
            } else if dist <= outer + 1.8 {
                [255, 255, 255, 48]
            } else {
                [0, 0, 0, 0]
            };

            rgba[idx..idx + 4].copy_from_slice(&pixel);
        }
    }

    Image::new_owned(rgba, size, size)
}
