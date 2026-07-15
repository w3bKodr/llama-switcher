//! Local HTTP control API for Hermes Agent. Bound to 127.0.0.1 only.
//!
//! Every endpoint except `/health` requires `Authorization: Bearer <token>`.
//! The API never runs scripts itself — it delegates to `process_manager`, which
//! owns all process lifecycle.

use crate::process_manager as pm;
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;
use tiny_http::{Header, Method, Response, Server};

/// Start the control API server in a background thread.
pub fn start(app: AppHandle, state: Arc<AppState>) {
    let port = state.settings_snapshot().agent_api_port;
    std::thread::spawn(move || {
        let addr = format!("127.0.0.1:{}", port);
        let server = match Server::http(&addr) {
            Ok(s) => s,
            Err(e) => {
                let _ = tauri::Emitter::emit(
                    &app,
                    "warning",
                    format!("Failed to bind agent API on {}: {}", addr, e),
                );
                return;
            }
        };
        for request in server.incoming_requests() {
            handle(&app, &state, request);
        }
    });
}

fn json_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()
}

fn respond(request: tiny_http::Request, code: u16, body: Value) {
    let data = body.to_string();
    let response = Response::from_string(data)
        .with_status_code(code)
        .with_header(json_header());
    let _ = request.respond(response);
}

fn authorized(request: &tiny_http::Request, token: &str) -> bool {
    let expected = format!("Bearer {}", token);
    request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Authorization") && h.value.as_str() == expected)
}

fn read_body(request: &mut tiny_http::Request) -> Value {
    let mut content = String::new();
    let _ = request.as_reader().read_to_string(&mut content);
    serde_json::from_str(&content).unwrap_or(Value::Null)
}

fn status_to_json(s: &crate::state::Status) -> Value {
    serde_json::to_value(s).unwrap_or(Value::Null)
}

fn handle(app: &AppHandle, state: &Arc<AppState>, mut request: tiny_http::Request) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();

    // /health is public.
    if method == Method::Get && path == "/health" {
        respond(request, 200, json!({ "ok": true, "app": "Llama Switcher" }));
        return;
    }

    // Everything else needs the bearer token.
    let token = state.settings_snapshot().agent_api_token;
    if !authorized(&request, &token) {
        respond(request, 401, json!({ "error": "Unauthorized" }));
        return;
    }

    // Block server-control actions while a benchmark owns the server.
    let mutating = matches!(
        path.as_str(),
        "/start" | "/switch" | "/switch-by-name" | "/switch-by-alias" | "/restart" | "/stop"
    );
    if mutating && crate::benchmark::is_running(state) {
        respond(
            request,
            409,
            json!({ "error": "A benchmark is running; server controls are disabled until it finishes." }),
        );
        return;
    }

    let result: Result<Value, (u16, String)> = match (method.clone(), path.as_str()) {
        (Method::Get, "/status") => Ok(status_to_json(&pm::status_with_probe(app, state))),

        (Method::Get, "/profiles") => {
            Ok(serde_json::to_value(state.profiles()).unwrap_or(Value::Null))
        }

        (Method::Post, "/rescan") => {
            let scan = crate::rescan_and_store(app, state);
            Ok(serde_json::to_value(scan).unwrap_or(Value::Null))
        }

        (Method::Post, "/start") | (Method::Post, "/switch") => {
            let body = read_body(&mut request);
            match body.get("profileId").and_then(|v| v.as_str()) {
                Some(id) => pm::activate_profile(app, state, id)
                    .map(|s| status_to_json(&s))
                    .map_err(|e| (400, e)),
                None => Err((400, "Missing 'profileId'".into())),
            }
        }

        (Method::Post, "/switch-by-name") => {
            let body = read_body(&mut request);
            let model = body.get("model").and_then(|v| v.as_str());
            let feature = body.get("feature").and_then(|v| v.as_str());
            match (model, feature) {
                (Some(m), Some(f)) => match pm::resolve_name(state, m, f) {
                    Ok(id) => pm::activate_profile(app, state, &id)
                        .map(|s| status_to_json(&s))
                        .map_err(|e| (400, e)),
                    Err(e) => Err((409, e)),
                },
                _ => Err((400, "Missing 'model' or 'feature'".into())),
            }
        }

        (Method::Post, "/switch-by-alias") => {
            let body = read_body(&mut request);
            match body.get("alias").and_then(|v| v.as_str()) {
                Some(alias) => match pm::resolve_alias(state, alias) {
                    Ok(id) => pm::activate_profile(app, state, &id)
                        .map(|s| status_to_json(&s))
                        .map_err(|e| (400, e)),
                    Err(e) => Err((409, e)),
                },
                None => Err((400, "Missing 'alias'".into())),
            }
        }

        (Method::Post, "/restart") => pm::restart_server(app, state)
            .map(|s| status_to_json(&s))
            .map_err(|e| (400, e)),

        (Method::Post, "/stop") => pm::stop_server(app, state)
            .map(|s| status_to_json(&s))
            .map_err(|e| (400, e)),

        (Method::Post, "/open-dashboard") => {
            crate::show_dashboard(app, None);
            Ok(json!({ "ok": true }))
        }

        _ => Err((404, "Not found".into())),
    };

    match result {
        Ok(value) => respond(request, 200, value),
        Err((code, msg)) => respond(request, code, json!({ "error": msg })),
    }
}
