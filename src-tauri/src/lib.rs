mod ara;
mod overlay;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ara::Pending::default())
        .invoke_handler(tauri::generate_handler![
            ara::start_login,
            ara::finish_login,
            ara::cancel_login,
            ara::sign_out,
            ara::fetch_challenges,
            overlay::overlay_status,
            overlay::download_overlay,
            overlay::open_overlay,
            overlay::overlay_settings,
            overlay::save_overlay_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
