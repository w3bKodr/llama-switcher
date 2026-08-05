import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const widgetDir = join(projectRoot, "widget");
const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const tauriCli = join(widgetDir, "node_modules", ".bin", process.platform === "win32" ? "tauri.cmd" : "tauri");

function run(args) {
  return spawnSync(npm, args, {
    cwd: widgetDir,
    env: process.env,
    stdio: "inherit",
    // Windows cannot execute npm.cmd directly through spawnSync without a
    // command shell. All arguments here are fixed repository scripts.
    shell: process.platform === "win32",
  }).status ?? 1;
}

if (!existsSync(tauriCli)) {
  const installStatus = run(["ci"]);
  if (installStatus !== 0) process.exit(installStatus);
}

let buildStatus = run(["run", "tauri:build", "--", "--verbose"]);
// Windows security scanners occasionally hold the freshly linked executable
// for a moment. A clean retry consistently succeeds once that lock clears.
if (buildStatus !== 0) {
  console.warn("Widget packaging was temporarily blocked; retrying once…");
  buildStatus = run(["run", "tauri:build", "--", "--verbose"]);
}
process.exit(buildStatus);
