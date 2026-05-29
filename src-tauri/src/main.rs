// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if steam_workshop_uploader_lib::try_handle_steam_status_probe_process() {
        return;
    }
    steam_workshop_uploader_lib::run()
}
