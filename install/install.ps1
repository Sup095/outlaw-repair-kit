<#
.SYNOPSIS
    Installer for the Outlaw Repair Kit, by Outlaw Systems.

.DESCRIPTION
    Works out which release fits this machine, downloads it, refuses to install
    it if the checksum does not match, and puts `outlaw` on your PATH. It can
    also install the desktop app and set up a local model, but only if asked --
    and it prints the exact command before running anything that is not its own
    business.

.EXAMPLE
    irm https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Desktop -LocalModel
#>
[CmdletBinding()]
param(
    # A specific release tag, e.g. v0.4.0. Defaults to the newest.
    [string] $Version = "latest",

    # Where to put the program.
    [string] $Dir = "$env:LOCALAPPDATA\Programs\OutlawRepairKit",

    # Also install the desktop app.
    [switch] $Desktop,

    # Also set up a model on this machine.
    [switch] $LocalModel,

    # Do not ask about the local model at all.
    [switch] $NoLocalModel,

    # Do not ask anything; take the safe default each time.
    [switch] $Yes
)

$ErrorActionPreference = "Stop"
$Repo = "Sup095/outlaw-repair-kit"

function Write-Step($message) { Write-Host "==> " -ForegroundColor DarkYellow -NoNewline; Write-Host $message }
function Write-Warn($message) { Write-Host "warning: " -ForegroundColor Yellow -NoNewline; Write-Host $message }
function Stop-With($message) { Write-Host "error: " -ForegroundColor Red -NoNewline; Write-Host $message; exit 1 }

function Read-YesNo($question) {
    # Answers no unless the person says yes, and answers no on its own when
    # there is nobody there to ask.
    if ($Yes -or -not [Environment]::UserInteractive) { return $false }
    $reply = Read-Host "$question [y/N]"
    return $reply -match '^(y|yes)$'
}

if ([Environment]::Is64BitOperatingSystem -eq $false) {
    Stop-With "no 32-bit build is published -- build from source, see the README"
}
$target = "x86_64-pc-windows-msvc"

Write-Step "Finding the release to install"
if ($Version -eq "latest") {
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "outlaw-installer" }
        $Version = $release.tag_name
    } catch {
        Stop-With "could not work out the newest release -- try -Version v0.4.0"
    }
}
Write-Host "  $Version for $target"

$asset = "outlaw-$Version-$target.zip"
$base = "https://github.com/$Repo/releases/download/$Version"
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("outlaw-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $work | Out-Null

try {
    Write-Step "Downloading $asset"
    $archive = Join-Path $work $asset
    try {
        Invoke-WebRequest -Uri "$base/$asset" -OutFile $archive -UseBasicParsing
    } catch {
        Stop-With "could not download $asset -- check https://github.com/$Repo/releases"
    }

    Write-Step "Checking what was downloaded"
    $sums = Join-Path $work "SHA256SUMS"
    $verified = $false
    try {
        Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sums -UseBasicParsing
        $line = Get-Content $sums | Where-Object { $_ -match [regex]::Escape($asset) } | Select-Object -First 1
        if ($null -eq $line) {
            Write-Warn "that release publishes no checksum for $asset"
        } else {
            $expected = ($line -split '\s+')[0]
            $actual = (Get-FileHash -Path $archive -Algorithm SHA256).Hash
            if ($expected -ine $actual) {
                # A file that is not the published one is not going anywhere
                # near anybody's PATH.
                Stop-With "the download does not match its published checksum -- not installing it"
            }
            Write-Host "  checksum matches"
            $verified = $true
        }
    } catch {
        if (-not $verified) { Write-Warn "that release publishes no SHA256SUMS file, so the download could not be verified" }
    }

    Write-Step "Installing to $Dir"
    if (-not (Test-Path $Dir)) { New-Item -ItemType Directory -Path $Dir -Force | Out-Null }
    Expand-Archive -Path $archive -DestinationPath $work -Force
    $binary = Get-ChildItem -Path $work -Recurse -Filter "outlaw.exe" | Select-Object -First 1
    if ($null -eq $binary) { Stop-With "the archive did not contain the program" }
    Copy-Item -Path $binary.FullName -Destination (Join-Path $Dir "outlaw.exe") -Force
    Write-Host "  $Dir\outlaw.exe"

    # Only the user's own PATH is touched, never the machine's, so this needs
    # no administrator rights and affects nobody else who uses this computer.
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$Dir*") {
        Write-Step "Adding it to your PATH"
        $updated = if ([string]::IsNullOrEmpty($userPath)) { $Dir } else { "$userPath;$Dir" }
        [Environment]::SetEnvironmentVariable("Path", $updated, "User")
        Write-Host "  open a new terminal for this to take effect"
    }

    if ($Desktop) {
        Write-Step "Installing the desktop app"
        $installer = "Outlaw Repair Kit_$($Version.TrimStart('v'))_x64-setup.exe"
        $downloaded = Join-Path $work $installer
        # The published name contains spaces, which have to be escaped in a URL
        # even though they are fine in a file name.
        $url = "$base/" + [Uri]::EscapeDataString($installer)
        try {
            Invoke-WebRequest -Uri $url -OutFile $downloaded -UseBasicParsing
            Write-Host "  starting the installer -- Windows will ask you to confirm it"
            # Started, not silenced: installing an application is the user's
            # decision to confirm, not this script's to make for them.
            Start-Process -FilePath $downloaded -Wait
        } catch {
            Write-Warn "no desktop installer was published for $Version -- see https://github.com/$Repo/releases"
        }
    }
} finally {
    Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
}

# --- optional: a model on this machine --------------------------------------
#
# The tool runs every check and explains known problems with no model at all.
# A model only helps with problems that are not in the runbook library, so this
# is a question rather than a step.

function Get-VramGb {
    $smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    if ($null -ne $smi) {
        $megabytes = & $smi.Source --query-gpu=memory.total --format=csv,noheader,nounits 2>$null | Select-Object -First 1
        if ($megabytes -match '^\d+$') { return [int]([int]$megabytes / 1024) }
    }
    try {
        $adapter = Get-CimInstance Win32_VideoController -ErrorAction Stop |
            Sort-Object AdapterRAM -Descending | Select-Object -First 1
        # AdapterRAM is a 32-bit field, so anything above 4GB reads as 4095MB.
        # Treated as a floor, not a measurement.
        if ($adapter.AdapterRAM -gt 0) { return [int]($adapter.AdapterRAM / 1GB) }
    } catch { }
    return 0
}

function Get-ModelForVram([int] $vram) {
    # Sized so the model fits with room for its context, rather than the
    # largest one that technically loads.
    if ($vram -ge 22) { return "qwen3:32b" }
    if ($vram -ge 14) { return "qwen3:14b" }
    if ($vram -ge 10) { return "qwen3:8b" }
    if ($vram -ge 6) { return "qwen3:4b" }
    return "qwen3:1.7b"
}

$wantModel = $LocalModel.IsPresent
if (-not $wantModel -and -not $NoLocalModel) {
    Write-Host ""
    Write-Host "The tool runs every check and explains known problems with no model at all."
    Write-Host "A model only helps with problems that are not in the runbook library."
    $wantModel = Read-YesNo "Set up a model on this machine as well?"
}

if ($wantModel) {
    Write-Step "Setting up a local model"
    if ($null -eq (Get-Command ollama -ErrorAction SilentlyContinue)) {
        Write-Host "  Ollama is not installed. It is what runs the model."
        if ($null -ne (Get-Command winget -ErrorAction SilentlyContinue)) {
            Write-Host "  It would be installed with:"
            Write-Host "      winget install --id Ollama.Ollama -e"
            if (Read-YesNo "  Run that now?") {
                winget install --id Ollama.Ollama -e --accept-package-agreements --accept-source-agreements
            } else {
                Write-Warn "skipping the model. Install Ollama or LM Studio later and the tool will find it."
                $wantModel = $false
            }
        } else {
            Write-Warn "winget is not available -- install Ollama from https://ollama.com/download, or use LM Studio"
            $wantModel = $false
        }
    }
}

if ($wantModel -and $null -ne (Get-Command ollama -ErrorAction SilentlyContinue)) {
    $vram = Get-VramGb
    $model = Get-ModelForVram $vram
    if ($vram -gt 0) {
        Write-Host "  ${vram}GB of video memory found -- $model fits comfortably"
    } else {
        Write-Host "  no graphics card found, so $model was chosen to run on the processor"
    }
    if (Read-YesNo "  Download $model now? (several gigabytes)") {
        ollama pull $model
    } else {
        Write-Host "  Skipped. Run 'ollama pull $model' whenever you like."
    }
}

Write-Host ""
Write-Step "Done"
Write-Host "  outlaw boot      check everything is working"
Write-Host "  outlaw scan      look for problems"
Write-Host "  outlaw models    see which model would be used, and why"
Write-Host ""
Write-Host "  Made by Outlaw Systems, in collaboration with AI."
