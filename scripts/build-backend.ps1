# 构建产物 2/4：纯后端（仅 API 网关，不含 Web UI）
# 产物：backend\target\release\gateway.exe
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

Set-Location (Join-Path $root 'backend')
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ''
Write-Host "[OK] 纯后端构建完成 -> $root\backend\target\release\gateway.exe" -ForegroundColor Green
