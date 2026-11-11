param([int]$Port = 8741)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$exePath = Join-Path $projectRoot 'target\release\quickway.exe'
$statePath = Join-Path $projectRoot '.quickway'
$keyPath = Join-Path $statePath 'proxy.key'
if (-not (Test-Path -LiteralPath $exePath)) {
    Push-Location $projectRoot
    try {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'QuickWay build failed.' }
    } finally { Pop-Location }
}
try {
    $health = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/health" -TimeoutSec 2
    if ($health.service -ne 'quickway') { throw "Port $Port is occupied by another service." }
    $key = (Get-Content -Raw -LiteralPath $keyPath).Trim()
    $null = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/v1/models" -Headers @{ 'x-api-key' = $key } -TimeoutSec 2
    Write-Host "QuickWay is ready at http://127.0.0.1:$Port"
    return
} catch {
    # A listening port must never be mistaken for our authenticated instance.
    $socket = [System.Net.Sockets.TcpClient]::new()
    try {
        $task = $socket.ConnectAsync('127.0.0.1', $Port)
        if ($task.Wait(1000) -and $socket.Connected) { throw "Port $Port is occupied or its QuickWay key differs. Stop that instance or choose another port." }
    } catch [System.Management.Automation.MethodInvocationException] {
        # Connection refused: safe to start a new listener.
    } finally { $socket.Dispose() }
}
New-Item -ItemType Directory -Path $statePath -Force | Out-Null
$process = Start-Process -FilePath $exePath -ArgumentList @('serve', '--listen', "127.0.0.1:$Port") -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $statePath 'proxy.stdout.log') -RedirectStandardError (Join-Path $statePath 'proxy.stderr.log')
for ($attempt = 0; $attempt -lt 40; $attempt++) {
    if ($process.HasExited) { throw "QuickWay exited. See $statePath\proxy.stderr.log" }
    Start-Sleep -Milliseconds 250
    try {
        $health = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/health" -TimeoutSec 1
        if ($health.service -eq 'quickway') {
            $key = (Get-Content -Raw -LiteralPath $keyPath).Trim()
            $null = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/v1/models" -Headers @{ 'x-api-key' = $key } -TimeoutSec 1
            Set-Content -LiteralPath (Join-Path $statePath 'proxy.pid') -Value $process.Id
            Write-Host "QuickWay started at http://127.0.0.1:$Port"
            return
        }
    } catch { }
}
throw "QuickWay did not become ready. See $statePath\proxy.stderr.log"
