#!/usr/bin/env pwsh
# NFR Baseline: measures RSS and startup-to-ready for AsterClaw.
# Usage: .\scripts\nfr_baseline.ps1

function Write-MinimalConfig {
    param([string]$CfgDir, [string]$WsDir, [int]$Port)
    New-Item -ItemType Directory -Path $CfgDir -Force | Out-Null
    New-Item -ItemType Directory -Path $WsDir -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $WsDir "memory") -Force | Out-Null
    Set-Content -Path (Join-Path (Join-Path $WsDir "memory") "MEMORY.md") -Value "# Memory"
    @{
        agents    = @{ defaults = @{ provider = "openai"; model = "gpt-4o-mini"; workspace = $WsDir; restrict_to_workspace = $true; max_tool_iterations = 5 } }
        providers = @{ openai = @{ api_key = "nfr-test"; api_base = "http://127.0.0.1:1" } }
        channels  = @{ telegram = @{ enabled = $false; token = "" } }
        heartbeat = @{ enabled = $false; interval = 30 }
        devices   = @{ enabled = $false; monitor_usb = $false }
        gateway   = @{ host = "127.0.0.1"; port = $Port }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $CfgDir "config.json")
}

function Measure-Binary {
    param([string]$Binary, [string]$Label, [int]$Port)

    $homeDir = Join-Path ([System.IO.Path]::GetTempPath()) "nfr_$Label"
    if (Test-Path $homeDir) { Remove-Item -Recurse -Force $homeDir }
    New-Item -ItemType Directory -Path $homeDir -Force | Out-Null

    Write-MinimalConfig -CfgDir (Join-Path $homeDir ".asterclaw") -WsDir (Join-Path $homeDir "ws") -Port $Port

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Binary
    $psi.Arguments = "gateway"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError  = $true
    $psi.CreateNoWindow = $true
    $psi.EnvironmentVariables["HOME"]           = $homeDir
    $psi.EnvironmentVariables["USERPROFILE"]     = $homeDir
    $psi.EnvironmentVariables["ASTERCLAW_HOME"]  = $homeDir

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = [System.Diagnostics.Process]::Start($psi)

    # Poll /health
    $healthUrl = "http://127.0.0.1:$Port/health"
    $deadline = (Get-Date).AddSeconds(15)
    $ready = $false
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 100
        try {
            $r = Invoke-WebRequest -Uri $healthUrl -UseBasicParsing -TimeoutSec 1 -ErrorAction SilentlyContinue
            if ($r.StatusCode -eq 200) { $ready = $true; break }
        } catch { }
    }
    $sw.Stop()

    $startupMs = $sw.ElapsedMilliseconds
    $rssKB = 0
    if (-not $proc.HasExited) {
        $proc.Refresh()
        $rssKB = [math]::Round($proc.WorkingSet64 / 1024)
        $proc.Kill()
        $proc.WaitForExit(3000) | Out-Null
    }
    $proc.Dispose()

    Remove-Item -Recurse -Force $homeDir -ErrorAction SilentlyContinue

    return @{ Label = $Label; Ready = $ready; StartupMs = $startupMs; RssKB = $rssKB }
}

# ── Main ──
Write-Host "=== AsterClaw NFR Baseline ===" -ForegroundColor Cyan

Write-Host "Building AsterClaw (release)..." -ForegroundColor Yellow
cargo build --release 2>$null
$asterclawBin = Join-Path $PSScriptRoot "..\target\release\asterclaw.exe"
if (-not (Test-Path $asterclawBin)) { Write-Host "ERROR: asterclaw.exe not found" -ForegroundColor Red; exit 1 }

$binarySize = (Get-Item $asterclawBin).Length
$fr = Measure-Binary -Binary $asterclawBin -Label "AsterClaw" -Port 19801

Write-Host ""
Write-Host "--- AsterClaw ---" -ForegroundColor Green
Write-Host "  Binary:  $([math]::Round($binarySize / 1MB, 1)) MB"
Write-Host "  Ready:   $($fr.Ready)"
Write-Host "  Startup: $($fr.StartupMs) ms"
Write-Host "  RSS:     $($fr.RssKB) KB ($([math]::Round($fr.RssKB / 1024, 1)) MB)"

Write-Host "`nDone." -ForegroundColor Cyan
