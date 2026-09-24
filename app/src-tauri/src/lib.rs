pub mod commands;
pub mod engine;
pub mod history;
pub mod profiles;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::JobRegistry::default())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::open_log_dir,
            commands::list_history,
            commands::read_log,
            commands::delete_history_entry,
            commands::start_job,
            commands::cancel_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
