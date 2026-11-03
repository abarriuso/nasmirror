# NASMirror (robocopy, the app's exact flags) vs File Explorer's copy engine
# (SHFileOperation, the shell copy engine Explorer uses), copying to a NAS.
# Run it from PowerShell, not Git Bash: Git Bash rewrites \\nas\share paths.
#
#   .\bench.ps1 -Data D:\bench-data -Dest \\nas\share\nasmirror-bench -Incremental -Cooldown 90
#
# -Data is the folder make-data.py filled; -Dest a folder on the NAS that does
# not exist yet (each run writes into a subfolder and deletes it afterwards).
param(
    [Parameter(Mandatory)] [string] $Data,
    [Parameter(Mandatory)] [string] $Dest,
    # Logs and results: next to the test data, not in the repository.
    [string] $Out = (Join-Path (Split-Path $Data -Parent) 'bench-out'),
    [string] $Results = (Join-Path $Out 'results.csv'),
    # small, photos, videos; "none" skips them (for -Incremental or -LongPath alone).
    [string[]] $Datasets = @('small', 'photos', 'videos'),
    [int] $Runs = 3,
    [switch] $Incremental,
    # The folders the "copying again" tests use.
    [string[]] $IncrementalSets = @('small', 'photos'),
    [switch] $LongPath,
    # Seconds to let the NAS flush its write cache between runs: without it,
    # a long series of big writes measures the NAS slowing down, not the tool.
    [int] $Cooldown = 0
)
$ErrorActionPreference = 'Stop'
# -File passes "a,b" as one string.
$Datasets = $Datasets -split ','
$IncrementalSets = $IncrementalSets -split ','

function Pause-ForNas { if ($Cooldown -gt 0) { Start-Sleep -Seconds $Cooldown } }

. (Join-Path $PSScriptRoot 'shellcopy.ps1')

$CommonArgs = @('/MIR', '/BYTES', '/NP', '/NC', '/FP', '/R:1', '/W:2')
$LogDir = Join-Path $Out 'logs'
New-Item -ItemType Directory -Force $LogDir | Out-Null

function Log($msg) {
    $line = "[{0:HH:mm:ss}] {1}" -f (Get-Date), $msg
    Write-Host $line
    # Best effort: a reader holding the file open must not stop the benchmark.
    try { Add-Content -Encoding utf8 -LiteralPath (Join-Path $Out 'progress.txt') -Value $line -ErrorAction Stop } catch {}
}

function Stats($dir) {
    $files = Get-ChildItem -LiteralPath $dir -Recurse -File -Force -ErrorAction SilentlyContinue
    [pscustomobject]@{ Files = @($files).Count; Bytes = ($files | Measure-Object Length -Sum).Sum }
}

function Get-LongPath($p) { if ($p.StartsWith('\\')) { '\\?\UNC\' + $p.Substring(2) } else { '\\?\' + $p } }

function Remove-Tree($dir) {
    if (Test-Path -LiteralPath $dir) { cmd /c "rd /s /q `"$dir`"" | Out-Null }
}

function Warm($dir) {
    # Read every source file once, so both tools read from the same RAM cache.
    foreach ($f in Get-ChildItem -LiteralPath $dir -Recurse -File) {
        $s = [IO.File]::OpenRead($f.FullName); $s.CopyTo([IO.Stream]::Null); $s.Dispose()
    }
}

function Run-Explorer($src, $parent) {
    New-Item -ItemType Directory -Force $parent | Out-Null
    $aborted = $false
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $code = [ShellCopy]::Copy($src, $parent, [ref]$aborted)
    $sw.Stop()
    [pscustomobject]@{ Scan = 0.0; Copy = $sw.Elapsed.TotalSeconds; Code = $code; Note = $(if ($aborted) { 'aborted' } else { '' }) }
}

function Run-Robocopy($src, $dst, $tag) {
    $log = Join-Path $LogDir "$tag.log"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & robocopy $src $dst @CommonArgs '/L' > "$log.scan" 2>&1
    $scan = $sw.Elapsed.TotalSeconds
    $sw.Restart()
    & robocopy $src $dst @CommonArgs '/NDL' '/MT:16' > $log 2>&1
    $code = $LASTEXITCODE
    $sw.Stop()
    [pscustomobject]@{ Scan = $scan; Copy = $sw.Elapsed.TotalSeconds; Code = $code; Note = '' }
}

function Record($test, $dataset, $tool, $run, $r, $stats) {
    $row = [pscustomobject]@{
        Test = $test; Dataset = $dataset; Tool = $tool; Run = $run
        ScanS = [math]::Round($r.Scan, 2); CopyS = [math]::Round($r.Copy, 2)
        TotalS = [math]::Round($r.Scan + $r.Copy, 2)
        Files = $stats.Files; Bytes = $stats.Bytes; Code = $r.Code; Note = $r.Note
        At = (Get-Date -Format s)
    }
    $row | Export-Csv -Append -NoTypeInformation -LiteralPath $Results
    Log ("{0} {1} {2} #{3}: scan {4}s copy {5}s total {6}s, {7} files, {8:N0} bytes, code {9} {10}" -f `
        $test, $dataset, $tool, $run, $row.ScanS, $row.CopyS, $row.TotalS, $row.Files, $row.Bytes, $row.Code, $row.Note)
}

$order = @('Explorer', 'NASMirror', 'NASMirror', 'Explorer', 'Explorer', 'NASMirror', 'NASMirror', 'Explorer', 'Explorer', 'NASMirror')
foreach ($ds in $Datasets | Where-Object { $_ -and $_ -ne 'none' }) {
    $src = Join-Path $Data $ds
    Log "warming $ds"; Warm $src
    $n = @{ Explorer = 0; NASMirror = 0 }
    foreach ($tool in $order[0..(2 * $Runs - 1)]) {
        $n[$tool]++
        $parent = Join-Path $Dest "$ds-$tool-$($n[$tool])"
        $target = Join-Path $parent $ds
        Remove-Tree $parent
        if ($tool -eq 'Explorer') { $r = Run-Explorer $src $parent } else { $r = Run-Robocopy $src $target "$ds-$($n[$tool])" }
        Record 'full' $ds $tool $n[$tool] $r (Stats $target)
        Remove-Tree $parent
        Pause-ForNas
    }
}

if ($Incremental) {
    # Copy the folders once, then re-run with nothing changed and with ~2% changed.
    $src = Join-Path $Data 'incr'
    Remove-Tree $src
    New-Item -ItemType Directory -Force $src | Out-Null
    foreach ($ds in $IncrementalSets) {
        & robocopy (Join-Path $Data $ds) (Join-Path $src $ds) /E /MT:16 /NFL /NDL /NJH /NJS | Out-Null
    }
    $sets = $IncrementalSets -join '+'
    Warm $src
    $parent = Join-Path $Dest 'incr'
    $target = Join-Path $parent 'incr'
    Remove-Tree $parent
    & robocopy $src $target /MIR /MT:16 /NFL /NDL /NJH /NJS | Out-Null
    Pause-ForNas
    $all = Get-ChildItem -LiteralPath $src -Recurse -File
    $rng = [Random]::new(7)
    for ($i = 1; $i -le $Runs; $i++) {
        Record 'unchanged' $sets 'NASMirror' $i (Run-Robocopy $src $target "unchanged-$i") (Stats $target)
    }
    for ($i = 1; $i -le $Runs; $i++) {
        # Rewrite 2% of the files with new content (same size, new date).
        $pick = $all | Sort-Object { $rng.Next() } | Select-Object -First ([math]::Ceiling($all.Count * 0.02))
        foreach ($f in $pick) {
            $buf = [byte[]]::new($f.Length); $rng.NextBytes($buf); [IO.File]::WriteAllBytes($f.FullName, $buf)
        }
        Warm $src
        $r = Run-Robocopy $src $target "changed-$i"
        $r.Note = "$($pick.Count) files changed"
        Record 'changed2pct' $sets 'NASMirror' $i $r (Stats $target)
    }
    for ($i = 1; $i -le $Runs; $i++) {
        # Explorer has no "only what changed": copying again means replacing everything.
        Pause-ForNas
        Record 'unchanged' $sets 'Explorer (replace all)' $i (Run-Explorer $src $parent) (Stats $target)
    }
    Remove-Tree $parent
    Remove-Tree $src
}

if ($LongPath) {
    $src = Join-Path $Data 'longpath'
    foreach ($tool in 'Explorer', 'NASMirror') {
        $parent = Join-Path $Dest "longpath-$tool"
        $target = Join-Path $parent 'longpath'
        Remove-Tree $parent
        if ($tool -eq 'Explorer') { $r = Run-Explorer $src $parent } else { $r = Run-Robocopy $src $target 'longpath' }
        $deep = Get-ChildItem -LiteralPath (Get-LongPath $target) -Recurse -File -Force -ErrorAction SilentlyContinue |
            Where-Object Name -eq 'report-with-a-long-name.pdf'
        $r.Note = $(if ($deep) { 'long path copied' } else { 'long path MISSING' }) + $(if ($r.Note) { "; $($r.Note)" } else { '' })
        Record 'longpath' 'longpath' $tool 1 $r (Stats (Get-LongPath $target))
        Remove-Tree (Get-LongPath $parent)
    }
}
Log 'done'
