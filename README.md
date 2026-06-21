# Llama Switcher

A very lightweight **Windows system-tray** desktop app for switching between
**llama.cpp** server startup scripts. Built with **Tauri v2 + Rust** (all backend
logic) and a small **React + TypeScript + Vite** dashboard.

- Tray-first: starts minimized to the notification area, dashboard opens on demand.
- Rust owns everything: process management, script scanning, the local control
  API, logging, settings, tray menu, and Windows process-tree killing.
- One llama.cpp server at a time. Switching stops the current one (whole process
  tree), waits for it to exit, frees the port, then launches the new script.
- A local HTTP API (127.0.0.1 only, bearer-token auth) lets **Hermes Agent**
  control the app. Hermes never runs scripts directly.

> **Hermes is not a llama.cpp model/profile.** It is a separate agent that
> controls Llama Switcher through the local API or the `hermes-skill/` adapter.

## How profiles are detected

Point the app at a scripts folder (default `D:\llama`). It scans for files named:

```
start - {model} - {feature}.cmd
start - {model} - {feature}.bat
start - {model} - {feature}.ps1
```

and builds a profile for each. Examples:

| File | Alias |
| --- | --- |
| `start - qwen-27B - MTP.cmd` | **Qwen-27B MTP** |
| `start - qwen-27B - Vision.cmd` | **Qwen-27B Vision** |
| `start - qwen-4B - Vision.cmd` | **Qwen-4B Vision** |
| `start - llama-70B - Standard.cmd` | **Llama-70B Standard** |
| `start - mistral-7B - CPU.cmd` | **Mistral-7B CPU** |

Files that don't match are reported as *ignored* (with a reason) in the dashboard.

## Prerequisites

- Windows 10/11
- [Rust](https://rustup.rs/) (stable) + the MSVC build tools
- [Node.js](https://nodejs.org/) 18+
- WebView2 runtime (preinstalled on Windows 11)

## Install & run (development)

```bash
npm install
npm run tauri dev
```

The first build also needs app icons. If `src-tauri/icons/` is empty, generate
them once (requires a source PNG, or use the included generator):

```bash
# from the project root, with a 512x512 source image:
npm run tauri icon path\to\icon.png
```

A simple placeholder generator (`scripts/generate-icons.ps1`) is included for
getting started — run it with PowerShell to produce basic icons.

## Build a release installer

```bash
npm run tauri build
```

Produces an NSIS installer under `src-tauri/target/release/bundle/`.

## Project layout

```
src/                     React + TS dashboard (Status, Detected Scripts,
                         Settings, Logs, Agent Control)
src-tauri/src/
  settings.rs            Load/save settings JSON in app data dir
  script_scanner.rs      Folder scan -> profiles + ignored files
  alias_formatter.rs     Pretty model/feature names, aliases, matching
  process_manager.rs     Start/stop/switch/restart, port checks, health polling
  process_tree.rs        Windows-safe process-tree termination
  local_api.rs           127.0.0.1 control API (bearer token)
  tray.rs                Dynamic tray menu
  logging.rs             Per-run log files
  state.rs               Shared app state
  lib.rs                 Tauri commands + setup
hermes-skill/            TypeScript adapter for Hermes Agent
HERMES_AGENT_TOOLING.md  Full local API + agent tooling reference
```

## Settings

Stored as JSON in the app data directory (`%APPDATA%\com.llamaswitcher.app\settings.json`):

```json
{
  "scriptsFolder": "D:\\llama",
  "scanPattern": "start - {model} - {feature}",
  "allowedExtensions": [".cmd", ".bat", ".ps1"],
  "serverPort": 8080,
  "healthUrl": "http://127.0.0.1:8080/health",
  "agentApiPort": 47891,
  "agentApiToken": "generated-random-token",
  "autoRescanOnStartup": true,
  "autoRescanIntervalSeconds": null,
  "defaultProfileMode": "none",
  "defaultProfileId": null,
  "lastUsedProfileId": null,
  "stopTimeoutSeconds": 15,
  "healthCheckTimeoutSeconds": 60
}
```

## Runtime behavior

- The control API binds to `127.0.0.1` only and requires `Authorization: Bearer
  <token>` for every endpoint except `GET /health`.
- Start / Switch always takes ownership of the configured server port by
  stopping its current listener first, including externally launched servers.
- When an external listener's process ancestry names a detected startup script,
  Llama Switcher automatically relaunches that same profile under management so
  model/feature metadata, Restart, and captured run logs are immediately available.
- No background polling loops unless you enable an auto-rescan interval. Health
  checks only run during the startup window of a launch.

## Hermes Agent

See [HERMES_AGENT_TOOLING.md](HERMES_AGENT_TOOLING.md) and
[`hermes-skill/README.md`](hermes-skill/README.md).
