#!/usr/bin/env node
// 构建产物 4/4：Tauri 2 桌面端
// (进程内启动网关 + 嵌入 Web UI，WebView 加载本地端口；数据库存放在系统应用数据目录)
// 产物：desktop\src-tauri\target\release\opengate-desktop.exe
//       desktop\src-tauri\target\release\bundle\nsis\*.exe（安装包，--no-bundle 跳过）
const path = require('path');
const { run, ok } = require('./_run');

// --no-bundle：只编译可执行文件，跳过 NSIS 安装包打包
const noBundle = process.argv.includes('--no-bundle');

// 1) 前端（嵌入特性在编译期读取 frontend\dist）
run('pnpm', ['install'], { cwd: 'frontend' });
run('pnpm', ['build'], { cwd: 'frontend' });

// 2) Tauri
run('pnpm', ['install'], { cwd: 'desktop' });
const tauriArgs = ['tauri', 'build'];
if (noBundle) tauriArgs.push('--no-bundle');
run('pnpm', tauriArgs, { cwd: 'desktop' });

ok(`桌面端构建完成 -> desktop${path.sep}src-tauri${path.sep}target${path.sep}release${path.sep}opengate-desktop.exe`);
if (!noBundle) {
  console.log(`     安装包位于 desktop${path.sep}src-tauri${path.sep}target${path.sep}release${path.sep}bundle${path.sep}nsis${path.sep}`);
}