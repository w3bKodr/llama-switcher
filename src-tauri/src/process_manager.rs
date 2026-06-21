//! Start / stop / switch / restart the single managed llama.cpp server.
//!
//! All long-running work (waiting for shutdown, health polling) happens off the
//! UI thread: Tauri commands wrap these in `spawn_blocking`, the local API calls
//! them from its own thread, and health polling runs in a detached thread.

use crate::alias_formatter::normalize_alias;
use crate::logging;
use crate::process_tree;
use crate::script_scanner::Profile;
use crate::settings::{DefaultProfileMode, Settings};
use crate::state::{AppState, RunningProcess, Status};
use crate::tray;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Emit the current status to the frontend and refresh the tray menu.
pub fn notify(app: &AppHandle, state: &Arc<AppState>) {
    let status = state.status();
    let _ = app.emit("status-changed", &status);
    tray::rebuild(app, state);
}

/// Quick health probe: returns true if the URL produced any HTTP response
/// (even a 4xx/5xx), i.e. something is actually listening and answering.
pub fn probe_reachable(url: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(800))
        .build();
    match agent.get(url).call() {
        Ok(_) => true,
        Err(ureq::Error::Status(_, _)) => true,
        Err(_) => false,
    }
}

/// Build current status with a live health probe. When a reachable external
/// listener belongs to one of our detected scripts, immediately relaunch that
/// same profile under Llama Switcher so logs and lifecycle controls work.
pub fn status_with_probe(app: &AppHandle, state: &Arc<AppState>) -> Status {
    let mut s = state.status();
    let reachable = probe_reachable(&s.health_url);
    s.server_reachable = reachable;
    if s.running {
        s.healthy = reachable;
    } else if reachable {
        let _takeover = state.takeover_lock.lock().unwrap();

        // Another caller may have completed takeover while we waited.
        let current = state.status();
        if current.running {
            return current;
        }

        if let Some(pid) = pid_on_port(s.server_port) {
            s.pid = Some(pid);
            let already_checked = *state.external_pid_checked.lock().unwrap() == Some(pid);
            if !already_checked {
                *state.external_pid_checked.lock().unwrap() = Some(pid);
                if let Some(profile) = identify_external_profile(state, pid) {
                    let _ = app.emit(
                        "warning",
                        format!(
                            "Taking control of externally started {} (PID {}).",
                            profile.alias, pid
                        ),
                    );
                    match activate_profile(app, state, &profile.id) {
                        Ok(status) => {
                            if let Some(running) = state.running.lock().unwrap().as_ref() {
                                logging::append_line(
                                    &running.log_path,
                                    &format!(
                                        "Took control from externally started listener PID {}.",
                                        pid
                                    ),
                                );
                            }
                            return status;
                        }
                        Err(error) => {
                            let _ = app.emit(
                                "warning",
                                format!("Could not take control of external server: {}", error),
                            );
                        }
                    }
                }
            }
        }
    } else {
        *state.external_pid_checked.lock().unwrap() = None;
    }
    s
}

/// Query the listener's process ancestry and match an ancestor command line to
/// a detected startup script. The common external-launch shape is
/// `cmd.exe /C "D:\...\start - model - feature.cmd" -> llama-server.exe`.
fn identify_external_profile(state: &Arc<AppState>, listener_pid: u32) -> Option<Profile> {
    let command_lines = process_ancestry_command_lines(listener_pid);
    match_profile_command_lines(&state.profiles(), &command_lines)
}

fn match_profile_command_lines(
    profiles: &[Profile],
    command_lines: &[String],
) -> Option<Profile> {
    let lines: Vec<String> = command_lines.iter().map(|line| line.to_lowercase()).collect();
    profiles.iter().find_map(|profile| {
        let path = profile.script_path.to_lowercase();
        let filename = std::path::Path::new(&profile.script_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())?;
        lines
            .iter()
            .any(|line| line.contains(&path) || line.contains(&filename))
            .then(|| profile.clone())
    })
}

#[cfg(windows)]
fn process_ancestry_command_lines(pid: u32) -> Vec<String> {
    let script = format!(
        "$p=Get-CimInstance Win32_Process -Filter 'ProcessId={}'; ",
        pid
    ) + "while($null -ne $p -and $p.ProcessId -ne 0) { "
        + "if($p.CommandLine) { [Console]::Out.WriteLine($p.CommandLine) }; "
        + "$parent=$p.ParentProcessId; "
        + "$p=Get-CimInstance Win32_Process -Filter \"ProcessId=$parent\" -ErrorAction SilentlyContinue }";
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn process_ancestry_command_lines(_pid: u32) -> Vec<String> {
    vec![]
}

// ---------------------------------------------------------------------------
// Port helpers
// ---------------------------------------------------------------------------

/// Is the TCP port free on 127.0.0.1?
/// Find the PID of the process listening on `port`, if any (netstat).
/// Works for both IPv4 (127.0.0.1:port) and IPv6 ([::1]:port) listeners.
pub fn pid_on_port(port: u16) -> Option<u32> {
    let mut cmd = Command::new("netstat");
    cmd.args(["-ano", "-p", "TCP"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let needle = format!(":{}", port);
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // proto local foreign state pid
        if f.len() >= 5 && f[0].eq_ignore_ascii_case("TCP") && f[3] == "LISTENING" {
            if f[1].ends_with(&needle) {
                if let Ok(pid) = f[4].parse::<u32>() {
                    return Some(pid);
                }
            }
        }
    }
    None
}

/// Check whether the port is free by looking for any LISTENING process.
/// Uses netstat so it works for both IPv4 and IPv6 bindings.
pub fn is_port_free(port: u16) -> bool {
    pid_on_port(port).is_none()
}

// ---------------------------------------------------------------------------
// Stop
// ---------------------------------------------------------------------------

/// Stop the server on the configured port, whether it was launched by Llama
/// Switcher or by another startup mechanism. Blocks until the port is free or
/// the stop timeout elapses.
pub fn stop_server(app: &AppHandle, state: &Arc<AppState>) -> Result<Status, String> {
    let settings = state.settings_snapshot();
    let mut rp = match state.running.lock().unwrap().take() {
        Some(rp) => rp,
        None => {
            stop_external_listener(app, &settings, "Stop requested")?;
            notify(app, state);
            *state.external_pid_checked.lock().unwrap() = None;
            return Ok(status_with_probe(app, state));
        }
    };
    notify(app, state);

    let log_path = rp.log_path.clone();
    logging::append_line(&log_path, "Stop requested.");

    // 1. Kill the managed process tree (shell + all known descendants).
    let tree = process_tree::descendants(rp.pid);
    logging::append_line(&log_path, &format!("Killing tree of {} processes (root PID {}).", tree.len(), rp.pid));
    process_tree::kill_tree(rp.pid);
    let _ = rp.child.wait();

    // 2. Aggressive port cleanup loop: keep finding and killing the PARENT
    //    tree of whatever process is holding the port. This handles restart
    //    loops (parent script respawns child), orphaned grandchildren, and
    //    reparented processes that survive the initial tree kill.
    let deadline = Instant::now() + Duration::from_secs(settings.stop_timeout_seconds.max(1));
    let mut killed_count = 0;
    while Instant::now() < deadline {
        if is_port_free(settings.server_port) {
            break;
        }
        if let Some(pid) = pid_on_port(settings.server_port) {
            killed_count += 1;
            logging::append_line(&log_path, &format!("Port still occupied by PID {}; killing parent tree.", pid));
            process_tree::kill_parent_tree(pid);
            thread::sleep(Duration::from_millis(500));
        } else {
            // Port not free but no listener found (e.g. TIME_WAIT) — wait.
            thread::sleep(Duration::from_millis(250));
        }
    }

    let port_free = is_port_free(settings.server_port);
    logging::append_line(
        &log_path,
        &format!(
            "Stopped at {}. Port {} free: {}. Killed {} extra process{}.",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
            settings.server_port,
            port_free,
            killed_count,
            if killed_count == 1 { "" } else { "es" }
        ),
    );

    notify(app, state);

    if !port_free {
        return Err(format!(
            "Server stopped but port {} is still in use.",
            settings.server_port
        ));
    }
    Ok(state.status())
}

// ---------------------------------------------------------------------------
// Activate (start / switch / restart all funnel through here)
// ---------------------------------------------------------------------------

/// Start the given profile, stopping any currently running profile first.
pub fn activate_profile(
    app: &AppHandle,
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<Status, String> {
    let settings = state.settings_snapshot();
    let profile = state
        .find_profile(profile_id)
        .ok_or_else(|| format!("Unknown profile id: {}", profile_id))?;

    // Validate the script and working directory still exist.
    if !std::path::Path::new(&profile.script_path).is_file() {
        return Err(format!("Script not found: {}", profile.script_path));
    }
    if !std::path::Path::new(&profile.working_directory).is_dir() {
        return Err(format!(
            "Working directory not found: {}",
            profile.working_directory
        ));
    }

    // Stop any currently running managed process (handles both switch & restart).
    if state.running.lock().unwrap().is_some() {
        stop_server(app, state)?;
    }

    // Always take ownership of the configured server port. This covers servers
    // launched at Windows sign-in, from a terminal, or by another application.
    ensure_port_available(app, &settings)?;
    *state.external_pid_checked.lock().unwrap() = None;

    // Create the run log and launch the script.
    let log_path = logging::create_run_log(&state.logs_dir, &profile, None);
    let child = spawn_script(&profile, &log_path)?;
    let pid = child.id();
    let started_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    logging::append_line(&log_path, &format!("Launched shell PID {}.", pid));

    {
        let mut running = state.running.lock().unwrap();
        *running = Some(RunningProcess {
            profile: profile.clone(),
            pid,
            child,
            started_at,
            log_path: log_path.clone(),
            healthy: false,
        });
    }

    // Persist last-used profile.
    {
        let mut s = state.settings.lock().unwrap();
        s.last_used_profile_id = Some(profile.id.clone());
        let _ = s.save(&state.settings_path);
    }

    notify(app, state);
    spawn_health_poller(app.clone(), Arc::clone(state), profile.id.clone(), pid);

    Ok(state.status())
}

fn ensure_port_available(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    if is_port_free(settings.server_port) {
        return Ok(());
    }
    stop_external_listener(app, settings, "Start / Switch requested")
}

/// Stop a listener not represented in `state.running`. Aggressively kill
/// whatever is on the port until it clears.
fn stop_external_listener(
    app: &AppHandle,
    settings: &Settings,
    reason: &str,
) -> Result<(), String> {
    if is_port_free(settings.server_port) {
        return Ok(());
    }

    let deadline = Instant::now() + Duration::from_secs(settings.stop_timeout_seconds.max(1));
    while Instant::now() < deadline {
        if is_port_free(settings.server_port) {
            return Ok(());
        }
        match pid_on_port(settings.server_port) {
            Some(pid) => {
                let _ = app.emit(
                    "warning",
                    format!(
                        "{}: killing parent tree of PID {} on server port {}.",
                        reason, pid, settings.server_port
                    ),
                );
                // Kill the PARENT tree, not just the listener.
                // This terminates the restart-loop script AND all its children.
                process_tree::kill_parent_tree(pid);
                thread::sleep(Duration::from_millis(500));
            }
            None => {
                thread::sleep(Duration::from_millis(250));
            }
        }
    }

    Err(format!(
        "Port {} is still in use after {}s.",
        settings.server_port, settings.stop_timeout_seconds
    ))
}

fn spawn_script(profile: &Profile, log_path: &std::path::Path) -> Result<std::process::Child, String> {
    let out = logging::open_for_append(log_path).map_err(|e| e.to_string())?;
    let err = logging::open_for_append(log_path).map_err(|e| e.to_string())?;

    let ext = profile.extension.to_lowercase();
    let mut cmd;
    if ext == ".ps1" {
        cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        cmd.arg(&profile.script_path);
    } else {
        // .cmd / .bat — quote the path so spaces work under cmd.exe's parser.
        cmd = Command::new("cmd");
        #[cfg(windows)]
        {
            cmd.raw_arg("/C");
            cmd.raw_arg(format!("\"{}\"", profile.script_path));
        }
        #[cfg(not(windows))]
        {
            cmd.args(["/C", &profile.script_path]);
        }
    }

    cmd.current_dir(&profile.working_directory);
    cmd.stdout(Stdio::from(out));
    cmd.stderr(Stdio::from(err));
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.spawn().map_err(|e| format!("Failed to launch script: {}", e))
}

/// Poll the health URL until healthy or timeout, then stop polling. This is the
/// only timed loop and it is bounded to the startup window.
fn spawn_health_poller(app: AppHandle, state: Arc<AppState>, profile_id: String, pid: u32) {
    thread::spawn(move || {
        let settings = state.settings_snapshot();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(3))
            .build();
        let deadline = Instant::now() + Duration::from_secs(settings.health_check_timeout_seconds.max(1));

        loop {
            // Bail if this run is no longer the active one.
            {
                let running = state.running.lock().unwrap();
                match running.as_ref() {
                    Some(rp) if rp.profile.id == profile_id && rp.pid == pid => {}
                    _ => return,
                }
            }

            let healthy = agent
                .get(&settings.health_url)
                .call()
                .map(|r| r.status() >= 200 && r.status() < 400)
                .unwrap_or(false);

            if healthy {
                let mut running = state.running.lock().unwrap();
                if let Some(rp) = running.as_mut() {
                    if rp.profile.id == profile_id && rp.pid == pid {
                        rp.healthy = true;
                        logging::append_line(&rp.log_path, "Health check: HEALTHY.");
                    }
                }
                drop(running);
                notify(&app, &state);
                return;
            }

            if Instant::now() >= deadline {
                let running = state.running.lock().unwrap();
                if let Some(rp) = running.as_ref() {
                    logging::append_line(&rp.log_path, "Health check timed out.");
                }
                return;
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

// ---------------------------------------------------------------------------
// Restart
// ---------------------------------------------------------------------------

pub fn restart_server(app: &AppHandle, state: &Arc<AppState>) -> Result<Status, String> {
    let current = state
        .running
        .lock()
        .unwrap()
        .as_ref()
        .map(|rp| rp.profile.id.clone());
    match current {
        Some(id) => activate_profile(app, state, &id),
        None => Err("No server is currently running to restart.".into()),
    }
}

// ---------------------------------------------------------------------------
// Alias / name resolution
// ---------------------------------------------------------------------------

/// Resolve a human alias to exactly one profile id, or return an ambiguity error.
pub fn resolve_alias(state: &Arc<AppState>, alias: &str) -> Result<String, String> {
    let target = normalize_alias(alias);
    let profiles = state.profiles();
    let matches: Vec<&Profile> = profiles
        .iter()
        .filter(|p| normalize_alias(&p.alias) == target)
        .collect();
    match matches.len() {
        1 => Ok(matches[0].id.clone()),
        0 => Err(format!("No profile matches alias '{}'.", alias)),
        _ => Err(format!(
            "Alias '{}' is ambiguous. Matches: {}",
            alias,
            matches
                .iter()
                .map(|p| p.alias.clone())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn resolve_name(state: &Arc<AppState>, model: &str, feature: &str) -> Result<String, String> {
    let m = normalize_alias(model);
    let f = normalize_alias(feature);
    let profiles = state.profiles();
    let matches: Vec<&Profile> = profiles
        .iter()
        .filter(|p| normalize_alias(&p.pretty_model) == m && normalize_alias(&p.pretty_feature) == f)
        .collect();
    match matches.len() {
        1 => Ok(matches[0].id.clone()),
        0 => Err(format!("No profile matches model '{}' feature '{}'.", model, feature)),
        _ => Err(format!(
            "Model '{}' feature '{}' is ambiguous. Matches: {}",
            model,
            feature,
            matches
                .iter()
                .map(|p| p.alias.clone())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

// ---------------------------------------------------------------------------
// Auto-start on launch
// ---------------------------------------------------------------------------

pub fn auto_start_if_configured(app: &AppHandle, state: &Arc<AppState>) {
    let settings = state.settings_snapshot();
    let id = match settings.default_profile_mode {
        DefaultProfileMode::None => None,
        DefaultProfileMode::LastUsed => settings.last_used_profile_id.clone(),
        DefaultProfileMode::Specific => settings.default_profile_id.clone(),
    };
    if let Some(id) = id {
        if state.find_profile(&id).is_some() {
            let _ = activate_profile(app, state, &id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(path: &str, id: &str) -> Profile {
        Profile {
            id: id.into(),
            raw_model: "qwen-27B".into(),
            raw_feature: "Vision".into(),
            pretty_model: "Qwen-27B".into(),
            pretty_feature: "Vision".into(),
            alias: "Qwen-27B Vision".into(),
            display_name: "Qwen-27B Vision".into(),
            script_path: path.into(),
            working_directory: r"D:\llama".into(),
            extension: ".cmd".into(),
        }
    }

    #[test]
    fn matches_external_profile_from_parent_command_line() {
        let profiles = vec![
            profile(r"D:\llama\start - qwen-9B - MTP.cmd", "qwen-9b-mtp"),
            profile(
                r"D:\llama\start - qwen-27B - Vision.cmd",
                "qwen-27b-vision",
            ),
        ];
        let lines = vec![
            r#"E:\llama.cpp\llama-server.exe --port 1234"#.to_string(),
            r#""cmd" /C "D:\llama\start - qwen-27B - Vision.cmd""#.to_string(),
        ];

        let matched = match_profile_command_lines(&profiles, &lines).unwrap();
        assert_eq!(matched.id, "qwen-27b-vision");
    }

    #[test]
    fn external_profile_match_is_case_insensitive() {
        let profiles = vec![profile(
            r"D:\llama\start - qwen-27B - Vision.cmd",
            "qwen-27b-vision",
        )];
        let lines = vec![r#"CMD /C "START - QWEN-27B - VISION.CMD""#.to_string()];

        assert!(match_profile_command_lines(&profiles, &lines).is_some());
    }
}
