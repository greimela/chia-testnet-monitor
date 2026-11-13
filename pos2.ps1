<#
Control script for PoS2 testnet testing. Always runs the hardfork-testnet build in .\chia-blockchain
against the testnet root in network.txt, never against ~/.chia/mainnet.

  .\pos2.ps1 use <network_name> [fingerprint]   pick the testnet (CHIA_ROOT = ~/.chia/<network_name>) and key
  .\pos2.ps1 start                  start farmer (node, farmer, harvester, wallet), the solver and the dashboard
  .\pos2.ps1 stop                   stop all of them
  .\pos2.ps1 status                 node, farm and plot summary
  .\pos2.ps1 dashboard              open the testnet monitor (http://127.0.0.1:8899), starting it if needed
  .\pos2.ps1 plot1 [chia args]      create v1 plots in .\plots\v1 (default -k 25 -n 1), e.g. plot1 -n 4
  .\pos2.ps1 plot2 [chia args]      create v2 plots in .\plots\v2 (k28, -s 2, -n 1), e.g. plot2 -n 6
  .\pos2.ps1 plotting [watch]       finished and in-progress plots ("watch" refreshes every 5 s)
  .\pos2.ps1 log                    follow debug.log
  .\pos2.ps1 chia <args...>         run any chia command against this testnet
#>
# No param() block: PowerShell would swallow dash flags (-n, -i, ...) meant for chia.
$Command = if ($args.Count) { [string]$args[0] } else { "status" }
$Rest = @(if ($args.Count -gt 1) { $args[1..($args.Count - 1)] })
$ErrorActionPreference = "Continue"  # "Stop" turns chia's log output on stderr into a fatal error in PowerShell 5.1
$Here = $PSScriptRoot
$Bin = Join-Path $Here "chia-blockchain\.venv\Scripts"
$Chia = Join-Path $Bin "chia.cmd"
$Python = Join-Path $Bin "python.exe"
$NetFile = Join-Path $Here "network.txt"
$KeyFile = Join-Path $Here "fingerprint.txt"

if ($Command -eq "use") {
    if (-not $Rest -or $Rest[0] -eq "mainnet") { throw "Usage: .\pos2.ps1 use <testnet network name> (not mainnet)" }
    Set-Content -Path $NetFile -Value $Rest[0] -Encoding ascii
    if ($Rest.Count -gt 1) { Set-Content -Path $KeyFile -Value $Rest[1] -Encoding ascii }
    Write-Host "Now using $($Rest[0]) at ~/.chia/$($Rest[0])"
    exit 0
}
if (-not (Test-Path $Chia)) { throw "chia isn't installed in chia-blockchain\.venv yet. Run chia-blockchain\Install.ps1." }
if (-not (Test-Path $NetFile)) { throw "No testnet selected. Run: .\pos2.ps1 use <network_name>" }
$Network = (Get-Content $NetFile -TotalCount 1).Trim()
$env:CHIA_ROOT = Join-Path $HOME ".chia\$Network"
$env:PATH = "$Bin;$env:PATH"  # the daemon finds chia_full_node etc. on PATH, as an activated venv would
$env:PYTHONUNBUFFERED = "1"   # so plot progress shows up in redirected logs as it happens
$DashUrl = "http://127.0.0.1:8899"
$Background = @{ WindowStyle = "Hidden"; WorkingDirectory = $Here }  # no console windows on the taskbar

# The solver and dashboard run hidden, so find them by command line to stop them.
function Get-Helper($pattern) {
    Get-CimInstance Win32_Process -Filter "Name = 'python.exe' OR Name = 'cmd.exe'" | Where-Object { $_.CommandLine -like $pattern }
}
function Start-Dashboard {
    if (Get-Helper "*$Here\server.py*") { return }
    Start-Process -FilePath $Python -ArgumentList "`"$(Join-Path $Here 'server.py')`"", "--root", "`"$env:CHIA_ROOT`"" `
        -RedirectStandardOutput (Join-Path $Here "server.log") -RedirectStandardError (Join-Path $Here "server.err.log") @Background
}
$KeyArgs = @(if (Test-Path $KeyFile) { "-a", (Get-Content $KeyFile -TotalCount 1).Trim() })

switch ($Command) {
    "start" {
        & $Chia start farmer
        if (-not (Get-Helper "*$Bin*chia_solver*")) { Start-Process -FilePath (Join-Path $Bin "chia_solver.cmd") @Background }
        Start-Dashboard
        Start-Sleep -Seconds 5
        & $Chia rpc farmer connect_to_solver '{\"host\": \"localhost\", \"port\": 8666}'
        Write-Host "Dashboard: $DashUrl"
    }
    "stop" {
        & $Chia stop all -d
        Get-Helper "*$Bin*chia_solver*" | ForEach-Object { Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }
        Get-Helper "*$Here\server.py*" | ForEach-Object { Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }
    }
    "status" {
        Write-Host "Network $Network, CHIA_ROOT $env:CHIA_ROOT"
        & $Chia show -s
        & $Chia farm summary
    }
    "dashboard" { Start-Dashboard; Start-Sleep -Seconds 2; Start-Process $DashUrl }
    "plot1" {
        $dir = Join-Path $Here "plots\v1"
        New-Item -ItemType Directory -Force $dir | Out-Null
        & $Chia plots add -d $dir | Out-Null  # farm each plot as soon as it's done, not after the whole batch
        $size = if ($Rest -contains "-k") { @() } else { @("-k", "25") }
        & $Chia plots create @size --override-k @KeyArgs -t $dir -d $dir @Rest
    }
    "plot2" {
        $dir = Join-Path $Here "plots\v2"
        New-Item -ItemType Directory -Force $dir | Out-Null
        & $Chia plots add -d $dir | Out-Null  # farm each plot as soon as it's done, not after the whole batch
        & $Chia plots create2 @KeyArgs -d $dir @Rest
    }
    "plotting" {
        do {
            if ($Rest -contains "watch") { Clear-Host }
            foreach ($v in "v1", "v2") {
                $dir = Join-Path $Here "plots\$v"
                $files = @(Get-ChildItem $dir -File -ErrorAction SilentlyContinue)
                $done = @($files | Where-Object { $_.Extension -ne ".tmp" })
                $gb = ($done | Measure-Object Length -Sum).Sum / 1GB
                Write-Host ("{0}: {1} finished ({2:N1} GB)" -f $v, $done.Count, $gb)
                $files | Where-Object { $_.Extension -eq ".tmp" } | Group-Object { $_.Name -replace "\.plot.*", "" } | ForEach-Object {
                    $newest = $_.Group | Sort-Object LastWriteTime | Select-Object -Last 1
                    $phase = if ($newest.Name -match "\.p(\d)\.") { "phase $($Matches[1]) of 4" } else { "in progress" }
                    Write-Host ("    plotting {0}..., {1}, last write {2:HH:mm:ss}" -f $_.Name.Substring(0, [Math]::Min(30, $_.Name.Length)), $phase, $newest.LastWriteTime)
                }
            }
            if ($Rest -contains "watch") { Start-Sleep -Seconds 5 }
        } while ($Rest -contains "watch")
    }
    "log" { Get-Content (Join-Path $env:CHIA_ROOT "log\debug.log") -Tail 40 -Wait }
    "chia" { & $Chia @Rest }
    default { Get-Help $PSCommandPath -Detailed; exit 1 }
}
