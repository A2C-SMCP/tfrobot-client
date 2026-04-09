// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tfrobot_client_lib::services::shell_env::fix_path_env();
    tfrobot_client_lib::run()
}
