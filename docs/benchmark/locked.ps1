# One file in the folder is open in another program (locked, no sharing):
# what does each tool do with the other 5,999?
#
#   .\locked.ps1 -Data D:\bench-data -Dest D:\bench-locked
param([Parameter(Mandatory)] [string] $Data, [Parameter(Mandatory)] [string] $Dest)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'shellcopy.ps1')

$src = Join-Path $Data 'small'
$locked = Get-ChildItem -LiteralPath (Join-Path $src 'project05') -Recurse -File | Select-Object -First 1
$lock = [IO.File]::Open($locked.FullName, 'Open', 'ReadWrite', 'None')
try {
    foreach ($tool in 'Explorer', 'NASMirror') {
        $parent = Join-Path $Dest "locked-$tool"
        if (Test-Path $parent) { cmd /c "rd /s /q `"$parent`"" | Out-Null }
        New-Item -ItemType Directory -Force $parent | Out-Null
        $sw = [Diagnostics.Stopwatch]::StartNew()
        if ($tool -eq 'Explorer') {
            $aborted = $false
            $code = [ShellCopy]::Copy($src, $parent, [ref]$aborted)
            $note = "SHFileOperation returned 0x{0:X}, aborted={1}" -f $code, $aborted
        } else {
            & robocopy $src (Join-Path $parent 'small') /MIR /BYTES /NP /NC /FP /R:1 /W:2 /NDL /MT:16 > (Join-Path $Dest 'locked.log') 2>&1
            $note = "robocopy exit code $LASTEXITCODE"
        }
        $sw.Stop()
        $n = @(Get-ChildItem -LiteralPath $parent -Recurse -File).Count
        "{0}: {1} of 6000 files copied in {2:N1} s; {3}" -f $tool, $n, $sw.Elapsed.TotalSeconds, $note
        cmd /c "rd /s /q `"$parent`"" | Out-Null
    }
} finally {
    $lock.Dispose()
}
