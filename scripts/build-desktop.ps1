# 构建产物 4/4：Tauri 2 桌面端
# （进程内启动网关 + 嵌入 Web UI，WebView 加载本地端口；数据库存放在系统应用数据目录）
# 产物：desktop\src-tauri\target\release\opengate-desktop.exe
#       desktop\src-tauri\target\release\bundle\nsis\*.exe（安装包，-NoBundle 跳过）
param(
  # 只编译可执行文件，跳过 NSIS 安装包打包
  [switch]$NoBundle
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# 1) 前端（嵌入特性在编译期读取 frontend\dist）
Set-Location (Join-Path $root 'frontend')
pnpm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pnpm build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# 2) Tauri
Set-Location (Join-Path $root 'desktop')
pnpm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($NoBundle) {
  pnpm tauri build --no-bundle
} else {
  pnpm tauri build
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ''
Write-Host "[OK] 桌面端构建完成 -> $root\desktop\src-tauri\target\release\opengate-desktop.exe" -ForegroundColor Green
if (-not $NoBundle) {
  Write-Host "     安装包位于 $root\desktop\src-tauri\target\release\bundle\nsis\"
}
