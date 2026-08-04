export type UsageState = "free" | "busy" | "unknown";

export interface VramProcess {
  pid: number;
  name: string;
  usedMib: number;
}

export interface VramStatus {
  totalMib: number | null;
  usedMib: number | null;
  freeMib: number | null;
  modelMib: number | null;
  processes: VramProcess[];
}

export interface LlamaStatus {
  running: boolean;
  currentProfileId: string | null;
  alias: string | null;
  currentProfileName: string | null;
  model: string | null;
  feature: string | null;
  pid: number | null;
  healthy: boolean;
  serverReachable: boolean;
  usageState: UsageState;
  avgTokensPerSecond: number | null;
  vram: VramStatus;
}

export interface WidgetSettings {
  opacity: number;
  blur: number;
  refreshSeconds: number;
  startWithWindows: boolean;
  alwaysOnTop: boolean;
}
