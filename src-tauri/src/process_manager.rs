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
use crate::state::{AppState, RunningProcess, Status, UsageState, VramProcess, VramStatus};
use crate::tray;
use serde_json::Value;
use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Emit the current status to the frontend and refresh the tray menu.
pub fn notify(app: &AppHandle, state: &Arc<AppState>) {
    if state.shutting_down.load(Ordering::Relaxed) {
        return;
    }
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
    s.vram = probe_vram(s.pid);
    s
}

/// Query NVIDIA GPU memory without adding a CUDA dependency. The executable is
/// installed alongside the driver, so absence simply means this stays empty.
fn probe_vram(server_pid: Option<u32>) -> VramStatus {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.total,memory.used,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let Ok(output) = output else {
        return VramStatus::default();
    };
    if !output.status.success() {
        return VramStatus::default();
    }

    let mut total_mib = 0;
    let mut used_mib = 0;
    let mut free_mib = 0;
    let mut gpu_found = false;
    for row in String::from_utf8_lossy(&output.stdout).lines() {
        let values: Vec<_> = row.split(',').map(|value| value.trim().parse::<u64>()).collect();
        if let [Ok(total), Ok(used), Ok(free)] = values.as_slice() {
            total_mib += total;
            used_mib += used;
            free_mib += free;
            gpu_found = true;
        }
    }
    if !gpu_found {
        return VramStatus::default();
    }

    let mut processes: Vec<VramProcess> = Vec::new();
    if let Ok(output) = Command::new("nvidia-smi")
        .args([
            "--query-compute-apps=pid,process_name,used_memory",
            "--format=csv,noheader,nounits",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        if output.status.success() {
            for row in String::from_utf8_lossy(&output.stdout).lines() {
                let mut values = row.split(',').map(str::trim);
                let (Some(pid), Some(name), Some(used_mib)) = (values.next(), values.next(), values.next()) else {
                    continue;
                };
                let (Ok(pid), Ok(used_mib)) = (pid.parse::<u32>(), used_mib.parse::<u64>()) else {
                    continue;
                };
                // A process can have an entry per GPU. Present one clear total.
                if let Some(existing) = processes.iter_mut().find(|process| process.pid == pid) {
                    existing.used_mib += used_mib;
                } else {
                    processes.push(VramProcess {
                        pid,
                        name: name.to_string(),
                        used_mib,
                    });
                }
            }
        }
    }
    processes = reconcile_vram_processes(processes, total_mib, server_pid);
    // Under Windows' WDDM driver mode, nvidia-smi often reports total VRAM but
    // omits every process. Windows' own GPU counters expose those allocations.
    let mut windows_processes = probe_windows_vram_processes();
    if !windows_processes.is_empty() {
        // DWM mirrors application surfaces for desktop composition. Its
        // Dedicated Usage is already attributed to the owning applications,
        // so including it makes the process rows exceed physical VRAM usage.
        windows_processes.retain(|process| !process.name.eq_ignore_ascii_case("dwm"));
        processes = reconcile_vram_processes(windows_processes, total_mib, server_pid);
        let process_used_mib = processes.iter().map(|process| process.used_mib).sum::<u64>();
        // Keep the headline and the breakdown on the same Windows accounting
        // basis when the cleaned allocation total is physically possible.
        if process_used_mib > 0 && process_used_mib <= total_mib {
            used_mib = process_used_mib;
            free_mib = total_mib - process_used_mib;
        }
    }
    let model_mib = server_pid
        .and_then(|pid| {
            processes
                .iter()
                .find(|process| process.pid == pid)
                .map(|process| process.used_mib)
        })
        // The tracked PID is usually the launch shell. Find its llama-server
        // child by name when Windows attributes the allocation to that child.
        .or_else(|| {
            processes
                .iter()
                .find(|process| is_llama_server_process(&process.name))
                .map(|process| process.used_mib)
        });

    VramStatus {
        total_mib: Some(total_mib),
        used_mib: Some(used_mib),
        free_mib: Some(free_mib),
        model_mib,
        processes,
    }
}

fn is_llama_server_process(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("llama-server")
}

/// Windows GPU Process Memory counters can retain an allocation after a PID is
/// reused and can report overlapping WDDM allocations for multiple processes.
/// Prefer the active server and only expose a physically possible combination.
fn reconcile_vram_processes(
    mut processes: Vec<VramProcess>,
    total_mib: u64,
    server_pid: Option<u32>,
) -> Vec<VramProcess> {
    processes.retain(|process| process.used_mib > 0 && process.used_mib <= total_mib);
    processes.sort_by(|a, b| {
        let a_is_server = Some(a.pid) == server_pid || is_llama_server_process(&a.name);
        let b_is_server = Some(b.pid) == server_pid || is_llama_server_process(&b.name);
        b_is_server
            .cmp(&a_is_server)
            .then_with(|| b.used_mib.cmp(&a.used_mib))
    });

    let mut accounted_mib = 0_u64;
    processes.retain(|process| {
        if accounted_mib.saturating_add(process.used_mib) > total_mib {
            return false;
        }
        accounted_mib += process.used_mib;
        true
    });
    processes.sort_by(|a, b| b.used_mib.cmp(&a.used_mib));
    processes
}

#[derive(Default)]
struct VramProcessCache {
    sampled_at: Option<Instant>,
    processes: Vec<VramProcess>,
    refreshing: bool,
}

/// Return the most recent Windows per-process VRAM sample immediately. A slow
/// Get-Counter refresh runs in the background so it never stalls status, tray,
/// or UI refresh calls.
fn probe_windows_vram_processes() -> Vec<VramProcess> {
    static CACHE: OnceLock<Arc<Mutex<VramProcessCache>>> = OnceLock::new();
    let cache = Arc::clone(CACHE.get_or_init(|| Arc::new(Mutex::new(VramProcessCache::default()))));
    let mut cached = cache.lock().unwrap();
    let fresh = cached
        .sampled_at
        .is_some_and(|sampled_at| sampled_at.elapsed() < Duration::from_secs(8));
    if fresh || cached.refreshing {
        return cached.processes.clone();
    }
    cached.refreshing = true;
    let current = cached.processes.clone();
    drop(cached);

    thread::spawn(move || {
        let processes = query_windows_vram_processes();
        let mut cached = cache.lock().unwrap();
        cached.processes = processes;
        cached.sampled_at = Some(Instant::now());
        cached.refreshing = false;
    });
    current
}

fn query_windows_vram_processes() -> Vec<VramProcess> {
    let script = "$usage=@{}; (Get-Counter '\\GPU Process Memory(*)\\Dedicated Usage' -ErrorAction SilentlyContinue).CounterSamples | ForEach-Object { if ($_.InstanceName -match '^pid_(\\d+)_') { $processId=[uint32]$Matches[1]; $usage[$processId]=[double]($usage[$processId])+[double]$_.CookedValue } }; $usage.GetEnumerator() | ForEach-Object { $process=Get-Process -Id $_.Key -ErrorAction SilentlyContinue; if ($process -and $_.Value -ge 1048576) { '{0}|{1}|{2}' -f $_.Key,$process.ProcessName,[long]$_.Value } }";
    let mut processes = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|row| {
                    let mut values = row.trim().splitn(3, '|');
                    let pid = values.next()?.parse::<u32>().ok()?;
                    let name = values.next()?.trim();
                    let bytes = values.next()?.parse::<u64>().ok()?;
                    Some(VramProcess {
                        pid,
                        name: name.to_string(),
                        used_mib: (bytes + 524_288) / 1_048_576,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // The Windows counter provider can retain a large allocation after its
    // owner exits. If Windows later reuses that PID, Get-Process gives the
    // stale allocation the new process's name (the observed 20 GB Firefox
    // bug). NVIDIA pmon is a live list, so require every displayed PID to be
    // active there instead of trusting the stale counter instance name.
    if let Some(active_pids) = query_active_gpu_pids() {
        processes.retain(|process| active_pids.contains(&process.pid));
    }
    processes.sort_by(|a, b| b.used_mib.cmp(&a.used_mib));
    processes
}

fn query_active_gpu_pids() -> Option<HashSet<u32>> {
    let output = Command::new("nvidia-smi")
        .args(["pmon", "-c", "1", "-s", "m"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_active_gpu_pids(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_active_gpu_pids(output: &str) -> HashSet<u32> {
    output
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_whitespace().nth(1)?.parse::<u32>().ok())
        .collect()
}

/// Average generation tokens/sec for the current model since it started.
/// Prefers llama.cpp `/metrics` counters; falls back to parsing the run log
/// (for forks like beellama that do not expose those counters).
fn probe_avg_tps(state: &Arc<AppState>, status: &Status) -> Option<f64> {
    // Re-baseline everything when the model changes.
    {
        let mut t = state.tps.lock().unwrap();
        if t.profile_id != status.current_profile_id {
            *t = crate::state::TpsTracker {
                profile_id: status.current_profile_id.clone(),
                ..Default::default()
            };
        }
    }

    if let Some(v) = probe_metrics_tps(state, status) {
        return Some(v);
    }
    probe_log_tps(state)
}

/// Source 1: llama.cpp `/metrics` cumulative counters, reusing the `/slots`
/// API key. Returns None on 401/missing so the log fallback can run.
fn probe_metrics_tps(state: &Arc<AppState>, status: &Status) -> Option<f64> {
    let api_key = usage_probe_api_key(state, status)?;
    let url = format!("{}/metrics", server_origin(&status.health_url, status.server_port));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(800))
        .build();
    let body = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {}", api_key))
        .call()
        .ok()?
        .into_string()
        .ok()?;

    let tokens = parse_prometheus_metric(&body, "llamacpp:tokens_predicted_total")?;
    let seconds = parse_prometheus_metric(&body, "llamacpp:tokens_predicted_seconds_total")?;

    let mut t = state.tps.lock().unwrap();
    let (base_tokens, base_seconds) = *t.metrics_baseline.get_or_insert((tokens, seconds));
    let d_tokens = tokens - base_tokens;
    let d_seconds = seconds - base_seconds;
    if d_seconds > 0.05 && d_tokens > 0.0 {
        Some(d_tokens / d_seconds)
    } else {
        None
    }
}

/// Source 2: parse the managed run log incrementally for generation
/// `eval time = X ms / Y tokens` lines and average tokens/sec since model start.
fn probe_log_tps(state: &Arc<AppState>) -> Option<f64> {
    use std::io::{Read, Seek, SeekFrom};

    let log_path = state
        .running
        .lock()
        .unwrap()
        .as_ref()
        .map(|rp| rp.log_path.clone())?;
    let mut file = std::fs::File::open(&log_path).ok()?;
    let len = file.metadata().ok()?.len();

    let mut t = state.tps.lock().unwrap();
    // Log rotated/truncated — restart the accumulation.
    if t.log_offset > len {
        t.log_offset = 0;
        t.log_gen_tokens = 0.0;
        t.log_gen_ms = 0.0;
    }
    file.seek(SeekFrom::Start(t.log_offset)).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    // Only consume up to the last complete line so we never split a line.
    let consumed = buf.rfind('\n').map(|i| i + 1).unwrap_or(0);
    for line in buf[..consumed].lines() {
        if let Some((tokens, ms)) = parse_gen_eval_line(line) {
            t.log_gen_tokens += tokens;
            t.log_gen_ms += ms;
        }
    }
    t.log_offset += consumed as u64;

    if t.log_gen_ms > 0.0 && t.log_gen_tokens > 0.0 {
        Some(t.log_gen_tokens / (t.log_gen_ms / 1000.0))
    } else {
        None
    }
}

/// Parse a llama.cpp generation timing line, returning (tokens, milliseconds).
/// Matches `... eval time = <ms> ms / <n> tokens (...)` but NOT `prompt eval
/// time` (prompt processing) or `total time`.
fn parse_gen_eval_line(line: &str) -> Option<(f64, f64)> {
    if line.contains("prompt eval time") {
        return None;
    }
    let idx = line.find("eval time =")?;
    let rest = &line[idx + "eval time =".len()..];
    let ms = parse_leading_f64(rest)?;
    let slash = rest.find('/')?;
    let tokens = parse_leading_f64(&rest[slash + 1..])?;
    Some((tokens, ms))
}

/// Parse the first numeric token (skipping leading whitespace) as f64.
fn parse_leading_f64(s: &str) -> Option<f64> {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    s[..end].parse::<f64>().ok()
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

/// Live watchdog: preserve the managed server tree (or the recognized external
/// listener) and terminate any other configured server binary. `try_lock`
/// keeps the watchdog out of the way during an active start/stop/switch.
pub fn sweep_stale_servers(app: &AppHandle, state: &Arc<AppState>, status: &Status) {
    let Ok(_operation) = state.op_lock.try_lock() else {
        return;
    };
    let settings = state.settings_snapshot();
    if settings.server_process_names.is_empty() {
        return;
    }

    let managed_root = state
        .running
        .lock()
        .unwrap()
        .as_ref()
        .map(|running| running.pid);
    let allowed = managed_root
        .or_else(|| status.server_reachable.then_some(status.pid).flatten())
        .map(process_tree::descendants)
        .unwrap_or_default();
    let count = process_tree::kill_all_by_image_except(&settings.server_process_names, &allowed);
    if count > 0 {
        let _ = app.emit(
            "warning",
            format!(
                "Watchdog terminated {} rogue or stale server process{}.",
                count,
                if count == 1 { "" } else { "es" }
            ),
        );
    }
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
    if state.shutting_down.load(Ordering::Relaxed) {
        return Err("Application is shutting down.".into());
    }
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
        *tracker = crate::state::TpsTracker {
            profile_id: Some(profile.id.clone()),
            ..Default::default()
        };
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
    let (profile_id, old_shell_pid, old_log_path) = {
        let running = state.running.lock().unwrap();
        let current = running
            .as_ref()
            .ok_or_else(|| "No server is currently running to restart.".to_string())?;
        (
            current.profile.id.clone(),
            current.pid,
            current.log_path.clone(),
        )
    };
    logging::append_line(&old_log_path, "Restart requested.");
    let old_listener_pid = pid_on_port(state.settings_snapshot().server_port);

    let launched = activate_profile(app, state, &profile_id)?;
    let new_shell_pid = launched
        .pid
        .ok_or_else(|| "Restart did not create a new server process.".to_string())?;
    if new_shell_pid == old_shell_pid {
        return Err(format!(
            "Restart reused the old shell PID {} instead of launching a replacement.",
            old_shell_pid
        ));
    }

    wait_for_restart_ready(
        app,
        state,
        &profile_id,
        new_shell_pid,
        old_listener_pid,
    )
}

fn wait_for_restart_ready(
    app: &AppHandle,
    state: &Arc<AppState>,
    profile_id: &str,
    new_shell_pid: u32,
    old_listener_pid: Option<u32>,
) -> Result<Status, String> {
    let settings = state.settings_snapshot();
    let deadline = Instant::now()
        + Duration::from_secs(settings.health_check_timeout_seconds.max(1));

    loop {
        {
            let running = state.running.lock().unwrap();
            let still_current = running.as_ref().is_some_and(|process| {
                process.profile.id == profile_id && process.pid == new_shell_pid
            });
            if !still_current {
                return Err("The replacement server exited or was superseded during restart.".into());
            }
        }

        let listener_pid = pid_on_port(settings.server_port);
        let listener_replaced = restart_has_replacement_listener(old_listener_pid, listener_pid);
        let health = listener_replaced.then(|| probe_health(&settings.health_url));
        if health.as_ref().is_some_and(|probe| probe.healthy) {
            {
                let mut running = state.running.lock().unwrap();
                if let Some(process) = running.as_mut() {
                    if process.profile.id == profile_id && process.pid == new_shell_pid {
                        process.healthy = true;
                        logging::append_line(
                            &process.log_path,
                            &format!(
                                "Restart verified: new shell PID {}, listener PID {}, health endpoint ready.",
                                new_shell_pid,
                                listener_pid.unwrap_or_default()
                            ),
                        );
                    }
                }
            }
            notify(app, state);
            let mut status = state.status();
            status.server_reachable = true;
            status.healthy = true;
            return Ok(status);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "Replacement server launched as PID {} but did not become healthy within {} seconds.",
                new_shell_pid, settings.health_check_timeout_seconds
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn restart_has_replacement_listener(old_listener: Option<u32>, current_listener: Option<u32>) -> bool {
    current_listener.is_some()
        && (old_listener.is_none() || current_listener != old_listener)
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

    fn vram_process(pid: u32, name: &str, used_mib: u64) -> VramProcess {
        VramProcess {
            pid,
            name: name.into(),
            used_mib,
        }
    }

    #[test]
    fn filters_impossible_overlapping_windows_vram_allocations() {
        let processes = vec![
            vram_process(40_120, "llama-server", 23_000),
            vram_process(22_184, "firefox", 20_000),
            vram_process(12_832, "explorer", 80),
        ];

        let reconciled = reconcile_vram_processes(processes, 24_576, Some(11_736));

        assert!(reconciled.iter().any(|process| process.name == "llama-server"));
        assert!(reconciled.iter().any(|process| process.name == "explorer"));
        assert!(!reconciled.iter().any(|process| process.name == "firefox"));
        assert!(reconciled.iter().map(|process| process.used_mib).sum::<u64>() <= 24_576);
    }

    #[test]
    fn retains_multiple_real_allocations_that_fit() {
        let processes = vec![
            vram_process(101, "llama-server", 12_000),
            vram_process(202, "renderer", 10_000),
        ];

        let reconciled = reconcile_vram_processes(processes, 24_576, Some(101));
        assert_eq!(reconciled.len(), 2);
    }

    #[test]
    fn parses_only_live_pids_from_nvidia_pmon() {
        let output = "# gpu pid type fb ccpm command\n\
                         0  42928 C 20480 0 llama-server.exe\n\
                         0  12832 G 112 0 explorer.exe\n";
        let active = parse_active_gpu_pids(output);
        assert!(active.contains(&42_928));
        assert!(active.contains(&12_832));
        assert!(!active.contains(&22_184));
    }

    #[test]
    fn restart_requires_a_new_listener_process() {
        assert!(!restart_has_replacement_listener(Some(9_148), None));
        assert!(!restart_has_replacement_listener(Some(9_148), Some(9_148)));
        assert!(restart_has_replacement_listener(Some(9_148), Some(35_456)));
        assert!(restart_has_replacement_listener(None, Some(35_456)));
    }

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

    #[test]
    fn parses_generation_eval_lines_from_log() {
        // Generation line -> (tokens, ms).
        let gen = "1.53.184.575 I slot print_timing: id  0 | task 205 |        eval time =    1919.06 ms /    74 tokens (   25.93 ms per token,    38.56 tokens per second)";
        assert_eq!(parse_gen_eval_line(gen), Some((74.0, 1919.06)));

        // Prompt-processing lines must be ignored.
        let prompt = "1.37.109.438 I slot print_timing: id  0 | task 144 | prompt eval time =    7083.14 ms /  8535 tokens (    0.83 ms per token,  1204.97 tokens per second)";
        assert!(parse_gen_eval_line(prompt).is_none());

        // total time and streaming (tg) lines are not generation eval lines.
        assert!(parse_gen_eval_line("... |       total time =    8582.90 ms /  8597 tokens").is_none());
        assert!(parse_gen_eval_line("... | n_decoded =    100, tg = 111.10 t/s").is_none());

        // Cumulative average over two generations.
        let (t1, m1) = parse_gen_eval_line(gen).unwrap();
        let (t2, m2) = parse_gen_eval_line("eval time = 1499.76 ms / 62 tokens ( 24.19 ms per token, 41.34 tokens per second)").unwrap();
        let avg = (t1 + t2) / ((m1 + m2) / 1000.0);
        assert!((avg - 39.78).abs() < 0.1);
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
