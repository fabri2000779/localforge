<#
.SYNOPSIS
LocalForge agent installer for Windows / Windows Server.

.DESCRIPTION
Downloads the matching localforge-agent.exe, generates a bearer token
and self-signed TLS cert, writes a 0600-ish ACL'd config file, and
registers the agent to auto-start at boot (via Task Scheduler by
default, or as a Windows Service when -InstallAsService is passed).

Run from an elevated PowerShell prompt.

.EXAMPLE
# One-liner: download + run with defaults.
iex "& { $(irm https://github.com/fabri2000779/localforge/releases/latest/download/install-agent.ps1) }"

.EXAMPLE
# With a custom domain (skips public-IP detection).
$env:LOCALFORGE_AGENT_DOMAIN = "node.example.com"
iex "& { $(irm https://github.com/fabri2000779/localforge/releases/latest/download/install-agent.ps1) }"

.NOTES
Environment variable overrides (mirror install-agent.sh):

  LOCALFORGE_AGENT_VERSION   release tag (default: latest)
  LOCALFORGE_AGENT_BIND      bind interface     (default: 0.0.0.0)
  LOCALFORGE_AGENT_PORT      TCP port           (default: 7878)
  LOCALFORGE_AGENT_DATA_ROOT data directory     (default: C:\ProgramData\LocalForge)
  LOCALFORGE_AGENT_DOMAIN    surfaced in the pairing summary
  LOCALFORGE_AGENT_LABEL     friendly node label printed in summary

#>
[CmdletBinding()]
param(
    # Register as a Windows Service via sc.exe instead of a scheduled
    # task. Useful on Windows Server. Note: a plain console exe doesn't
    # speak the Service Control Manager protocol, so `sc start` will
    # time out — wrap with NSSM if you need a "real" service.
    [switch]$InstallAsService
)

$ErrorActionPreference = 'Stop'
$Repo = 'fabri2000779/localforge'

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function Write-Step($msg) { Write-Host "▸ $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "✓ $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "! $msg" -ForegroundColor Yellow }
function Write-Fail($msg) { Write-Host "✗ $msg" -ForegroundColor Red; throw $msg }

function Require-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($id)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Write-Fail "This installer needs administrator. Right-click PowerShell → Run as Administrator and try again."
    }
}

function Detect-Arch {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($arch -ne 'X64') {
        Write-Fail "Unsupported architecture: $arch. The Windows agent build currently targets x86_64 only."
    }
    Write-Ok "Architecture: x86_64-pc-windows-msvc"
}

function Ensure-Docker {
    $docker = Get-Command docker -ErrorAction SilentlyContinue
    if (-not $docker) {
        Write-Warn "Docker is not on PATH."
        Write-Host @"
Install Docker Desktop (or Docker Engine on Windows Server) before
running this script:

  https://www.docker.com/products/docker-desktop/
  (or for Windows Server: https://learn.microsoft.com/en-us/virtualization/windowscontainers/quick-start/set-up-environment)

After installing, re-run this script.
"@ -ForegroundColor Yellow
        throw "Docker missing."
    }
    try {
        docker info *> $null
    } catch {
        Write-Warn "Docker binary present but the daemon isn't responding. Start Docker Desktop (or the docker service) and re-run."
        throw "Docker daemon unreachable."
    }
    Write-Ok "Docker is reachable."
}

function Resolve-Version {
    $version = $env:LOCALFORGE_AGENT_VERSION
    if (-not $version) {
        Write-Step "Resolving latest release tag..."
        $resp = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
        $version = $resp.tag_name
    }
    if (-not $version) {
        Write-Fail "Could not determine the latest LocalForge agent version."
    }
    Write-Ok "Release: $version"
    return $version
}

function Download-Binary($version, $destExe) {
    $url = "https://github.com/$Repo/releases/download/$version/localforge-agent-x86_64-pc-windows-msvc.exe"
    Write-Step "Downloading $url"
    $tmp = "$destExe.tmp"
    Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
    Move-Item -Force $tmp $destExe
    Unblock-File $destExe -ErrorAction SilentlyContinue
    Write-Ok "Installed to $destExe"
}

function Restrict-Acl($path) {
    # Strip inheritance, allow SYSTEM + Administrators read-only. The
    # service runs as LocalSystem so this is sufficient.
    icacls.exe $path /inheritance:r *> $null
    icacls.exe $path /grant:r "SYSTEM:(R)" "Administrators:(R)" *> $null
}

function Detect-PublicAddress {
    if ($env:LOCALFORGE_AGENT_DOMAIN) {
        return $env:LOCALFORGE_AGENT_DOMAIN
    }
    $services = @(
        'https://api.ipify.org',
        'https://ifconfig.me/ip',
        'https://icanhazip.com',
        'https://checkip.amazonaws.com'
    )
    foreach ($svc in $services) {
        try {
            $ip = (Invoke-WebRequest -Uri $svc -UseBasicParsing -TimeoutSec 3).Content.Trim()
            if ($ip) { return $ip }
        } catch {}
    }
    return '<server-ip>'
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

Require-Admin
Detect-Arch
Ensure-Docker
$Version = Resolve-Version

$DataRoot   = if ($env:LOCALFORGE_AGENT_DATA_ROOT) { $env:LOCALFORGE_AGENT_DATA_ROOT } else { 'C:\ProgramData\LocalForge' }
$Bind       = if ($env:LOCALFORGE_AGENT_BIND) { $env:LOCALFORGE_AGENT_BIND } else { '0.0.0.0' }
$Port       = if ($env:LOCALFORGE_AGENT_PORT) { [int]$env:LOCALFORGE_AGENT_PORT } else { 7878 }
$Label      = if ($env:LOCALFORGE_AGENT_LABEL) { $env:LOCALFORGE_AGENT_LABEL } else { $env:COMPUTERNAME }
$InstallDir = 'C:\Program Files\LocalForge'
$ConfigDir  = Join-Path $DataRoot 'config'
$ConfigPath = Join-Path $ConfigDir 'agent.toml'
$Exe        = Join-Path $InstallDir 'localforge-agent.exe'

New-Item -ItemType Directory -Force -Path $InstallDir, $DataRoot, $ConfigDir | Out-Null

Download-Binary $Version $Exe

if (Test-Path $ConfigPath) {
    Write-Warn "Existing config at $ConfigPath — keeping it as-is (delete it to regenerate)."
    $pairingOutput = ''
} else {
    Write-Step "Generating bearer token and self-signed TLS cert..."
    $pairingOutput = & $Exe install `
        --config-path $ConfigPath `
        --data-root   $DataRoot `
        --bind        $Bind `
        --port        $Port
    if ($LASTEXITCODE -ne 0) {
        Write-Fail "Agent install failed with exit code $LASTEXITCODE."
    }
    Restrict-Acl $ConfigPath
    Write-Ok "Config written to $ConfigPath (ACL restricted to SYSTEM + Administrators)."
}

# Extract pairing values from the install output (or skip if the config
# already existed — the user can re-read the .toml then).
$token = $null
$fingerprint = $null
if ($pairingOutput) {
    $tokenMatch = [regex]::Match($pairingOutput, 'lf_agent_[0-9a-f]+')
    if ($tokenMatch.Success) { $token = $tokenMatch.Value }
    $fpMatch = [regex]::Match($pairingOutput, 'SHA256:[0-9A-F:]+')
    if ($fpMatch.Success) { $fingerprint = $fpMatch.Value }
}

# ---------------------------------------------------------------------------
# Register auto-start (scheduled task by default, service if requested)
# ---------------------------------------------------------------------------

# NB: don't use $args — it's a PowerShell automatic variable.
$taskArgs = "--config `"$ConfigPath`" serve"
$cmd      = "`"$Exe`" --config `"$ConfigPath`" serve"

if ($InstallAsService) {
    Write-Step "Registering Windows Service 'LocalForgeAgent' via sc.exe..."
    sc.exe stop   LocalForgeAgent *> $null
    sc.exe delete LocalForgeAgent *> $null
    $createRc = (sc.exe create LocalForgeAgent binPath= $cmd start= auto DisplayName= "LocalForge Agent" 2>&1) -join "`n"
    Write-Host $createRc
    sc.exe description LocalForgeAgent "Remote game-server agent for LocalForge." *> $null
    Write-Warn "Note: the agent binary is a console app, not a native Windows Service. ``sc start`` may time out; wrap with NSSM (https://nssm.cc) for production."
    sc.exe start LocalForgeAgent *> $null
    Write-Ok "Service registered (start manually if it didn't come up — see warning above)."
} else {
    Write-Step "Registering scheduled task 'LocalForgeAgent' (runs at boot as SYSTEM)..."
    Unregister-ScheduledTask -TaskName LocalForgeAgent -Confirm:$false -ErrorAction SilentlyContinue
    $action    = New-ScheduledTaskAction    -Execute $Exe -Argument $taskArgs
    $trigger   = New-ScheduledTaskTrigger   -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest -LogonType ServiceAccount
    $settings  = New-ScheduledTaskSettingsSet `
                    -StartWhenAvailable `
                    -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
                    -ExecutionTimeLimit ([TimeSpan]::Zero) `
                    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
    Register-ScheduledTask `
        -TaskName    LocalForgeAgent `
        -Description 'Remote game-server agent for LocalForge.' `
        -Action      $action `
        -Trigger     $trigger `
        -Principal   $principal `
        -Settings    $settings | Out-Null
    Start-ScheduledTask -TaskName LocalForgeAgent
    Write-Ok "Task registered and started."
}

# ---------------------------------------------------------------------------
# Pairing summary
# ---------------------------------------------------------------------------

$publicHost = Detect-PublicAddress

@"

────────────────────────────────────────────────────────────────────────
  ✓ LocalForge agent installed and running on this host.
────────────────────────────────────────────────────────────────────────

  Paste these three values into the desktop's "Add Node" form:

    Label:        $Label
    URL:          https://$publicHost`:$Port
    Token:        $(if ($token) { $token } else { "<see $ConfigPath>" })
    Fingerprint:  $(if ($fingerprint) { $fingerprint } else { "<see $ConfigPath>" })

  Status:  Get-ScheduledTaskInfo -TaskName LocalForgeAgent
  Stop:    Stop-ScheduledTask     -TaskName LocalForgeAgent
  Logs:    Event Viewer → Windows Logs → Application (filter source: localforge-agent)

  ── Control it from your phone (or another desktop), with this one OFF ──
  On a paid plan (Hobby/Team), enroll this agent on the LocalForge cloud
  relay so it's reachable without your main desktop running:
    1. In the LocalForge desktop app, open Nodes → "Link to cloud".
    2. Copy the one-line code it shows, then run it here:
         localforge-agent link <code>
  The agent dials out to the cloud — no inbound ports. On a free/standalone
  setup, the "Add Node" pairing above is all you need.

  Override the public hostname in pairing output:
    `$env:LOCALFORGE_AGENT_DOMAIN = 'node.example.com'

"@ | Write-Host
