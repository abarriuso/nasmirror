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
- The robocopy preview counted files that exist only in the destination as
  files to copy, so the mirror-mode deletion warning never showed and an
  add-only job with extra files in the destination never reported "no
  changes". They now count as deletions in mirror mode, are ignored in
  add-only mode, and no longer inflate the progress bar.
- The exclusion fields swallowed the space between entries, so only one
  folder or file pattern could be typed. Entries with spaces go in quotes.
- Number fields outside what robocopy and the job file accept (negative,
  decimals, more than 128 threads) are kept in range instead of failing the
  save or the copy.
- A job that failed at once (e.g. a missing source) could leave the window
  on an endless progress view instead of its result.
- Enter in the network password field starts the job; Escape closes the
  password prompt.
- **Delete** (job or history entry) and **Stop** never asked for
  confirmation: the dialog plugin replaces `window.confirm` with a function
  that always reads as "yes". They now ask in a native dialog, and anything
  but an explicit OK keeps the job, entry or copy. Lint (`no-alert`) keeps
  `alert` and `confirm` from coming back.
- One unreadable folder anywhere in the source (robocopy exit code 8 while
  scanning) aborted the whole job before copying anything. The rest is now
  copied; the preview warns, and the result names what could not be read.
  The preview also warns when robocopy reports a destination it cannot
  reach, instead of promising to copy every file there.
- A failed copy only said "Backup failed": the result and the history now
  carry robocopy's own explanation (e.g. the file in use and by what), in
  any Windows language, naming first the file robocopy gave up on rather
  than one it recovered from on retry. A failed scan says why instead of
  "exited with code 16".
- The progress counted a file that failed as copied, and a retried one once
  per attempt; files now count once robocopy has moved on from them.
- A new empty folder, when it was the only change, ended as "no changes"
  and was never created. The preview counts new folders too.
- A path that is not a full path (`\nas\share`, one backslash short, or a
  relative `backup`) was accepted and resolved against the current drive or
  the app's folder. It is now rejected before anything runs.
- Confirming the preview scanned the whole tree a second time; the copy now
  reuses what the preview found and starts at once.
- A cancelled copy was recorded as 0 files and 0 B; it now shows the files
  copied before stopping, without the folder count and exit code it cannot
  know.
- In mirror mode the result listed the deleted extras as "Files only in the
  destination"; it now says they were deleted.
- The time left showed nonsense (hundreds of hours) while robocopy worked
  through small files, since it went by bytes alone; it now follows files
  and bytes together, and waits for 10 s of data.
- The job form saved a network login with no user name and a Wake-on-LAN
  setting with no valid MAC, and kept spaces around the paths. It now says
  what is missing, and the job list shows the destination on its own line.

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
