//! Cloudflare-aware HTTP security gateway for llama-server.

use crate::process_manager;
use crate::settings::{NetworkAccessMode, Settings};
use crate::state::AppState;
use chrono::{DateTime, Duration, Utc};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tauri::AppHandle;
use tiny_http::{Header, Request, Response, Server, StatusCode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanEntry {
    pub ip: String,
    pub reason: String,
    pub failure_count: u32,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub automatic: bool,
}

#[derive(Default)]
pub struct SecurityRuntime {
    pub bans: Vec<BanEntry>,
    failures: HashMap<String, Vec<DateTime<Utc>>>,
}

impl SecurityRuntime {
    pub fn load(path: &Path) -> Self {
        let bans = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            bans,
            failures: HashMap::new(),
        }
    }
}

pub fn validate_networks(values: &[String]) -> Result<(), String> {
    for value in values {
        if value.parse::<IpAddr>().is_err() && value.parse::<IpNet>().is_err() {
            return Err(format!("Invalid trusted IP or CIDR: {value}"));
        }
    }
    Ok(())
}

fn save_bans(state: &AppState) {
    let runtime = state.security.lock().unwrap();
    if let Ok(text) = serde_json::to_string_pretty(&runtime.bans) {
        let _ = std::fs::write(&state.security_path, text);
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|v| v.with_timezone(&Utc))
}

fn purge_expired(runtime: &mut SecurityRuntime) {
    let now = Utc::now();
    runtime.bans.retain(|ban| {
        ban.expires_at
            .as_deref()
            .and_then(parse_time)
            .is_none_or(|end| end > now)
    });
}

pub fn list_bans(state: &AppState) -> Vec<BanEntry> {
    let mut runtime = state.security.lock().unwrap();
    purge_expired(&mut runtime);
    runtime.bans.clone()
}

pub fn unban(state: &AppState, ip: &str) -> Result<(), String> {
    ip.parse::<IpAddr>()
        .map_err(|_| "Invalid IP address".to_string())?;
    let mut runtime = state.security.lock().unwrap();
    runtime.bans.retain(|entry| entry.ip != ip);
    runtime.failures.remove(ip);
    drop(runtime);
    save_bans(state);
    Ok(())
}

pub fn manual_ban(state: &AppState, ip: &str) -> Result<(), String> {
    let parsed = ip
        .parse::<IpAddr>()
        .map_err(|_| "Invalid IP address".to_string())?;
    if parsed.is_loopback() {
        return Err("Localhost cannot be banned".into());
    }
    let mut runtime = state.security.lock().unwrap();
    purge_expired(&mut runtime);
    runtime.bans.retain(|entry| entry.ip != ip);
    runtime.bans.push(BanEntry {
        ip: ip.into(),
        reason: "Manual ban".into(),
        failure_count: 0,
        created_at: Utc::now().to_rfc3339(),
        expires_at: None,
        automatic: false,
    });
    drop(runtime);
    save_bans(state);
    Ok(())
}

fn is_trusted(ip: IpAddr, settings: &Settings) -> bool {
    ip.is_loopback()
        || settings.trusted_networks.iter().any(|value| {
            value.parse::<IpAddr>().is_ok_and(|trusted| trusted == ip)
                || value.parse::<IpNet>().is_ok_and(|net| net.contains(&ip))
        })
}

fn is_banned(state: &AppState, ip: IpAddr) -> bool {
    let mut runtime = state.security.lock().unwrap();
    purge_expired(&mut runtime);
    runtime.bans.iter().any(|ban| ban.ip == ip.to_string())
}

fn record_failure(state: &AppState, ip: IpAddr, settings: &Settings) {
    if !settings.auto_ban_enabled || is_trusted(ip, settings) {
        return;
    }
    let now = Utc::now();
    let cutoff = now - Duration::seconds(settings.auto_ban_window_seconds.max(1) as i64);
    let mut runtime = state.security.lock().unwrap();
    let attempts = runtime.failures.entry(ip.to_string()).or_default();
    attempts.retain(|time| *time >= cutoff);
    attempts.push(now);
    if attempts.len() < settings.auto_ban_failure_threshold.max(1) as usize {
        return;
    }
    let count = attempts.len() as u32;
    runtime.bans.retain(|ban| ban.ip != ip.to_string());
    runtime.bans.push(BanEntry {
        ip: ip.to_string(),
        reason: "Repeated invalid API keys".into(),
        failure_count: count,
        created_at: now.to_rfc3339(),
        expires_at: Some(
            (now + Duration::seconds(settings.auto_ban_duration_seconds.max(1) as i64))
                .to_rfc3339(),
        ),
        automatic: true,
    });
    runtime.failures.remove(&ip.to_string());
    drop(runtime);
    save_bans(state);
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().trim().to_string())
}

fn client_ip(request: &Request, mode: &NetworkAccessMode) -> Result<IpAddr, String> {
    let peer = request
        .remote_addr()
        .map(SocketAddr::ip)
        .ok_or("Client address unavailable")?;
    match mode {
        NetworkAccessMode::CloudflareTunnel | NetworkAccessMode::WhitelistOnly
            if peer.is_loopback() =>
        {
            match header(request, "CF-Connecting-IP") {
                Some(value) => value.parse().map_err(|_| "Invalid CF-Connecting-IP".into()),
                None => Ok(peer),
            }
        }
        _ => Ok(peer),
    }
}

fn json_response(code: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({"error": {"message": message}}).to_string();
    Response::from_string(body)
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
}

fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/health"
            | "/v1/health"
            | "/models"
            | "/v1/models"
            | "/"
            | "/index.html"
            | "/bundle.js"
            | "/bundle.css"
    )
}

fn handle(state: &Arc<AppState>, mut request: Request) {
    let settings = state.settings_snapshot();
    if !settings.security_gateway_enabled {
        let _ = request.respond(json_response(503, "Security gateway is disabled"));
        return;
    }
    let path = request.url().split('?').next().unwrap_or("");
    if matches!(settings.network_access_mode, NetworkAccessMode::LocalOnly)
        && header(&request, "CF-Connecting-IP").is_some()
    {
        let _ = request.respond(json_response(403, "Local-only mode"));
        return;
    }
    let ip = match client_ip(&request, &settings.network_access_mode) {
        Ok(ip) => ip,
        Err(error) => {
            let _ = request.respond(json_response(400, &error));
            return;
        }
    };
    if matches!(settings.network_access_mode, NetworkAccessMode::LocalOnly) && !ip.is_loopback() {
        let _ = request.respond(json_response(403, "Local-only mode"));
        return;
    }
    if matches!(
        settings.network_access_mode,
        NetworkAccessMode::WhitelistOnly
    ) && !is_trusted(ip, &settings)
    {
        let _ = request.respond(json_response(403, "Address is not whitelisted"));
        return;
    }
    if is_banned(state, ip) {
        let _ = request.respond(json_response(403, "Address is temporarily banned"));
        return;
    }

    if !is_public_path(path) {
        let expected = state
            .running
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|rp| process_manager::resolve_api_key(state, Some(&rp.profile.script_path)))
            .or_else(|| settings.llama_server_api_key.clone());
        if let Some(key) = expected {
            if header(&request, "Authorization").as_deref() != Some(&format!("Bearer {key}")) {
                record_failure(state, ip, &settings);
                let _ = request.respond(json_response(401, "Invalid API Key"));
                return;
            }
        }
    }

    let mut body = Vec::new();
    if request
        .as_reader()
        .take(128 * 1024 * 1024)
        .read_to_end(&mut body)
        .is_err()
    {
        let _ = request.respond(json_response(400, "Could not read request body"));
        return;
    }
    let url = format!("http://127.0.0.1:{}{}", settings.server_port, request.url());
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(3600))
        .build();
    let mut upstream = agent.request(request.method().as_str(), &url);
    for h in request.headers() {
        if !h.field.equiv("Host")
            && !h.field.equiv("Content-Length")
            && !h.field.equiv("Connection")
            && !h.field.equiv("CF-Connecting-IP")
        {
            upstream = upstream.set(h.field.as_str().as_str(), h.value.as_str());
        }
    }
    let result = if body.is_empty() {
        upstream.call()
    } else {
        upstream.send_bytes(&body)
    };
    let upstream_response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => {
            let _ = request.respond(json_response(
                502,
                &format!("Upstream unavailable: {error}"),
            ));
            return;
        }
    };
    let status = upstream_response.status();
    let mut headers = Vec::new();
    for name in upstream_response.headers_names() {
        if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let Some(value) = upstream_response.header(&name) {
            if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
                headers.push(h);
            }
        }
    }
    let response = Response::new(
        StatusCode(status),
        headers,
        upstream_response.into_reader(),
        None,
        None,
    );
    let _ = request.respond(response);
}

pub fn start(app: AppHandle, state: Arc<AppState>) {
    let settings = state.settings_snapshot();
    if !settings.security_gateway_enabled {
        return;
    }
    std::thread::spawn(move || {
        let host = if matches!(
            settings.network_access_mode,
            NetworkAccessMode::DirectInternet
        ) {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        let addr = format!("{host}:{}", settings.security_gateway_port);
        let server = match Server::http(&addr) {
            Ok(server) => server,
            Err(error) => {
                let _ = tauri::Emitter::emit(
                    &app,
                    "warning",
                    format!("Security gateway could not bind {addr}: {error}"),
                );
                return;
            }
        };
        for request in server.incoming_requests() {
            handle(&state, request);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_scanner::ScanResult;
    use std::path::PathBuf;
    #[test]
    fn validates_ip_networks() {
        assert!(validate_networks(&["192.168.1.0/24".into(), "2001:db8::1".into()]).is_ok());
        assert!(validate_networks(&["not-an-ip".into()]).is_err());
    }

    #[test]
    fn cloudflare_failures_create_a_ban_for_the_visitor_not_localhost() {
        let mut settings = Settings::default();
        settings.security_gateway_enabled = true;
        settings.llama_server_api_key = Some("correct-key".into());
        settings.auto_ban_failure_threshold = 2;
        settings.auto_ban_window_seconds = 60;
        let root =
            std::env::temp_dir().join(format!("llama-switcher-security-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let state = Arc::new(AppState::new(
            settings,
            ScanResult::default(),
            root.join("settings.json"),
            PathBuf::from(&root),
        ));
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            for request in server.incoming_requests().take(2) {
                handle(&worker_state, request);
            }
        });
        let url = format!("http://{address}/v1/chat/completions");
        for _ in 0..2 {
            let result = ureq::get(&url)
                .set("CF-Connecting-IP", "203.0.113.77")
                .set("Authorization", "Bearer wrong-key")
                .call();
            assert!(matches!(result, Err(ureq::Error::Status(401, _))));
        }
        worker.join().unwrap();
        let bans = list_bans(&state);
        assert_eq!(bans.len(), 1);
        assert_eq!(bans[0].ip, "203.0.113.77");
        assert_ne!(bans[0].ip, "127.0.0.1");
        let _ = std::fs::remove_dir_all(root);
    }
}
