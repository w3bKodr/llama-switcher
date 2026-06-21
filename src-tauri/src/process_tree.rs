//! Windows-safe process-tree termination.
//!
//! Strategy: take a ToolHelp snapshot of all processes, build the parent→child
//! map, then walk it to find every descendant of a given root PID. This is
//! reliable, never relies on window titles, and only touches processes that are
//! genuinely descendants of the process we launched.

#[cfg(windows)]
mod imp {
    use std::collections::{HashMap, HashSet};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_TERMINATE,
    };

    /// pid -> parent pid for every running process.
    fn snapshot() -> HashMap<u32, u32> {
        let mut map = HashMap::new();
        unsafe {
            let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                Ok(h) => h,
                Err(_) => return map,
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        map
    }

    /// All descendants of `root` (including `root` itself), parents last.
    pub fn descendants(root: u32) -> Vec<u32> {
        let map = snapshot();
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (&pid, &ppid) in &map {
            children.entry(ppid).or_default().push(pid);
        }
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(pid) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            ordered.push(pid);
            if let Some(kids) = children.get(&pid) {
                for &k in kids {
                    stack.push(k);
                }
            }
        }
        ordered
    }

    fn terminate(pid: u32) -> bool {
        unsafe {
            match OpenProcess(PROCESS_TERMINATE, false, pid) {
                Ok(handle) => {
                    let ok = TerminateProcess(handle, 1).is_ok();
                    let _ = CloseHandle(handle);
                    ok
                }
                Err(_) => false,
            }
        }
    }

    /// Terminate the whole tree rooted at `root`, leaf processes first so that
    /// children are not reparented to init while we still want them dead.
    pub fn kill_tree(root: u32) {
        let mut pids = descendants(root);
        pids.reverse();
        for pid in pids {
            terminate(pid);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn descendants(root: u32) -> Vec<u32> {
        vec![root]
    }
    pub fn kill_tree(_root: u32) {}
}

pub use imp::{descendants, kill_tree};
