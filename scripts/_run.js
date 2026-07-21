// 共享工具：在所有 build-*.js 之间复用子进程执行 / 路径 / 着色输出。
const { spawnSync } = require('child_process');
const path = require('path');

// 项目根目录 = scripts 的上一级
const root = path.resolve(__dirname, '..');

const GREEN = '\x1b[32m';
const CYAN = '\x1b[36m';
const RED = '\x1b[31m';
const RESET = '\x1b[0m';

// 同步运行一条命令，失败则打印并退出（等价于 ps1 的 $ErrorActionPreference='Stop' + 检查 $LASTEXITCODE）。
function run(cmd, args, opts = {}) {
  const cwd = opts.cwd ? path.join(root, opts.cwd) : root;
  console.log(`${CYAN}>${RESET} ${cmd} ${args.join(' ')}  ${path.relative(root, cwd)}`);
  const result = spawnSync(cmd, args, { cwd, stdio: 'inherit', shell: true });
  if (result.status !== 0) {
    console.error(`${RED}[FAIL] 命令退出码 ${result.status}${RESET}`);
    process.exit(result.status ?? 1);
  }
}

function ok(message) {
  console.log(`\n${GREEN}[OK] ${message}${RESET}`);
}

module.exports = { root, run, ok };