// Shared TypeScript types mirroring the Rust serde structs.
// Field names use camelCase because the Rust side serializes with
// `#[serde(rename_all = "camelCase")]`.

export interface Profile {
  id: string;
  rawModel: string;
  rawFeature: string;
  prettyModel: string;
  prettyFeature: string;
  alias: string;
  displayName: string;
  scriptPath: string;
  workingDirectory: string;
  extension: string;
}

export interface IgnoredFile {
  filename: string;
  reason: string;
}

export interface ScanResult {
  profiles: Profile[];
  ignoredFiles: IgnoredFile[];
  scannedAt: string;
}

export type DefaultProfileMode = "none" | "lastUsed" | "specific";

export interface Settings {
  scriptsFolder: string;
  scanPattern: string;
  allowedExtensions: string[];
  serverPort: number;
  healthUrl: string;
  llamaServerApiKey: string | null;
  agentApiPort: number;
  agentApiToken: string;
  autoRescanOnStartup: boolean;
  autoRescanIntervalSeconds: number | null;
  defaultProfileMode: DefaultProfileMode;
  defaultProfileId: string | null;
  lastUsedProfileId: string | null;
  stopTimeoutSeconds: number;
  healthCheckTimeoutSeconds: number;
  // Image names of the llama.cpp server binary used to enforce a single running
  // instance (e.g. ["llama-server.exe"]). Round-trips through the settings form.
  serverProcessNames: string[];
}

export interface Status {
  running: boolean;
  currentProfileId: string | null;
  alias: string | null;
  currentProfileName: string | null;
  model: string | null;
  feature: string | null;
  scriptPath: string | null;
  pid: number | null;
  healthy: boolean;
  serverReachable: boolean;
  avgTokensPerSecond: number | null;
  serverPort: number;
  healthUrl: string;
  startedAt: string | null;
  usageState: "free" | "busy" | "unknown";
}

export interface BenchmarkPrompt {
  id: string;
  title: string;
  text: string;
}

export interface BenchmarkConfig {
  profileIds: string[];
  prompts: BenchmarkPrompt[];
  outputDir: string;
  timeoutSeconds: number;
}

export interface BenchmarkProgress {
  kind: "run" | "model" | "prompt";
  status: "running" | "done" | "error" | "switching" | "finished" | "cancelled";
  profileId: string | null;
  alias: string | null;
  promptId: string | null;
  outputPath: string | null;
  message: string | null;
  durationSeconds: number | null;
  tokensPerSecond: number | null;
}

export interface AgentApiInfo {
  baseUrl: string;
  port: number;
  token: string;
}

export interface LogEntry {
  filename: string;
  path: string;
  modifiedAt: string;
  sizeBytes: number;
}

export interface LogUpdate {
  text: string;
  nextOffset: number;
  truncated: boolean;
}
