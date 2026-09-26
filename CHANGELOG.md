# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed
- Network paths written as `//nas/share` or with spaces around them now get
  the job's credentials and Wake-on-LAN, like `\\nas\share`; paths are
  trimmed before validating.
- A `profiles.json` that cannot be read (held open by another program, no
  permission) is reported as an error instead of being overwritten with just
  the job being saved. A corrupt file is set aside with a timestamp.
- Restic exit code 3 (snapshot created, some files unreadable) is reported
  as "Finished with warnings" instead of "Backup failed"; retention is
  skipped for that run.

## [0.9.0] - 2026-09-24

First public release of NASMirror, a **Tauri 2 + React + Rust** desktop app.

### Added
- Several named sync jobs (profiles), stored in `%APPDATA%\NASMirror`.
  `profiles.json` is written atomically; a corrupt file is set aside as
  `profiles.json.corrupt` instead of being overwritten.
- A copy engine chosen per job: **robocopy** (direct copy) or **restic**
  (encrypted, deduplicated versions, with `forget --prune` retention).
- A real preview before copying (robocopy `/L`) that warns about deletions in
  mirror mode. The scan shows its phase (Wake-on-LAN, connection, scan) and
  can be cancelled.
- Path validation: the source and destination may not be the same, nor one
  inside the other. Local paths are normalised and canonicalised before the
  check.
- Live progress: 10 s / 60 s average speed, ETA as a range, and a speed chart
  interpolated to the monitor's refresh rate.
- SMB mounting through `WNetAddConnection2W` (non-interactive, 20 s timeout).
  Mapped network drives are never disconnected; common network errors are
  explained in plain language.
- Wake-on-LAN, waiting until the NAS answers on port 445.
- Robocopy output is decoded as UTF-8 with a fallback to the system OEM code
  page, so copies work on non-English Windows and accented paths display
  correctly.
- Unattended mode: `nasmirror.exe --job "<name>" [--dry-run]` with a real exit
  code, for Task Scheduler. Jobs that need a password exit with code 5.
- Run history: every real run leaves a record next to its log (a run rejected
  before copying, e.g. because the source is missing, too), and the History
  screen lists them (date, job, result, files, data and duration) with
  the full log one click away.
- A system notification when a copy ends, only if the window is not in the
  foreground.
- Unit tests for the engine (cargo test) and the frontend (vitest), CI on
  GitHub Actions (lint, build, both test suites, clippy) and a release
  workflow that builds MSI/NSIS installers.
- `install.ps1`: one-line install for the current user, adding
  `nasmirror.exe` to the PATH.
