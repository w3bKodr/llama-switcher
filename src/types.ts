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
export type NetworkAccessMode = "localOnly" | "cloudflareTunnel" | "directInternet" | "whitelistOnly";

export interface SecurityBan {
  ip: string;
  reason: string;
  failureCount: number;
  createdAt: string;
  expiresAt: string | null;
  automatic: boolean;
}

export interface Settings {
  scriptsFolder: string;
  scanPattern: string;
  allowedExtensions: string[];
  serverPort: number;
  healthUrl: string;
  llamaServerApiKey: string | null;
  requireServerApiKey: boolean;
  securityGatewayEnabled: boolean;
  securityGatewayPort: number;
  networkAccessMode: NetworkAccessMode;
  trustedNetworks: string[];
  autoBanEnabled: boolean;
  autoBanFailureThreshold: number;
  autoBanWindowSeconds: number;
  autoBanDurationSeconds: number;
  agentApiPort: number;
  agentApiToken: string;
  autoRescanOnStartup: boolean;
  autoRescanIntervalSeconds: number | null;
  defaultProfileMode: DefaultProfileMode;
  defaultProfileId: string | null;
  lastUsedProfileId: string | null;
  favoriteProfileIds: string[];
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
  avgSpeculativeAcceptanceRate: number | null;
  serverPort: number;
  healthUrl: string;
  startedAt: string | null;
  usageState: "free" | "busy" | "unknown";
  vram: VramStatus;
}

export interface VramStatus {
  totalMib: number | null;
  usedMib: number | null;
  freeMib: number | null;
  modelMib: number | null;
  processes: VramProcess[];
}

export interface VramProcess {
  pid: number;
  name: string;
  usedMib: number;
}

export interface BenchmarkPrompt {
  id: string;
  title: string;
  text: string;
  enabled: boolean;
}

export type BenchmarkDifficulty = "easy" | "medium" | "hard";

export interface ProfessionalBenchmarkSummary {
  id: string;
  suiteId: string;
  suiteVersion: number;
  title: string;
  description: string;
  difficulty: BenchmarkDifficulty;
  weight: number;
  prompt: string;
  functionSignature: string;
  publicTestCount: number;
  totalTestCount: number;
}

export interface BenchmarkGrade {
  benchmarkId: string;
  suiteVersion: number;
  status: string;
  score: number;
  passed: number;
  failed: number;
  total: number;
  feedback: string[];
}

export interface BenchmarkConfig {
  profileIds: string[];
  prompts: BenchmarkPrompt[];
  professionalCaseIds: string[];
  outputDir: string;
  timeoutSeconds: number;
  modelStartTimeoutSeconds: number;
  gradingTimeoutSeconds: number;
  runsPerPrompt: number;
  generateHtmlReport: boolean;
  openReportWhenComplete: boolean;
  resumeCompletedRuns: boolean;
}

export interface BenchmarkProgress {
  kind: "run" | "model" | "prompt";
  status: "running" | "pausing" | "paused" | "resumed" | "iteration_done" | "iteration_error" | "done" | "error" | "switching" | "finished" | "cancelled";
  profileId: string | null;
  alias: string | null;
  promptId: string | null;
  outputPath: string | null;
  message: string | null;
  durationSeconds: number | null;
  tokensPerSecond: number | null;
  runIndex: number | null;
  runCount: number | null;
  draftTokens: number | null;
  acceptedDraftTokens: number | null;
  speculativeAcceptanceRate: number | null;
  score: number | null;
  passedTests: number | null;
  totalTests: number | null;
  gradeStatus: string | null;
  reportPath: string | null;
}

export interface AgentApiInfo {
  baseUrl: string;
  port: number;
  token: string;
}

export interface WidgetInstallStatus {
  installed: boolean;
  executablePath: string | null;
  startWithWindows: boolean;
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
