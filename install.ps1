# NASMirror installer.
#
#   irm https://raw.githubusercontent.com/abarriuso/nasmirror/main/install.ps1 | iex
#
# Downloads the NSIS installer from the latest GitHub release, installs it for
# the current user (no admin needed) and adds nasmirror.exe to the user PATH.

$ErrorActionPreference = 'Stop'
$Repo = 'abarriuso/nasmirror'

if ($env:OS -ne 'Windows_NT') {
    Write-Error 'NASMirror only runs on Windows (it relies on robocopy and the Windows networking API).'
    return
}

Write-Host 'Looking up the latest NASMirror release...'
$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
    -Headers @{ 'User-Agent' = 'nasmirror-installer' }
$asset = $release.assets | Where-Object { $_.name -like '*x64-setup.exe' } | Select-Object -First 1
if (-not $asset) {
    Write-Error "No x64 installer found in release $($release.tag_name)."
    return
}

$setup = Join-Path ([System.IO.Path]::GetTempPath()) $asset.name
Write-Host "Downloading $($asset.name) ($($release.tag_name))..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $setup -UseBasicParsing

Write-Host 'Installing...'
$proc = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
Remove-Item $setup -Force -ErrorAction SilentlyContinue
if ($proc.ExitCode -ne 0) {
    Write-Error "The installer exited with code $($proc.ExitCode)."
    return
}

$candidates = @(
    (Join-Path $env:LOCALAPPDATA 'NASMirror'),
    (Join-Path $env:ProgramFiles 'NASMirror')
)
$installDir = $candidates | Where-Object { Test-Path (Join-Path $_ 'nasmirror.exe') } | Select-Object -First 1
if (-not $installDir) {
    Write-Warning 'Installed, but nasmirror.exe was not found to add it to PATH.'
    return
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $installDir) {
    $newPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Added $installDir to your user PATH (open a new terminal to use it)."
}

Write-Host ''
Write-Host "NASMirror $($release.tag_name) installed in $installDir."
if (-not (Get-Command restic -ErrorAction SilentlyContinue)) {
    Write-Host 'Optional: install restic for encrypted, versioned backups:  winget install restic.restic'
}
