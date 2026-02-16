param(
    [string]$WorkDir = "target/nfr",
    [int]$Port = 18791,
    [int]$ReadyTimeoutSec = 45,
    [int]$Runs = 3,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Assert-CommandAvailable {
    param([string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command not found in PATH: $Name"
    }
}

function Set-TempEnvironment {
    param([hashtable]$Values)
    $previous = @{}
    foreach ($key in $Values.Keys) {
        $item = Get-Item -Path ("Env:" + $key) -ErrorAction SilentlyContinue
        if ($null -eq $item) {
            $previous[$key] = $null
        } else {
            $previous[$key] = $item.Value
        }
        [System.Environment]::SetEnvironmentVariable($key, [string]$Values[$key], "Process")
    }
    return $previous
}

function Restore-TempEnvironment {
    param([hashtable]$Previous)
    foreach ($key in $Previous.Keys) {
        $value = $Previous[$key]
        if ($null -eq $value) {
            Remove-Item -Path ("Env:" + $key) -ErrorAction SilentlyContinue
        } else {
            [System.Environment]::SetEnvironmentVariable($key, [string]$value, "Process")
        }
    }
}

function Update-GatewayConfig {
    param(
        [string]$Path,
        [int]$PortValue
    )
    if (-not (Test-Path -Path $Path)) {
        throw "Config file not found: $Path"
    }
    $raw = Get-Content -Path $Path -Raw
    $cfg = $raw | ConvertFrom-Json
    if ($null -eq $cfg.gateway) {
        throw "gateway block not found in $Path"
    }
    $cfg.gateway.host = "127.0.0.1"
    $cfg.gateway.port = $PortValue
    ($cfg | ConvertTo-Json -Depth 100) | Set-Content -Path $Path -Encoding utf8
}

function Update-PicoclawConfig {
    param(
        [string]$Path,
        [int]$PortValue
    )
    if (-not (Test-Path -Path $Path)) {
        throw "Config file not found: $Path"
    }
    $cfg = (Get-Content -Path $Path -Raw) | ConvertFrom-Json
    $cfg.gateway.host = "127.0.0.1"
    $cfg.gateway.port = $PortValue
    $cfg.agents.defaults.model = "gpt-4o-mini"
    $cfg.agents.defaults.provider = "openai"
    $cfg.providers.openai.api_key = "nfr-dummy-key"
    $cfg.providers.openai.api_base = "https://api.openai.com/v1"
    $cfg.channels.telegram.enabled = $false
    $cfg.heartbeat.enabled = $false
    ($cfg | ConvertTo-Json -Depth 100) | Set-Content -Path $Path -Encoding utf8
}

function New-MinPicorsConfig {
    param(
        [string]$Path,
        [string]$WorkspaceRoot
    )
    $parent = Split-Path -Path $Path -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $workspace = Join-Path $WorkspaceRoot "picors-workspace"
    New-Item -ItemType Directory -Path $workspace -Force | Out-Null
    $cfg = @{
        gateway = @{
            host = "127.0.0.1"
            port = 18791
        }
        channels = @{
            telegram = @{
                enabled = $false
                token = ""
                allow_from = @()
            }
        }
        heartbeat = @{
            enabled = $false
            interval = 30
        }
        devices = @{
            enabled = $false
            monitor_usb = $false
        }
        agents = @{
            defaults = @{
                model = "gpt-4o-mini"
                provider = "openai"
                workspace = $workspace
                max_tokens = 8192
                max_tool_iterations = 10
                restrict_to_workspace = $true
            }
        }
        providers = @{
            openai = @{
                api_key = "nfr-dummy-key"
                api_base = "https://api.openai.com/v1"
            }
            openrouter = @{
                api_key = ""
                api_base = "https://openrouter.ai/api/v1"
            }
            groq = @{
                api_key = ""
                api_base = "https://api.groq.com/openai/v1"
            }
            zhipu = @{
                api_key = ""
                api_base = "https://open.bigmodel.cn/api/paas/v4"
            }
            deepseek = @{
                api_key = ""
                api_base = "https://api.deepseek.com/v1"
            }
            anthropic = @{
                api_key = ""
                api_base = "https://api.anthropic.com/v1"
            }
        }
        web_search = @{
            enabled = $false
            provider = "duckduckgo"
            api_key = ""
            max_results = 5
        }
        workspace = @{
            path = $workspace
        }
    }
    ($cfg | ConvertTo-Json -Depth 100) | Set-Content -Path $Path -Encoding utf8
}

function Wait-Ready {
    param(
        [int]$PortValue,
        [int]$TimeoutSec
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        try {
            $resp = Invoke-WebRequest -Uri ("http://127.0.0.1:{0}/ready" -f $PortValue) -UseBasicParsing -TimeoutSec 1
            if ($resp.StatusCode -eq 200) {
                return $true
            }
        } catch {
            # not ready yet
        }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

function Measure-Gateway {
    param(
        [string]$Name,
        [string]$ExePath,
        [string]$LogPath,
        [int]$PortValue,
        [int]$TimeoutSec
    )

    $stdoutLog = "$LogPath.stdout.log"
    $stderrLog = "$LogPath.stderr.log"
    $proc = Start-Process -FilePath $ExePath -ArgumentList "gateway" -PassThru -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $ready = Wait-Ready -PortValue $PortValue -TimeoutSec $TimeoutSec
        if (-not $ready) {
            throw "$Name did not become ready within $TimeoutSec seconds. See log: $LogPath"
        }
        $sw.Stop()
        $ps = Get-Process -Id $proc.Id -ErrorAction Stop
        return @{
            name = $Name
            startup_ms = [int]$sw.ElapsedMilliseconds
            rss_bytes = [int64]$ps.WorkingSet64
            pid = $proc.Id
        }
    } finally {
        if (-not $proc.HasExited) {
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        }
        Start-Sleep -Milliseconds 200
    }
}

function Measure-GatewayStable {
    param(
        [string]$Name,
        [string]$ExePath,
        [string]$LogPath,
        [int]$PortValue,
        [int]$TimeoutSec,
        [int]$RunsCount
    )
    if ($RunsCount -lt 1) {
        $RunsCount = 1
    }
    $samples = @()
    # Warmup
    $null = Measure-Gateway -Name "$Name-warmup" -ExePath $ExePath -LogPath "$LogPath.warmup" -PortValue $PortValue -TimeoutSec $TimeoutSec
    for ($i = 1; $i -le $RunsCount; $i++) {
        $samples += Measure-Gateway -Name $Name -ExePath $ExePath -LogPath "$LogPath.run$i" -PortValue $PortValue -TimeoutSec $TimeoutSec
    }
    $avgStartup = [int]([Math]::Round((($samples | Measure-Object -Property startup_ms -Average).Average), 0))
    $avgRss = [int64]([Math]::Round((($samples | Measure-Object -Property rss_bytes -Average).Average), 0))
    return @{
        name = $Name
        startup_ms = $avgStartup
        rss_bytes = $avgRss
        runs = $samples
    }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$workRoot = (Resolve-Path $repoRoot).Path
$nfrRoot = Join-Path $workRoot $WorkDir
$homeDir = Join-Path $nfrRoot "home"
$logsDir = Join-Path $nfrRoot "logs"

New-Item -ItemType Directory -Path $nfrRoot -Force | Out-Null
New-Item -ItemType Directory -Path $logsDir -Force | Out-Null
New-Item -ItemType Directory -Path $homeDir -Force | Out-Null

$picorsBin = Join-Path $workRoot "target/release/picors.exe"
$picoclawBin = Join-Path $nfrRoot "picoclaw.exe"

if (-not $SkipBuild) {
    Assert-CommandAvailable -Name "cargo"
    Assert-CommandAvailable -Name "go"

    Write-Host "[build] cargo build --release"
    & cargo build --release
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed"
    }

    $goRepo = Join-Path $workRoot "references/picoclaw"
    if (-not (Test-Path -Path $goRepo)) {
        throw "Go reference repo not found: $goRepo"
    }
    $goWorkspaceSrc = Join-Path $goRepo "workspace"
    $goWorkspaceDst = Join-Path $goRepo "cmd/picoclaw/workspace"
    if (Test-Path -Path $goWorkspaceSrc) {
        Remove-Item -Path $goWorkspaceDst -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Path (Split-Path -Path $goWorkspaceDst -Parent) -Force | Out-Null
        Copy-Item -Path $goWorkspaceSrc -Destination $goWorkspaceDst -Recurse -Force
    }
    Write-Host "[build] go build ./cmd/picoclaw"
    Push-Location $goRepo
    try {
        & go build -o $picoclawBin ./cmd/picoclaw
        if ($LASTEXITCODE -ne 0) {
            throw "go build failed"
        }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path -Path $picorsBin)) {
    throw "picors binary not found: $picorsBin"
}
if (-not (Test-Path -Path $picoclawBin)) {
    throw "picoclaw baseline binary not found: $picoclawBin"
}

$envPatch = @{
    HOME = $homeDir
    USERPROFILE = $homeDir
    PICORS_HOME = $homeDir
    OPENROUTER_API_KEY = "nfr-dummy-key"
    OPENAI_API_KEY = "nfr-dummy-key"
}
$previousEnv = Set-TempEnvironment -Values $envPatch

try {
    Remove-Item -Path (Join-Path $homeDir ".picors") -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Path (Join-Path $homeDir ".picoclaw") -Recurse -Force -ErrorAction SilentlyContinue

    New-Item -ItemType Directory -Path (Join-Path $homeDir ".picoclaw") -Force | Out-Null
    New-MinPicorsConfig -Path (Join-Path $homeDir ".picors/config.json") -WorkspaceRoot $nfrRoot

    Write-Host "[setup] onboarding picoclaw"
    & $picoclawBin onboard | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "picoclaw onboard failed"
    }

    Update-GatewayConfig -Path (Join-Path $homeDir ".picors/config.json") -PortValue $Port
    Update-PicoclawConfig -Path (Join-Path $homeDir ".picoclaw/config.json") -PortValue $Port

    Write-Host "[measure] picoclaw baseline"
    $picoclaw = Measure-GatewayStable `
        -Name "picoclaw" `
        -ExePath $picoclawBin `
        -LogPath (Join-Path $logsDir "picoclaw.log") `
        -PortValue $Port `
        -TimeoutSec $ReadyTimeoutSec `
        -RunsCount $Runs

    Write-Host "[measure] picors candidate"
    $picors = Measure-GatewayStable `
        -Name "picors" `
        -ExePath $picorsBin `
        -LogPath (Join-Path $logsDir "picors.log") `
        -PortValue $Port `
        -TimeoutSec $ReadyTimeoutSec `
        -RunsCount $Runs

    $rssRatio = [double]$picors.rss_bytes / [double]$picoclaw.rss_bytes
    $startupRatio = [double]$picors.startup_ms / [double]$picoclaw.startup_ms

    $result = @{
        picoclaw = $picoclaw
        picors = $picors
        ratio = @{
            rss = [Math]::Round($rssRatio, 4)
            startup = [Math]::Round($startupRatio, 4)
        }
        gates = @{
            rss_lte_1_05 = ($rssRatio -le 1.05)
            startup_lte_1_10 = ($startupRatio -le 1.10)
        }
        timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
    }

    $resultPath = Join-Path $nfrRoot "nfr-results.json"
    ($result | ConvertTo-Json -Depth 100) | Set-Content -Path $resultPath -Encoding utf8

    Write-Host ""
    Write-Host "NFR Comparison Results"
    Write-Host ("  picoclaw startup_ms: {0}" -f $picoclaw.startup_ms)
    Write-Host ("  picors   startup_ms: {0}" -f $picors.startup_ms)
    Write-Host ("  startup ratio      : {0}" -f ([Math]::Round($startupRatio, 4)))
    Write-Host ("  picoclaw rss_bytes : {0}" -f $picoclaw.rss_bytes)
    Write-Host ("  picors   rss_bytes : {0}" -f $picors.rss_bytes)
    Write-Host ("  rss ratio          : {0}" -f ([Math]::Round($rssRatio, 4)))
    Write-Host ("  gate startup<=1.10 : {0}" -f $result.gates.startup_lte_1_10)
    Write-Host ("  gate rss<=1.05     : {0}" -f $result.gates.rss_lte_1_05)
    Write-Host ("  report             : {0}" -f $resultPath)
} finally {
    Restore-TempEnvironment -Previous $previousEnv
}
