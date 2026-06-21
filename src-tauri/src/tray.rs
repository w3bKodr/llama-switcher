//! System tray icon + dynamically-built menu.

use crate::state::AppState;
use std::sync::Arc;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "main-tray";

/// Build the full tray menu from the latest scan result + running state.
pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<Arc<AppState>>();
    let scan = state.scan.lock().unwrap();

    let status_text = {
        let running = state.running.lock().unwrap();
        match running.as_ref() {
            Some(rp) => format!("Status: Running {}", rp.profile.alias),
            None => "Status: Stopped".to_string(),
        }
    };

    let mut menu = MenuBuilder::new(app);
    menu = menu.item(&MenuItemBuilder::with_id("open_dashboard", "Open Dashboard").build(app)?);
    menu = menu.item(
        &MenuItemBuilder::with_id("status_label", status_text)
            .enabled(false)
            .build(app)?,
    );
    menu = menu.separator();

    // Start / Switch Profile -> grouped by pretty model.
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
        for m in &models {
            let mut model_sub = SubmenuBuilder::new(app, m);
            for p in profiles.iter().filter(|p| &p.pretty_model == m) {
                let id = format!("profile::{}", p.id);
                model_sub =
                    model_sub.item(&MenuItemBuilder::with_id(id, &p.pretty_feature).build(app)?);
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
    let menu = build_menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Llama Switcher")
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
    if let Ok(menu) = build_menu(app) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_menu(Some(menu));
        }
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

/// Run a process operation off the UI thread so the tray stays responsive.
fn spawn_op<F>(app: AppHandle, op: F)
where
    F: FnOnce(AppHandle, Arc<AppState>) -> Result<crate::state::Status, String> + Send + 'static,
{
    std::thread::spawn(move || {
        let state = crate::get_state(&app);
        if let Err(e) = op(app.clone(), state) {
            let _ = tauri::Emitter::emit(&app, "warning", e);
        }
    });
}
