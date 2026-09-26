use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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

/// The saved jobs. Only a missing file means "no jobs yet": any other read
/// error (the file held open by an antivirus or a backup tool, no
/// permission, ...) is returned, because treating it as an empty list would
/// make the next save overwrite every job.
pub fn load_profiles() -> std::io::Result<Vec<Profile>> {
    load_from(&profiles_path())
}

fn load_from(path: &Path) -> std::io::Result<Vec<Profile>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(vec![]),
        // Not UTF-8: as unusable as malformed JSON.
        Err(e) if e.kind() == ErrorKind::InvalidData => {
            set_aside(path)?;
            return Ok(vec![]);
        }
        Err(e) => return Err(e),
    };
    match serde_json::from_str(&text) {
        Ok(profiles) => Ok(profiles),
        Err(_) => {
            set_aside(path)?;
            Ok(vec![])
        }
    }
}

/// Moves a corrupt profiles file aside so the next save does not overwrite it
/// with an empty list and lose every job. The name carries the time, so an
/// earlier corrupt copy is kept too. If it cannot be moved, the error stops
/// the save instead.
fn set_aside(path: &Path) -> std::io::Result<()> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    std::fs::rename(path, path.with_extension(format!("json.corrupt-{stamp}")))
}

pub fn save_profiles(profiles: &[Profile]) -> std::io::Result<()> {
    save_to(&profiles_path(), profiles)
}

fn save_to(path: &Path, profiles: &[Profile]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(profiles).map_err(std::io::Error::other)?;
    // Atomic write: an interruption leaves the previous file intact.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Passwords are never persisted: `Credentials.password` is cleared here as a
/// safeguard, since the GUI keeps it only in the in-memory request.
pub fn upsert_profile(profile: Profile) -> std::io::Result<Vec<Profile>> {
    upsert_in(&profiles_path(), profile)
}

fn upsert_in(path: &Path, mut profile: Profile) -> std::io::Result<Vec<Profile>> {
    if let Some(creds) = profile.credentials.as_mut() {
        creds.password.clear();
    }
    let mut profiles = load_from(path)?;
    if let Some(existing) = profiles.iter_mut().find(|p| p.id == profile.id) {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
    save_to(path, &profiles)?;
    Ok(profiles)
}

pub fn delete_profile(id: &str) -> std::io::Result<Vec<Profile>> {
    delete_in(&profiles_path(), id)
}

fn delete_in(path: &Path, id: &str) -> std::io::Result<Vec<Profile>> {
    let mut profiles = load_from(path)?;
    profiles.retain(|p| p.id != id);
    save_to(path, &profiles)?;
    Ok(profiles)
}

pub fn find_profile(id_or_name: &str) -> std::io::Result<Option<Profile>> {
    Ok(load_profiles()?
        .into_iter()
        .find(|p| p.id == id_or_name || p.name.eq_ignore_ascii_case(id_or_name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nasmirror-profiles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn profile(id: &str) -> Profile {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": format!("Job {id}"),
            "source": r"C:\source",
            "destination": r"D:\mirror",
            "mode": "mirror",
        }))
        .unwrap()
    }

    #[test]
    fn missing_file_means_no_jobs() {
        let dir = temp_dir();
        assert!(load_from(&dir.join("profiles.json")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saves_and_loads_jobs() {
        let dir = temp_dir();
        let path = dir.join("profiles.json");
        upsert_in(&path, profile("a")).unwrap();
        upsert_in(&path, profile("b")).unwrap();
        let ids: Vec<String> = load_from(&path).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(delete_in(&path, "a").unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_file_is_an_error_and_nothing_is_overwritten() {
        let dir = temp_dir();
        // A directory where the file should be: exists, but cannot be read.
        let path = dir.join("profiles.json");
        std::fs::create_dir_all(&path).unwrap();
        assert!(load_from(&path).is_err());
        assert!(upsert_in(&path, profile("new")).is_err());
        assert!(delete_in(&path, "a").is_err());
        assert!(path.is_dir());
        assert!(!path.with_extension("json.tmp").exists(), "nothing may be written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_set_aside_not_overwritten() {
        let dir = temp_dir();
        let path = dir.join("profiles.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_from(&path).unwrap().is_empty());
        assert!(!path.exists());
        let aside: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("profiles.json.corrupt-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read_to_string(aside[0].path()).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
