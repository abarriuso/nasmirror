// Prevents additional console window on Windows in release, DO NOT REMOVE.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

/// NASMirror — mirror folders to a NAS or drives with Robocopy.
///
/// With no arguments, opens the main window. With `--job <name>`, runs that
/// saved job without a window and exits; add `--dry-run` to only scan.
///
/// Exit codes: 0 success (or nothing to copy), 1 failure or connection error,
/// 2 cancelled, 3 job not found, 4 completed with mismatches,
/// 5 the job needs a password and cannot run without the window.
#[derive(Parser, Debug)]
#[command(name = "nasmirror", version)]
struct Cli {
    /// Name (or id) of a saved job to run without a window.
    #[arg(long)]
    job: Option<String>,
    /// Scan only (robocopy /L): nothing is copied or deleted.
    #[arg(long)]
    dry_run: bool,
}

fn main() {
    let cli = Cli::parse();
    if let Some(name) = cli.job {
        std::process::exit(run_headless(&name, cli.dry_run));
    }
    app_lib::run();
}

/// The job needs a password (network or restic) that is never stored, so it
/// cannot run without the window. Distinct from 4 ("completed with
/// mismatches") so a scheduled task can tell them apart.
const EXIT_NEEDS_WINDOW: i32 = 5;

fn run_headless(name: &str, dry_run: bool) -> i32 {
    use app_lib::engine::types::{JobRequest, Outcome};
    use app_lib::engine::JobContext;

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("could not start the async runtime: {e}");
            return 1;
        }
    };

    rt.block_on(async move {
        let Some(profile) = app_lib::profiles::find_profile(name) else {
            eprintln!("Job not found: '{name}'. Use the same name you gave it in the window.");
            return 3;
        };
        if profile.credentials.is_some() {
            // Passwords are never stored in the profile, so a headless job
            // cannot authenticate. Fail with a clear message instead of
            // hanging or failing silently.
            eprintln!(
                "Job '{name}' needs a network user and password, so it cannot run \
                 without a window yet. Use a destination that is already \
                 authenticated (a persistent network drive), or drop the \
                 credentials from the job."
            );
            return EXIT_NEEDS_WINDOW;
        }
        if profile.engine == app_lib::engine::types::Engine::Restic {
            // Same limitation: the restic repository password is never stored.
            eprintln!(
                "Job '{name}' uses the restic engine, which cannot run without a \
                 window yet: the repository password is never stored. Run it from \
                 the window, or switch the job to robocopy."
            );
            return EXIT_NEEDS_WINDOW;
        }

        let ctx = JobContext::new(uuid::Uuid::new_v4().to_string());
        let request = JobRequest {
            profile,
            password: None,
            restic_password: None,
            dry_run,
        };
        let log_dir = app_lib::profiles::log_dir();
        let result = app_lib::engine::run_job(&ctx, request, &log_dir, |_event| {}).await;

        eprintln!(
            "NASMirror: {:?} — {}",
            result.outcome,
            result.error.as_deref().unwrap_or(&result.log_path)
        );

        match result.outcome {
            Outcome::Success | Outcome::NoChanges => 0,
            Outcome::SuccessWithMismatches => 4,
            Outcome::Cancelled => 2,
            Outcome::Failed | Outcome::ConnectionError => 1,
        }
    })
}
