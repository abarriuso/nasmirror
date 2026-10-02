use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::engine::types::{JobEvent, JobRequest, Profile};
use crate::engine::JobContext;
use crate::profiles;

#[derive(Default)]
pub struct JobRegistry {
    pub jobs: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

const EVENT_CHANNEL: &str = "nasmirror://job";

#[tauri::command]
pub fn list_profiles() -> Result<Vec<Profile>, String> {
    profiles::load_profiles().map_err(|e| format!("could not read the saved jobs: {e}"))
}

#[tauri::command]
pub fn save_profile(profile: Profile) -> Result<Vec<Profile>, String> {
    profiles::upsert_profile(profile).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_profile(id: String) -> Result<Vec<Profile>, String> {
    profiles::delete_profile(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_log_dir(app: AppHandle) -> Result<(), String> {
    let dir = profiles::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_history() -> Vec<crate::history::HistoryEntry> {
    crate::history::list(&profiles::log_dir())
}

/// The frontend sends only the entry id; `history` resolves it inside the log
/// folder and rejects anything that points outside it.
#[tauri::command]
pub fn read_log(id: String) -> Result<String, String> {
    crate::history::read_log(&profiles::log_dir(), &id)
}

#[tauri::command]
pub fn delete_history_entry(id: String) -> Result<Vec<crate::history::HistoryEntry>, String> {
    let dir = profiles::log_dir();
    crate::history::delete(&dir, &id)?;
    Ok(crate::history::list(&dir))
}

#[tauri::command]
pub async fn start_job(
    app: AppHandle,
    registry: State<'_, JobRegistry>,
    request: JobRequest,
) -> Result<String, String> {
    // Only one job at a time, so a double click cannot start two copies to the
    // same destination. Check and register under a single lock; otherwise two
    // concurrent calls could both pass the check.
    let job_id = Uuid::new_v4().to_string();
    let ctx = JobContext::new(job_id.clone());
    {
        let mut jobs = registry.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if !jobs.is_empty() {
            return Err("A job is already running. Wait for it to finish, or cancel it.".into());
        }
        jobs.insert(job_id.clone(), ctx.cancel.clone());
    }

    let log_dir = profiles::log_dir();
    let app_for_emit = app.clone();
    let job_id_for_cleanup = job_id.clone();

    tokio::spawn(async move {
        crate::engine::run_job(&ctx, request, &log_dir, move |event: JobEvent| {
            let _ = app_for_emit.emit(EVENT_CHANNEL, &event);
        })
        .await;
        // Drop the cancel flag of the finished job.
        app.state::<JobRegistry>()
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job_id_for_cleanup);
    });

    Ok(job_id)
}

#[tauri::command]
pub fn cancel_job(registry: State<'_, JobRegistry>, job_id: String) -> bool {
    if let Some(flag) = registry
        .jobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&job_id)
    {
        flag.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}
