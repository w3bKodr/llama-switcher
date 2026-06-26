//! Shared application state, guarded by std mutexes.

use crate::script_scanner::{Profile, ScanResult};
use crate::settings::Settings;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;

/// The currently running managed llama.cpp server (the shell we launched plus
/// metadata). The actual server is usually a child of this shell process, which
/// is why stop logic kills the whole tree.
pub struct RunningProcess {
    pub profile: Profile,
    /// PID of the launched shell (cmd.exe / powershell.exe). Tree root.
    pub pid: u32,
    pub child: Child,
    pub started_at: String,
    pub log_path: PathBuf,
    pub healthy: bool,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub scan: Mutex<ScanResult>,
    pub running: Mutex<Option<RunningProcess>>,
    /// Serializes start/stop/switch so two launches can never race and leave two
    /// servers running. Held for the whole duration of an activate or stop.
    pub op_lock: Mutex<()>,
    /// Serializes external-server detection/takeover and remembers an unknown
    /// listener so status polling does not repeatedly query process ancestry.
    pub takeover_lock: Mutex<()>,
    pub external_pid_checked: Mutex<Option<u32>>,
    /// If the llama.cpp API protects `/slots`, disable usage probing for the
    /// current run after the first 401/403 so logs do not fill with retries.
    pub usage_probe_disabled: Mutex<bool>,
    pub settings_path: PathBuf,
    pub logs_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UsageState {
    Free,
    Busy,
    Unknown,
}

impl AppState {
    pub fn new(settings: Settings, scan: ScanResult, settings_path: PathBuf, logs_dir: PathBuf) -> Self {
        AppState {
            settings: Mutex::new(settings),
            scan: Mutex::new(scan),
            running: Mutex::new(None),
            op_lock: Mutex::new(()),
            takeover_lock: Mutex::new(()),
            external_pid_checked: Mutex::new(None),
            usage_probe_disabled: Mutex::new(false),
            settings_path,
            logs_dir,
        }
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn profiles(&self) -> Vec<Profile> {
        self.scan.lock().unwrap().profiles.clone()
    }

    pub fn find_profile(&self, id: &str) -> Option<Profile> {
        self.scan
            .lock()
            .unwrap()
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }
}

/// Serialized status returned to the frontend and the local API.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub current_profile_id: Option<String>,
    pub alias: Option<String>,
    pub current_profile_name: Option<String>,
    pub model: Option<String>,
    pub feature: Option<String>,
    pub script_path: Option<String>,
    pub pid: Option<u32>,
    pub healthy: bool,
    /// True when the configured health URL responds, even if the server is not
    /// managed by Llama Switcher (e.g. started externally).
    pub server_reachable: bool,
    pub server_port: u16,
    pub health_url: String,
    pub started_at: Option<String>,
    pub usage_state: UsageState,
}

impl AppState {
    pub fn status(&self) -> Status {
        let settings = self.settings.lock().unwrap();
        let running = self.running.lock().unwrap();
        match running.as_ref() {
            Some(rp) => Status {
                running: true,
                current_profile_id: Some(rp.profile.id.clone()),
                alias: Some(rp.profile.alias.clone()),
                current_profile_name: Some(rp.profile.display_name.clone()),
                model: Some(rp.profile.pretty_model.clone()),
                feature: Some(rp.profile.pretty_feature.clone()),
                script_path: Some(rp.profile.script_path.clone()),
                pid: Some(rp.pid),
                healthy: rp.healthy,
                server_reachable: false,
                server_port: settings.server_port,
                health_url: settings.health_url.clone(),
                started_at: Some(rp.started_at.clone()),
                usage_state: UsageState::Unknown,
            },
            None => Status {
                running: false,
                current_profile_id: None,
                alias: None,
                current_profile_name: None,
                model: None,
                feature: None,
                script_path: None,
                pid: None,
                healthy: false,
                server_reachable: false,
                server_port: settings.server_port,
                health_url: settings.health_url.clone(),
                started_at: None,
                usage_state: UsageState::Unknown,
            },
        }
    }
}
