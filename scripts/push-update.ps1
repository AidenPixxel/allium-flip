<#
.SYNOPSIS
    Pushes an Allium build onto the handheld over Wi-Fi, so updating needs no SD card.

.DESCRIPTION
    Fetches a release on this machine and uploads it to the file server the device already runs,
    landing it at /mnt/SDCARD/allium-ota.zip. The boot script picks it up from there and installs
    it on the next start.

    Why not let the device fetch it itself: its reqwest is built with no TLS backend at all, so it
    cannot reach https://api.github.com. Doing the download here, where TLS works, and uploading
    over plain HTTP on the LAN sidesteps that entirely.

    The archive is checked before it is uploaded, so a download cut short never reaches the
    device. The install script verifies it again on the device before extracting.

.PARAMETER Device
    The handheld's IP address, from Settings > Wi-Fi > IP Address.

.PARAMETER Zip
    A local build to push instead of the latest release. Use this for a build that was never
    released -- a side-branch artifact, say.

.PARAMETER Repo
    The GitHub repository to take the latest release from.

.EXAMPLE
    .\scripts\push-update.ps1 -Device 192.168.1.42

.EXAMPLE
    .\scripts\push-update.ps1 -Device 192.168.1.42 -Zip ~\Downloads\allium-armv7-unknown-linux-gnueabihf.zip

.NOTES
    Needs Wi-Fi on and Settings > Wi-Fi > Web File Explorer on. Note that the device pings 1.1.1.1
    before starting that server, so it will not come up on a LAN with no route to the internet.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Device,

    [string]$Zip,

    [string]$Repo = "AidenPixxel/allium-flip"
)

$ErrorActionPreference = "Stop"

# The asset name is a fixed contract: it has to match RELEASE_FILE in crates/allium-launcher/src/ota.rs
# and the name CI publishes.
$AssetName = "allium-armv7-unknown-linux-gnueabihf.zip"

# The name the boot script looks for, per static/.tmp_update/updater
$OtaName = "allium-ota.zip"

# `curl` in PowerShell is an alias for Invoke-WebRequest, which does not take -T. The real binary
# ships in System32 on Windows 10 and later, and is what handles a large upload with a progress bar.
$Curl = (Get-Command curl.exe -ErrorAction SilentlyContinue).Source
if (-not $Curl) {
    throw "curl.exe not found. It ships with Windows 10 and later in System32."
}

function Test-ZipIsWhole {
    param([string]$Path)

    # A zip keeps its index at the end of the file, so a truncated one cannot be opened at all --
    # which is exactly the failure a dropped connection produces.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    try {
        $archive = [System.IO.Compression.ZipFile]::OpenRead($Path)
    } catch {
        return $false
    }

    try {
        # An Allium build, not just any archive. Extracting something else over the SD root would
        # do real damage.
        #
        # Either separator: CI builds the release with `zip -r` on Linux, which writes forward
        # slashes, but PowerShell's own Compress-Archive writes backslashes -- so a zip made on
        # Windows would otherwise be rejected for the wrong reason.
        $names = $archive.Entries | ForEach-Object { $_.FullName -replace '\\', '/' }
        $hasAllium = @($names | Where-Object { $_ -like '.allium/*' }).Count -gt 0
        $hasUpdater = @($names | Where-Object { $_ -like '.tmp_update/*' }).Count -gt 0
        if (-not ($hasAllium -and $hasUpdater)) {
            Write-Warning "Archive has no .allium/ and .tmp_update/ -- this does not look like an Allium build."
            return $false
        }
        return $true
    } finally {
        $archive.Dispose()
    }
}

$temp = $null

try {
    if ($Zip) {
        $source = (Resolve-Path -LiteralPath $Zip).Path
        Write-Host "Using $source"
    } else {
        # GitHub's documented permalink for a release asset, so this needs no API call and no JSON
        # parsing -- curl just follows the redirect.
        $url = "https://github.com/$Repo/releases/latest/download/$AssetName"
        $temp = Join-Path ([System.IO.Path]::GetTempPath()) "allium-ota-$(Get-Random).zip"
        Write-Host "Fetching the latest release from $Repo ..."
        & $Curl -fL --progress-bar -o $temp $url
        if ($LASTEXITCODE -ne 0) {
            throw "Download failed. Is there a published release with an $AssetName asset?"
        }
        $source = $temp
    }

    $size = [math]::Round((Get-Item -LiteralPath $source).Length / 1MB, 1)
    Write-Host "Checking the archive ($size MB) ..."
    if (-not (Test-ZipIsWhole $source)) {
        throw "The archive is incomplete or is not an Allium build. Nothing was uploaded."
    }

    Write-Host "Uploading to http://$Device/$OtaName ..."
    & $Curl -f --progress-bar -T $source "http://$Device/$OtaName"
    if ($LASTEXITCODE -ne 0) {
        throw "Upload failed. Check the IP, and that Settings > Wi-Fi > Web File Explorer is on."
    }

    Write-Host ""
    Write-Host "Uploaded. Restart the handheld to install it." -ForegroundColor Green
} finally {
    if ($temp -and (Test-Path -LiteralPath $temp)) {
        Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
    }
}
