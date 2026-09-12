pub mod audio;
pub mod burner;
pub mod commands;
pub mod config;
pub mod cuesheet;
pub mod downloader;
pub mod models;
pub mod pipeline;
pub mod spotify;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_config,
            commands::save_config,
            commands::spotify_login,
            commands::spotify_logout,
            commands::get_user_profile,
            commands::get_user_playlists,
            commands::fetch_spotify_tracks,
            commands::search_spotify,
            commands::get_optical_drives,
            commands::get_media_status,
            commands::eject_drive,
            commands::start_burn_job,
            commands::open_cache_folder,
            commands::calculate_capacity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running spotyburn application");
}

#[cfg(test)]
mod tests {
    use super::commands::greet;

    #[test]
    fn test_greet() {
        let result = greet("Tester");
        assert_eq!(result, "Hello, Tester! Welcome to SpotyBurn.");
    }
}
