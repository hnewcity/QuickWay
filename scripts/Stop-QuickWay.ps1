$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$pidPath = Join-Path $projectRoot '.quickway\proxy.pid'
if (-not (Test-Path -LiteralPath $pidPath)) { Write-Host 'No QuickWay process started by the launcher was recorded.'; return }
$proxyPid = [int](Get-Content -Raw -LiteralPath $pidPath)
$process = Get-Process -Id $proxyPid -ErrorAction SilentlyContinue
$expectedPath = [System.IO.Path]::GetFullPath((Join-Path $projectRoot 'target\release\quickway.exe'))
if ($process) {
    if ($process.Path -ne $expectedPath) { throw 'Recorded PID belongs to another executable; refusing to stop it.' }
    Stop-Process -Id $proxyPid
}
Remove-Item -LiteralPath $pidPath
Write-Host 'QuickWay stopped.'
