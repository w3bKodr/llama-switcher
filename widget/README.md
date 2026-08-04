# Llama Switcher Widget

A compact Windows desktop widget for the main Llama Switcher application. It displays the loaded model and feature, live VRAM usage, model allocation, average tokens per second, and current server usage.

The widget reads the existing authenticated Llama Switcher localhost API. Llama Switcher must be running for live telemetry.

Use the settings button in the title bar to customize transparency and glass blur, keep the widget always on top, choose the telemetry refresh interval, or register it to start automatically with Windows. Appearance and behavior preferences are remembered locally.

## Development

```powershell
npm install
npm run tauri:dev
```

## Build installer

```powershell
npm run tauri:build
```

The NSIS installer is written under `src-tauri/target/release/bundle/nsis/`.
