#!/usr/bin/env node
// 构建产物 3/4：前端嵌入后端的单文件全栈版
// (embed-frontend 特性把 frontend\dist 打进二进制，/ 直接出管理台，SPA 路由自动回退)
// 产物：backend\target\release\gateway.exe（单文件即整套产品）
const path = require('path');
const { run, ok } = require('./_run');

// 1) 前端
run('pnpm', ['install'], { cwd: 'frontend' });
run('pnpm', ['build'], { cwd: 'frontend' });

// 2) 后端（嵌入 dist）
run('cargo', ['build', '--release', '--features', 'embed-frontend'], { cwd: 'backend' });

ok(`全栈单文件构建完成 -> backend${path.sep}target${path.sep}release${path.sep}gateway.exe`);
console.log('     运行后浏览器访问 http://127.0.0.1:<PORT>/ 即为管理台');