// Thin wrapper around Tauri's invoke() so pages don't repeat command names.
import { invoke } from "@tauri-apps/api/core";
import type {
  AgentApiInfo,
  LogEntry,
  Profile,
  ScanResult,
  Settings,
  Status,
} from "./types";

export const api = {
  getStatus: () => invoke<Status>("get_status"),
  isServerReachable: () => invoke<boolean>("is_server_reachable"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<Settings>("save_settings", { settings }),
  rescanScripts: () => invoke<ScanResult>("rescan_scripts"),
  getDetectedProfiles: () => invoke<Profile[]>("get_detected_profiles"),
  getScanResult: () => invoke<ScanResult>("get_scan_result"),

  startProfile: (profileId: string) =>
    invoke<Status>("start_profile", { profileId }),
  switchProfile: (profileId: string) =>
    invoke<Status>("switch_profile", { profileId }),
  switchProfileByName: (model: string, feature: string) =>
    invoke<Status>("switch_profile_by_name", { model, feature }),
  switchProfileByAlias: (alias: string) =>
    invoke<Status>("switch_profile_by_alias", { alias }),
  stopServer: () => invoke<Status>("stop_server"),
  restartServer: () => invoke<Status>("restart_server"),

  readLatestLog: () => invoke<string>("read_latest_log"),
  readLog: (path: string) => invoke<string>("read_log", { path }),
  listLogs: () => invoke<LogEntry[]>("list_logs"),
  clearOldLogs: () => invoke<number>("clear_old_logs"),
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  openScriptsFolder: () => invoke<void>("open_scripts_folder"),

  getAgentApiInfo: () => invoke<AgentApiInfo>("get_agent_api_info"),
  regenerateAgentApiToken: () => invoke<string>("regenerate_agent_api_token"),

  browseFolder: () => invoke<string | null>("browse_folder"),
  detectHermesSkillDirs: () => invoke<string[]>("detect_hermes_skill_dirs"),
  installHermesSkill: (targetDir: string) =>
    invoke<string>("install_hermes_skill", { targetDir }),
};
