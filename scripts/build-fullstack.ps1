# 构建产物 3/4：前端嵌入后端的单文件全栈版
# （embed-frontend 特性把 frontend\dist 打进二进制，/ 直接出管理台，SPA 路由自动回退）
# 产物：backend\target\release\gateway.exe（单文件即整套产品）
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# 1) 前端
Set-Location (Join-Path $root 'frontend')
pnpm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pnpm build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# 2) 后端（嵌入 dist）
Set-Location (Join-Path $root 'backend')
cargo build --release --features embed-frontend
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ''
Write-Host "[OK] 全栈单文件构建完成 -> $root\backend\target\release\gateway.exe" -ForegroundColor Green
Write-Host '     运行后浏览器访问 http://127.0.0.1:<PORT>/ 即为管理台'
