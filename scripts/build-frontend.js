#!/usr/bin/env node
// 构建产物 1/4：纯前端（静态站点，可交给任意静态服务器 / CDN）
// 产物：frontend\dist\
const path = require('path');
const { run, ok } = require('./_run');

run('pnpm', ['install'], { cwd: 'frontend' });
run('pnpm', ['build'], { cwd: 'frontend' });

ok(`纯前端构建完成 -> frontend${path.sep}dist`);