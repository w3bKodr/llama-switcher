---
name: llama-switcher
description: Control the local Llama Switcher tray app to list profiles, switch llama.cpp servers, restart or stop the active server, rescan scripts, and open the dashboard.
version: 0.2.0
platforms: [windows]
metadata:
  hermes:
    tags: [llama.cpp, local-ai, model-switching, windows]
required_environment_variables:
  - name: LLAMA_SWITCHER_API_TOKEN
    prompt: Llama Switcher Agent Control API token
    required_for: Authenticated access to the local Llama Switcher API
  - name: LLAMA_SWITCHER_BASE_URL
    prompt: Llama Switcher Agent Control base URL
    required_for: Connecting to the local Llama Switcher API
---

# Llama Switcher

Use this skill when the user asks to inspect, switch, restart, stop, or rescan
their local llama.cpp server profiles managed by the Llama Switcher tray app.

Llama Switcher owns all process management. Never run its `.cmd`, `.bat`, or
`.ps1` profile scripts directly and never kill llama.cpp processes yourself.

## Commands

Run the bundled client with the local terminal tool:

```text
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" status
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" profiles
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" rescan
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" switch-alias "Qwen-27B MTP"
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" switch-name "Qwen" "MTP"
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" restart
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" stop
python "${HERMES_SKILL_DIR}/scripts/llama_switcher.py" open-dashboard
```

Prefer `switch-alias` when the user supplies a display name. Use
`switch-name` only when the model and feature are separately known. If an
alias is ambiguous, show the API's possible matches and ask the user to choose.

Before changing or stopping the active server, confirm the intended profile
when the request is ambiguous. After a change, report the returned status.

## Troubleshooting

- If the client says the token is not configured, open Llama Switcher → Agent
  Control and click **Install / update Hermes skill** for the active Hermes
  profile.
- If the app is unreachable, ask the user to start the Llama Switcher tray app.
- Skills installed during an existing Hermes conversation appear after a new
  session or `/reset`.

