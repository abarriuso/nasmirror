use std::path::PathBuf;

use crate::engine::types::Profile;

/// App data folder. Resolved the same way by the GUI and the headless CLI, so a
/// scheduled headless run uses exactly the profiles and logs created in the
/// window.
pub fn app_data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("NASMirror")
}

pub fn log_dir() -> PathBuf {
    app_data_dir().join("logs")
}

fn profiles_path() -> PathBuf {
    app_data_dir().join("profiles.json")
}

pub fn load_profiles() -> Vec<Profile> {
    let path = profiles_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    match serde_json::from_str(&text) {
        Ok(profiles) => profiles,
        Err(_) => {
            // Move the corrupt file aside so the next save does not overwrite it
            // with an empty list and lose every job.
            let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
            vec![]
        }
    }
}

pub fn save_profiles(profiles: &[Profile]) -> std::io::Result<()> {
    let dir = app_data_dir();
    std::fs::create_dir_all(&dir)?;
    let text = serde_json::to_string_pretty(profiles).map_err(std::io::Error::other)?;
    // Atomic write: an interruption leaves the previous file intact.
    let tmp = profiles_path().with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, profiles_path())
}

/// Passwords are never persisted: `Credentials.password` is cleared here as a
/// safeguard, since the GUI keeps it only in the in-memory request.
pub fn upsert_profile(mut profile: Profile) -> std::io::Result<Vec<Profile>> {
    if let Some(creds) = profile.credentials.as_mut() {
        creds.password.clear();
    }
    let mut profiles = load_profiles();
    if let Some(existing) = profiles.iter_mut().find(|p| p.id == profile.id) {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
    save_profiles(&profiles)?;
    Ok(profiles)
}

pub fn delete_profile(id: &str) -> std::io::Result<Vec<Profile>> {
    let mut profiles = load_profiles();
    profiles.retain(|p| p.id != id);
    save_profiles(&profiles)?;
    Ok(profiles)
}

pub fn find_profile(id_or_name: &str) -> Option<Profile> {
    load_profiles()
        .into_iter()
        .find(|p| p.id == id_or_name || p.name.eq_ignore_ascii_case(id_or_name))
}
