// Shapes returned by the Llama Switcher local control API.

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
  serverPort: number;
  healthUrl: string;
  startedAt: string | null;
}

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

export interface SwitchByAliasInput {
  alias: string;
}

export interface SwitchByNameInput {
  model: string;
  feature: string;
}

export interface SwitchByIdInput {
  profileId: string;
}
