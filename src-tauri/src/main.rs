// Prevent an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = llama_switcher_lib::run_grader_mode() {
        std::process::exit(code);
    }
    llama_switcher_lib::run()
}
