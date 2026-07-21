# 构建产物 1/4：纯前端（静态站点，可交给任意静态服务器 / CDN）
# 产物：frontend\dist\
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

Set-Location (Join-Path $root 'frontend')
pnpm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pnpm build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ''
Write-Host "[OK] 纯前端构建完成 -> $root\frontend\dist" -ForegroundColor Green
