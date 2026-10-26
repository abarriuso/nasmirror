# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- The preview and the copy show each step as it happens: checking the
  paths, waking the NAS, connecting to the share (and as which user),
  comparing, copying. Each has its time and the exact paths or command
  line. While comparing, a running count of the folders checked, the folder
  it is in and what it has found so far: a long scan over the network no
  longer looks stuck.
- Each job card shows its last run: when, how it ended, files, data and
  duration.
- The results screen names the job and its paths, gives robocopy's exit
  code, lists the steps with their commands, and shows the run's log in
  place.
- The copy log starts with the exact robocopy command.

### Fixed
- **Connecting to the NAS, and disconnecting after each job, closed files
  other programs had open on it**: a document opened from the share in Word
  or Excel lost its file, and with it any unsaved work. NASMirror now only
  drops a connection nothing is using; on a conflict with another user's
  session it says to close what is open there.
- **The check for an earlier copy only looked among the 12 largest things
  to delete.** A folder named like the source, smaller than a dozen other
  items in the destination, went unnoticed: no confirmation, and `--job`
  deleted it. It is now found wherever it ranks, and the preview's list says
  how many more items it does not name, and their size.
- The window and a scheduled `--job` could run at the same time, two
  robocopy runs on the same NAS. A job now refuses to start while another
  NASMirror is running one, and copies or deletes nothing.
- While copying, every file read as "Deleted" (and the job as "Deleting what
  is not in the source…", even in add-only mode) when the source was typed
  with forward slashes (`D:/Photos`); deletions from a destination named like
  the source (`D:\Photos backup`) read as copies. The copy now says which
  each one is.
- Robocopy's exit code 4 was explained as harmless attribute differences
  with "the data itself was copied correctly". It means a name that is a
  file on one side and a folder on the other, and that file is not in the
  copy; the result now says so.
- A job that hit an internal error left the window on its progress screen,
  and every later Start was refused as "a job is already running" until
  NASMirror was restarted.
- A mistyped argument (`--dryrun`) exited with code 2, which means
  "Cancelled"; it now exits with 6.
- The result of a failed restic backup had no "View log" button, though its
  log is where restic says why.
- The steps said "Waking the destination" and "Connecting to the
  destination" when the NAS was the source.
- The history list wrote "1 files" and counts without thousands separators.
- **Mirror mode deleted an earlier copy without a clear warning.** A job
  pointed at the root of a share (`\\nas\copy`), while its earlier copy lived
  in `\\nas\copy\Photos`, deleted that copy and everything else on the share,
  and the preview only gave a count. Now:
  - the preview lists what would be deleted, folder by folder, with sizes;
  - when the destination has a folder named like the source, it says so and
    offers to use that folder as the destination, in one click;
  - deleting from a drive or share root, 1,000 or more items, or 1 GB or
    more needs a box ticked before the copy (the button reads "Delete N and
    copy");
  - an unattended run (`--job`) refuses such deletions, says why and copies
    nothing;
  - the job form warns when mirror mode points at a drive or share root.
- A copy that was only deleting sat at 0% for minutes with a flat chart:
  deletions are now counted on the bar, in a "Deleted" tile and as the latest
  item, and the chart shows files copied or deleted per second next to the
  data rate.
- Closing the window during a copy stopped it without a word; it now asks,
  and a confirmed close cancels the job properly first.
- Closing NASMirror, ending its task or a crash during a scan or a copy left
  robocopy (or restic) running on its own, unseen: still copying, or
  deleting in mirror mode, and competing with the next run for the NAS. They
  now end with NASMirror.
- Cancel pressed while a scan was still starting did nothing.
- The network password prompt has a Cancel button, besides Escape.
- The job form rejects paths that are not full paths, and a network user
  name on a job with no network path, before saving instead of at the next
  run.
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
- Wake-on-LAN with no host set now waits for the NAS when it is the
  **source** (e.g. a NAS backed up to a USB drive), not only when it is the
  destination; before, the job only waited a fixed few seconds and could
  start before the NAS was up.
- A restic retention run (`forget --prune`) that fails is reported as
  "Finished with warnings" with the reason, instead of only in the log.
- The job form no longer saves Wake-on-LAN without a valid MAC address, or
  network credentials without a user name (every run would fail), and keeps
  a custom Wake-on-LAN timeout when the MAC or host is edited.
- The copy mode (mirror / add only) is hidden for restic jobs, which ignore
  it, instead of showing a misleading "Mirror" badge.
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
- The job form kept spaces around the paths and did not say why it could
  not be saved; it now names what is missing next to Save, and the job list
  shows the destination on its own line.

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
