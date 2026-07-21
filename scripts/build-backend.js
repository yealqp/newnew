#!/usr/bin/env node
// 构建产物 2/4：纯后端（仅 API 网关，不含 Web UI）
// 产物：backend\target\release\gateway.exe
const path = require('path');
const { run, ok } = require('./_run');

run('cargo', ['build', '--release'], { cwd: 'backend' });

ok(`纯后端构建完成 -> backend${path.sep}target${path.sep}release${path.sep}gateway.exe`);