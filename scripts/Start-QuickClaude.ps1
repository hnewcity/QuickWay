param(
    [int]$Port = 8741,
    [string]$Model = '',
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$ClaudeArgs
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot 'Start-QuickWay.ps1') -Port $Port
$key = (Get-Content -Raw -LiteralPath (Join-Path $projectRoot '.quickway\proxy.key')).Trim()
$catalog = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/v1/models" -Headers @{ 'x-api-key' = $key }
$modelIds = @($catalog.data | ForEach-Object { $_.id })
if ($modelIds.Count -eq 0) {
    throw "Quick 云端模型目录为空，请确认登录并刷新目录。$($catalog.quickway_catalog.error)"
}
if ([string]::IsNullOrWhiteSpace($Model)) {
    $modelIds | ForEach-Object { Write-Host $_ }
    $Model = Read-Host '从上面的云端目录输入模型 ID'
}
if ($modelIds -cnotcontains $Model) {
    throw "模型 '$Model' 不在当前 Quick 云端目录中。可选模型：$($modelIds -join ', ')"
}
$variables = @{
    ANTHROPIC_BASE_URL = "http://127.0.0.1:$Port"
    ANTHROPIC_AUTH_TOKEN = $key
    ANTHROPIC_DEFAULT_OPUS_MODEL = $Model
    ANTHROPIC_DEFAULT_SONNET_MODEL = $Model
    ANTHROPIC_DEFAULT_HAIKU_MODEL = $Model
    ANTHROPIC_SMALL_FAST_MODEL = $Model
    CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS = '1'
    ENABLE_TOOL_SEARCH = 'false'
}
$saved = @{}
$connectionPath = Join-Path $projectRoot '.quickway\claude-connection.json'
@{ env = $variables } | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $connectionPath -Encoding UTF8
try {
    foreach ($name in $variables.Keys) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $variables[$name], 'Process')
    }
    # With both an API key and the QuickWay token set, Claude Code warns and may
    # prefer the key. Hide any user key from this Claude Code process only.
    $saved['ANTHROPIC_API_KEY'] = [Environment]::GetEnvironmentVariable('ANTHROPIC_API_KEY', 'Process')
    Remove-Item -LiteralPath 'Env:ANTHROPIC_API_KEY' -ErrorAction SilentlyContinue
    & claude --settings $connectionPath --model $Model @ClaudeArgs
    $claudeExitCode = $LASTEXITCODE
} finally {
    foreach ($name in $saved.Keys) {
        if ($null -eq $saved[$name]) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
        }
    }
}
if ($claudeExitCode -ne 0) { Write-Error "Claude Code exited with code $claudeExitCode" }
