# Hermes Agent Tooling — Llama Switcher Control API

This document is the reference for controlling **Llama Switcher** from **Hermes
Agent**. Hermes controls the app through the **local HTTP API** (or the
`hermes-skill/` adapter that wraps it). Hermes must **not** run `.cmd`, `.bat`,
or `.ps1` files directly — Llama Switcher owns all process management.

## Connection

- Base URL: `http://127.0.0.1:47891` (port configurable in Settings)
- Bound to `127.0.0.1` only — never exposed to the network.
- Auth: every endpoint **except** `GET /health` requires:

  ```
  Authorization: Bearer <agentApiToken>
  ```

  Copy the token from the dashboard (Agent Control page / Settings). It is
  generated randomly on first launch and can be regenerated.

## Endpoints

### `GET /health` — public

```json
{ "ok": true, "app": "Llama Switcher" }
```

### `GET /status`

```json
{
  "running": true,
  "currentProfileId": "qwen-27b__mtp",
  "alias": "Qwen-27B MTP",
  "currentProfileName": "Qwen-27B MTP",
  "model": "Qwen-27B",
  "feature": "MTP",
  "scriptPath": "D:\\llama\\start - qwen-27B - MTP.cmd",
  "pid": 12345,
  "healthy": true,
  "serverPort": 8080,
  "healthUrl": "http://127.0.0.1:8080/health",
  "startedAt": "2026-06-20T15:30:00"
}
```

### `GET /profiles`

Returns the array of detected profiles (`id`, `alias`, `prettyModel`,
`prettyFeature`, `scriptPath`, `extension`, …).

### `POST /rescan`

Rescans the configured scripts folder. Returns the full scan result
(`profiles`, `ignoredFiles`, `scannedAt`).

### `POST /start` and `POST /switch`

Body:

```json
{ "profileId": "qwen-27b__mtp" }
```

Both activate the given profile (stopping any current one first).

### `POST /switch-by-name`

```json
{ "model": "Qwen-27B", "feature": "MTP" }
```

### `POST /switch-by-alias`

```json
{ "alias": "Qwen-27B MTP" }
```

Alias matching is case-insensitive and separator-flexible (`Qwen-27B MTP`,
`qwen-27b mtp`, `Qwen 27B MTP` all match). If multiple profiles match, the API
returns **HTTP 409** with the list of candidates instead of guessing:

```json
{ "error": "Alias 'qwen vision' is ambiguous. Matches: Qwen-27B Vision, Qwen-4B Vision" }
```

### `POST /restart`

Restarts the currently running profile. 400 if nothing is running.

### `POST /stop`

Stops the running server (whole process tree) and confirms the port is free.

### `POST /open-dashboard`

```json
{ "ok": true }
```

## Error model

- `401` — missing/invalid bearer token.
- `400` — bad/missing body field, validation error, or a failed
  start/stop/restart (message in `error`).
- `409` — ambiguous alias / model+feature (message lists matches).
- `404` — unknown path.

## curl examples (`cmd`)

```bat
curl http://127.0.0.1:47891/status ^
  -H "Authorization: Bearer YOUR_TOKEN"

curl -X POST http://127.0.0.1:47891/switch-by-alias ^
  -H "Authorization: Bearer YOUR_TOKEN" ^
  -H "Content-Type: application/json" ^
  -d "{\"alias\":\"Qwen-27B MTP\"}"

curl -X POST http://127.0.0.1:47891/stop ^
  -H "Authorization: Bearer YOUR_TOKEN"
```

## Hermes skill

Hermes discovers skills from `~/.hermes/skills/**/SKILL.md`. Install the
`hermes-skill/` folder as `~/.hermes/skills/llama-switcher/`; no separate tool
registration or npm build is required. The skill uses its bundled,
dependency-free Python client to call the local API.

The Agent Control page performs this installation and adds
`LLAMA_SWITCHER_BASE_URL` and `LLAMA_SWITCHER_API_TOKEN` to the active Hermes
profile's `.env`. Start a new Hermes session or use `/reset` afterward.

Recommended agent behavior:

1. Prefer `switch_by_alias`; fall back to `switch_by_name`.
2. Call `list_profiles` first if you need to confirm an alias exists.
3. On a 409 ambiguity error, present the candidate aliases to the user.
4. On "not reachable", tell the user to start the Llama Switcher tray app.
