# Llama Switcher — Hermes Agent Skill

A native `SKILL.md` package that lets **Hermes Agent** control the **Llama
Switcher** tray app through its local HTTP control API.

> Hermes Agent is **not** a llama.cpp model or profile. It is a separate agent
> that controls Llama Switcher. This skill **never** runs `.cmd` / `.bat` /
> `.ps1` files and **never** kills processes — all process management happens
> inside the Tauri/Rust app. The skill only makes authenticated HTTP calls.

## Setup

1. **Start the Llama Switcher tray app.** It runs in the Windows system tray.
2. Open the dashboard (double-click the tray icon) and go to **Settings** (or the
   **Agent Control** page).
3. **Copy the Agent control API token.**
4. Add the API settings to the active Hermes profile's `.env` (normally
   `~/.hermes/.env`):

   ```env
   LLAMA_SWITCHER_BASE_URL=http://127.0.0.1:47891
   LLAMA_SWITCHER_API_TOKEN=your-token-here
   ```

5. Copy the skill into Hermes:

   ```bash
   mkdir -p ~/.hermes/skills
   cp -R hermes-skill ~/.hermes/skills/llama-switcher
   ```

6. Start a new Hermes session or run `/reset`. Hermes automatically discovers
   `~/.hermes/skills/**/SKILL.md`; no registration or npm build is needed.

7. Ask Hermes things like:
   - "List my llama profiles."
   - "Switch to Qwen-27B MTP."
   - "Restart llama.cpp."
   - "Stop llama.cpp."

## Tools

| Tool | Endpoint | Purpose |
| --- | --- | --- |
| `llama_switcher_status` | `GET /status` | What is running now. |
| `llama_switcher_list_profiles` | `GET /profiles` | All detected profiles. |
| `llama_switcher_rescan` | `POST /rescan` | Rescan the scripts folder. |
| `llama_switcher_switch_by_alias` | `POST /switch-by-alias` | Switch by alias (preferred). |
| `llama_switcher_switch_by_name` | `POST /switch-by-name` | Switch by model + feature. |
| `llama_switcher_restart` | `POST /restart` | Restart current profile. |
| `llama_switcher_stop` | `POST /stop` | Stop the server. |
| `llama_switcher_open_dashboard` | `POST /open-dashboard` | Open the dashboard. |

## Behavior rules

- Prefer `switch_by_alias` over `switch_by_name`.
- If an alias is ambiguous, the API returns HTTP 409 and the skill throws with
  the list of possible matches — return those to the user instead of guessing.
- If Llama Switcher is unreachable, the skill throws:
  *"Llama Switcher is not reachable. Start the tray app first."*
- If the token is missing, the skill throws:
  *"LLAMA_SWITCHER_API_TOKEN is not configured."*
- The skill never starts scripts or kills processes directly.

## curl tests (Windows `cmd`)

```bat
curl http://127.0.0.1:47891/status ^
  -H "Authorization: Bearer YOUR_TOKEN"

curl -X POST http://127.0.0.1:47891/switch-by-alias ^
  -H "Authorization: Bearer YOUR_TOKEN" ^
  -H "Content-Type: application/json" ^
  -d "{\"alias\":\"Qwen-27B MTP\"}"
```

## Smoke test

With the Hermes profile environment loaded and Llama Switcher running:

```bash
python scripts/llama_switcher.py status
```
