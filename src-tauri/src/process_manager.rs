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
use crate::state::{AppState, RunningProcess, Status, UsageState};
use crate::tray;
use serde_json::Value;
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
    probe_health(url).reachable
}

struct HealthProbe {
    reachable: bool,
    healthy: bool,
}

fn probe_health(url: &str) -> HealthProbe {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(800))
        .build();
    match agent.get(url).call() {
        Ok(response) => HealthProbe {
            reachable: true,
            healthy: response.status() >= 200 && response.status() < 400,
        },
        Err(ureq::Error::Status(code, _)) => HealthProbe {
            reachable: true,
            healthy: (200..400).contains(&code),
        },
        Err(_) => HealthProbe {
            reachable: false,
            healthy: false,
        },
    }
}

/// Build current status with a live health probe. When a reachable external
/// listener belongs to one of our detected scripts, immediately relaunch that
/// same profile under Llama Switcher so logs and lifecycle controls work.
pub fn status_with_probe(app: &AppHandle, state: &Arc<AppState>) -> Status {
    let mut s = state.status();
    let health = probe_health(&s.health_url);
    s.server_reachable = health.reachable;
    if s.running {
        s.healthy = health.healthy;
    } else if health.reachable {
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
    s.usage_state = if health.healthy && !*state.usage_probe_disabled.lock().unwrap() {
        probe_usage_state(state, &s)
    } else {
        UsageState::Unknown
    };
    s.avg_tokens_per_second = if health.healthy {
        probe_avg_tps(state, &s)
    } else {
        None
    };
    s
}

/// Read llama.cpp's cumulative generation counters from `/metrics` and return
/// the average generation tokens/sec for the current model since it started.
/// Reuses the same API key discovery as the `/slots` usage probe.
fn probe_avg_tps(state: &Arc<AppState>, status: &Status) -> Option<f64> {
    let api_key = usage_probe_api_key(state, status)?;
    let url = format!("{}/metrics", server_origin(&status.health_url, status.server_port));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(800))
        .build();
    let body = match agent
        .get(&url)
        .set("Authorization", &format!("Bearer {}", api_key))
        .call()
    {
        Ok(response) => response.into_string().ok()?,
        Err(_) => return None,
    };

    let tokens = parse_prometheus_metric(&body, "llamacpp:tokens_predicted_total")?;
    let seconds = parse_prometheus_metric(&body, "llamacpp:tokens_predicted_seconds_total")?;

    let mut tracker = state.tps.lock().unwrap();
    // Re-baseline if the model changed since the last sample.
    if tracker.profile_id != status.current_profile_id {
        tracker.profile_id = status.current_profile_id.clone();
        tracker.baseline = None;
    }
    let (base_tokens, base_seconds) = *tracker.baseline.get_or_insert((tokens, seconds));

    let d_tokens = tokens - base_tokens;
    let d_seconds = seconds - base_seconds;
    if d_seconds > 0.05 && d_tokens > 0.0 {
        Some(d_tokens / d_seconds)
    } else {
        None
    }
}

/// Parse a single unlabeled Prometheus metric value (`name value`).
fn parse_prometheus_metric(body: &str, name: &str) -> Option<f64> {
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() == Some(name) {
            return parts.next()?.parse::<f64>().ok();
        }
    }
    None
}

fn probe_usage_state(state: &Arc<AppState>, status: &Status) -> UsageState {
    let api_key = match usage_probe_api_key(state, status) {
        Some(key) => key,
        None => return UsageState::Unknown,
    };

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(800))
        .build();

    let url = format!("{}/slots", server_origin(&status.health_url, status.server_port));
    let auth = format!("Bearer {}", api_key);
    let response = match agent.get(&url).set("Authorization", &auth).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(401 | 403, _)) => {
            *state.usage_probe_disabled.lock().unwrap() = true;
            return UsageState::Unknown;
        }
        Err(_) => return UsageState::Unknown,
    };

    let body = match response.into_string() {
        Ok(body) => body,
        Err(_) => return UsageState::Unknown,
    };

    let value: Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return UsageState::Unknown,
    };

    infer_usage_state(&value).unwrap_or(UsageState::Unknown)
}

fn usage_probe_api_key(state: &Arc<AppState>, status: &Status) -> Option<String> {
    resolve_api_key(state, status.script_path.as_deref())
}

/// Resolve the llama.cpp bearer key: the profile script's `LLAMA_API_KEY` /
/// `--api-key` first, then the settings fallback. Shared by the usage probe,
/// the metrics probe, and the benchmark runner.
pub fn resolve_api_key(state: &Arc<AppState>, script_path: Option<&str>) -> Option<String> {
    let script_key = script_path
        .and_then(api_key_from_script)
        .and_then(|key| non_empty(key.trim()));
    let fallback_settings_key = state
        .settings_snapshot()
        .llama_server_api_key
        .and_then(|key| non_empty(key.trim()));
    script_key.or(fallback_settings_key)
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn api_key_from_script(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_api_key_from_script(&text)
}

fn parse_api_key_from_script(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if trimmed.is_empty() || lower.starts_with("rem ") || lower.starts_with('#') {
            continue;
        }

        if let Some(value) = parse_env_assignment(trimmed, "LLAMA_API_KEY") {
            return Some(value);
        }
        if let Some(value) = parse_flag_value(trimmed, "--api-key") {
            return Some(value);
        }
    }
    None
}

fn parse_env_assignment(line: &str, name: &str) -> Option<String> {
    let mut s = line.trim();
    if s.to_ascii_lowercase().starts_with("set ") {
        s = s[4..].trim();
    }
    if s.starts_with('$') {
        s = s.trim_start_matches('$');
        if let Some(rest) = s.strip_prefix("env:") {
            s = rest;
        }
    }
    s = s.trim_matches('"').trim_matches('\'').trim();

    let (left, right) = s.split_once('=')?;
    let left = left.trim().trim_matches('"').trim_matches('\'');
    if !left.eq_ignore_ascii_case(name) {
        return None;
    }

    Some(clean_script_value(right))
}

fn parse_flag_value(line: &str, flag: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let flag_lower = flag.to_ascii_lowercase();
    let pos = lower.find(&flag_lower)?;
    let after = line[pos + flag.len()..].trim_start();
    let value = if let Some(value) = after.strip_prefix('=') {
        value.trim_start()
    } else {
        after
    };
    Some(clean_script_value(value))
}

fn clean_script_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('^')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

pub fn server_origin(health_url: &str, server_port: u16) -> String {
    if let Some((scheme, rest)) = health_url.split_once("://") {
        let host = rest.split('/').next().unwrap_or_default();
        if !host.is_empty() {
            return format!("{}://{}", scheme, host);
        }
    }
    format!("http://127.0.0.1:{}", server_port)
}

fn infer_usage_state(value: &Value) -> Option<UsageState> {
    match value {
        Value::Array(items) => {
            let mut saw_free = false;
            for item in items {
                match infer_usage_state(item) {
                    Some(UsageState::Busy) => return Some(UsageState::Busy),
                    Some(UsageState::Free) => saw_free = true,
                    _ => {}
                }
            }
            saw_free.then_some(UsageState::Free)
        }
        Value::Object(map) => {
            for key in [
                "slots",
                "data",
                "result",
                "items",
                "list",
                "slot",
                "value",
            ] {
                if let Some(nested) = map.get(key) {
                    if let Some(state) = infer_usage_state(nested) {
                        return Some(state);
                    }
                }
            }

            for key in [
                "is_processing",
                "processing",
                "is_generating",
                "generating",
                "busy",
                "is_busy",
                "active",
                "is_idle",
                "idle",
                "in_use",
            ] {
                if let Some(flag) = map.get(key).and_then(Value::as_bool) {
                    return Some(match key {
                        "is_idle" | "idle" => {
                            if flag {
                                UsageState::Free
                            } else {
                                UsageState::Busy
                            }
                        }
                        _ => {
                            if flag {
                                UsageState::Busy
                            } else {
                                UsageState::Free
                            }
                        }
                    });
                }
            }

            for key in ["state", "status"] {
                if let Some(text) = map.get(key).and_then(Value::as_str) {
                    let state = text.to_ascii_lowercase();
                    if [
                        "busy",
                        "processing",
                        "generating",
                        "running",
                        "prompt",
                        "decode",
                    ]
                    .iter()
                    .any(|needle| state.contains(needle))
                    {
                        return Some(UsageState::Busy);
                    }
                    if [
                        "free",
                        "idle",
                        "ready",
                        "available",
                        "waiting",
                    ]
                    .iter()
                    .any(|needle| state.contains(needle))
                    {
                        return Some(UsageState::Free);
                    }
                }
            }

            for key in ["n_processing", "processing_count", "active_requests", "queued_requests"] {
                if let Some(count) = map.get(key).and_then(Value::as_u64) {
                    return Some(if count > 0 {
                        UsageState::Busy
                    } else {
                        UsageState::Free
                    });
                }
            }

            None
        }
        _ => None,
    }
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

/// Terminate every stray server-binary process to guarantee a single instance.
/// This is the catch-all that handles detached/orphaned servers (whose parent
/// link is broken so a tree-walk can't find them) and strays bound to a
/// non-configured port, which per-port reclamation alone would miss.
fn enforce_single_server(app: &AppHandle, settings: &Settings, reason: &str) -> usize {
    if settings.server_process_names.is_empty() {
        return 0;
    }
    let count = process_tree::kill_all_by_image(&settings.server_process_names);
    if count > 0 {
        let _ = app.emit(
            "warning",
            format!(
                "{}: terminated {} stray server process{} to enforce a single instance.",
                reason,
                count,
                if count == 1 { "" } else { "es" }
            ),
        );
    }
    count
}

/// Stop the server on the configured port, whether it was launched by Llama
/// Switcher or by another startup mechanism. Blocks until the port is free or
/// the stop timeout elapses. Public entry — serialized via `op_lock`.
pub fn stop_server(app: &AppHandle, state: &Arc<AppState>) -> Result<Status, String> {
    let _op = state.op_lock.lock().unwrap();
    stop_locked(app, state)
}

/// Stop implementation. Assumes the caller already holds `op_lock`.
fn stop_locked(app: &AppHandle, state: &Arc<AppState>) -> Result<Status, String> {
    let settings = state.settings_snapshot();
    let mut rp = match state.running.lock().unwrap().take() {
        Some(rp) => rp,
        None => {
            stop_external_listener(app, &settings, "Stop requested")?;
            enforce_single_server(app, &settings, "Stop");
            notify(app, state);
            *state.external_pid_checked.lock().unwrap() = None;
            *state.usage_probe_disabled.lock().unwrap() = false;
            return Ok(state.status());
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

    // Final sweep: terminate any stray server processes (detached, orphaned, or
    // bound to a different port) so exactly zero servers remain after a stop.
    let swept = enforce_single_server(app, &settings, "Stop");
    if swept > 0 {
        logging::append_line(
            &log_path,
            &format!("Swept {} stray server process(es) by image name.", swept),
        );
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
    *state.usage_probe_disabled.lock().unwrap() = false;

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

    // Serialize the entire start/switch so two activations cannot race and
    // leave two servers running. Held until the new server is launched.
    let _op = state.op_lock.lock().unwrap();

    // Stop any currently running managed process (handles both switch & restart).
    if state.running.lock().unwrap().is_some() {
        stop_locked(app, state)?;
    }

    // Always take ownership of the configured server port. This covers servers
    // launched at Windows sign-in, from a terminal, or by another application.
    ensure_port_available(app, &settings)?;

    // Guarantee no stray server (any port, including orphans) survives before
    // we launch exactly one.
    enforce_single_server(app, &settings, "Start / Switch");
    *state.external_pid_checked.lock().unwrap() = None;
    *state.usage_probe_disabled.lock().unwrap() = false;
    // Reset the tokens/sec average so it re-accumulates for the new model.
    {
        let mut tracker = state.tps.lock().unwrap();
        tracker.profile_id = Some(profile.id.clone());
        tracker.baseline = None;
    }

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
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to launch script: {}", e))?;

    if let Some(stdout) = child.stdout.take() {
        logging::spawn_filtered_pipe(log_path.to_path_buf(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        logging::spawn_filtered_pipe(log_path.to_path_buf(), stderr);
    }

    Ok(child)
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
        for attempt in 0..15 {
            if state.find_profile(&id).is_some() {
                if let Err(error) = activate_profile(app, state, &id) {
                    let _ = app.emit("warning", format!("Auto-start failed: {}", error));
                }
                return;
            }

            crate::rescan_and_store(app, state);
            if state.find_profile(&id).is_some() {
                if let Err(error) = activate_profile(app, state, &id) {
                    let _ = app.emit("warning", format!("Auto-start failed: {}", error));
                }
                return;
            }

            if attempt < 14 {
                thread::sleep(Duration::from_secs(2));
            }
        }

        let _ = app.emit(
            "warning",
            format!("Auto-start profile was not found after scanning: {}", id),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_prometheus_generation_counters() {
        // Real sample captured from a running llama.cpp `/metrics`.
        let body = "# HELP llamacpp:tokens_predicted_total ...\n\
            llamacpp:prompt_tokens_total 2.67172e+06\n\
            llamacpp:tokens_predicted_total 373356\n\
            llamacpp:tokens_predicted_seconds_total 2872.4\n\
            llamacpp:predicted_tokens_seconds 129.981\n";
        let tokens = parse_prometheus_metric(body, "llamacpp:tokens_predicted_total").unwrap();
        let seconds =
            parse_prometheus_metric(body, "llamacpp:tokens_predicted_seconds_total").unwrap();
        assert_eq!(tokens, 373356.0);
        assert!((seconds - 2872.4).abs() < 1e-6);
        // Scientific notation parses too.
        let prompt = parse_prometheus_metric(body, "llamacpp:prompt_tokens_total").unwrap();
        assert!((prompt - 2_671_720.0).abs() < 1.0);
        // Average generation speed ≈ tokens / seconds.
        assert!(((tokens / seconds) - 129.98).abs() < 0.1);
        assert!(parse_prometheus_metric(body, "llamacpp:does_not_exist").is_none());
    }

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

    #[test]
    fn infers_busy_usage_from_slots_payload() {
        let payload = json!({
            "slots": [
                { "id": 0, "state": "idle" },
                { "id": 1, "is_processing": true }
            ]
        });

        assert_eq!(infer_usage_state(&payload), Some(UsageState::Busy));
    }

    #[test]
    fn infers_free_usage_from_slots_payload() {
        let payload = json!([
            { "id": 0, "is_processing": false },
            { "id": 1, "status": "idle" }
        ]);

        assert_eq!(infer_usage_state(&payload), Some(UsageState::Free));
    }

    #[test]
    fn parses_cmd_llama_api_key_assignment() {
        let script = r#"
            @echo off
            set "LLAMA_API_KEY=sk-test-123"
            llama-server.exe --port 1234
        "#;

        assert_eq!(parse_api_key_from_script(script).as_deref(), Some("sk-test-123"));
    }

    #[test]
    fn parses_api_key_flag_assignment() {
        let script = r#"
            llama-server.exe ^
              --api-key sk-flag-456 ^
              --port 1234
        "#;

        assert_eq!(parse_api_key_from_script(script).as_deref(), Some("sk-flag-456"));
    }
}
