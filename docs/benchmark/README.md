# Benchmark: NASMirror vs File Explorer

The scripts behind the [benchmark in the main README](../../README.md#benchmark).
They copy generated test folders to a NAS with NASMirror's robocopy command
and with File Explorer's copy engine, and time both.

| Script | What it does |
|---|---|
| `make-data.py` | Writes the test folders: 6,000 small files, 400 photos, a 1.5 GB video, and a path over 260 characters. Random content, so compression cannot help either tool. |
| `bench.ps1` | Copies each folder to the NAS with both tools, alternating which goes first, and writes the times to `results.csv`. `-Incremental` adds the "copying again" tests (`-IncrementalSets small` for the small files only), `-LongPath` the long-path check. |
| `locked.ps1` | Copies the small files while one of them is held open by another program. |
| `shellcopy.ps1` | File Explorer's copy engine (`SHFileOperation`), used by the two scripts above. |

## Running it

From **PowerShell**, not Git Bash (Git Bash rewrites `\\nas\share` paths
into local ones):

```powershell
python make-data.py D:\bench-data small photos videos longpath
.\bench.ps1 -Data D:\bench-data -Dest \\nas\share\nasmirror-bench -Incremental -Cooldown 180
.\bench.ps1 -Data D:\bench-data -Dest \\nas\share\nasmirror-bench -Datasets none -Incremental -IncrementalSets small -Cooldown 60
.\locked.ps1 -Data D:\bench-data -Dest D:\bench-locked
```

- `-Dest` must be a folder that does not exist yet: every run writes into a
  subfolder of it and deletes that subfolder afterwards.
- `-Cooldown` waits that many seconds between runs. Many NAS write fast while
  their cache lasts and slow down after a few GB of continuous writing; the
  pause lets every run start from the same state, so the test measures the
  tools and not the NAS getting tired.
- The results and robocopy's logs go to `bench-out`, next to the test data.
- It needs about 2 GB free on the NAS and 6 GB on the PC (the test data, and
  a copy of it for the "copying again" tests). Delete the test data and
  `bench-out` when you are done; the NAS folder is left empty.
