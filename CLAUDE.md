# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

OpenGate：OpenAI / Claude 聚合网关。客户端用 OpenAI 或 Claude 格式请求 `/v1/*`，网关选渠道、做双向格式转换、按人民币记账（只统计、不扣费）。通用工程规范以 `~/.claude/iges/` 为准，本文件只记项目事实。

## 常用命令

```bash
# 后端（Rust · axum · sqlx/SQLite）
cd backend
cargo run                    # 读 .env；本仓库 .env 用 PORT=3001（与前端代理一致）
cargo test                   # 单元测试全部在 lib target
cargo test convert::         # 按模块跑；单个测试: cargo test test_calculate_basic
cargo build --release --features embed-frontend   # 嵌入前端（要求 frontend/dist 已存在）

# 前端（React 19 · antd 6 · VChart · Vite）
cd frontend
pnpm dev                     # vite 把 /api /v1 /health 代理到 127.0.0.1:3001
pnpm build                   # tsc -b && vite build；仅类型检查: npx tsc -b
pnpm lint                    # oxlint

# 四种构建产物（Node.js 脚本，跨平台；node scripts/build-*.js）
node scripts/build-frontend.js | build-backend.js | build-fullstack.js | build-desktop.js [--no-bundle]
```

Windows + windows-gnu 工具链开发；Tauri 2 在该工具链可直接编译，无需 MSVC。

## 架构

三个交付形态共用一套后端代码：`backend/` 是 lib + bin —— `lib.rs::build_router()` 被 `main.rs`（命令行版）和 `desktop/src-tauri`（Tauri 2 桌面版，进程内随机端口起服务、WebView 加载该端口）共同调用；`embed-frontend` 特性（`webui.rs`，rust-embed）把 `frontend/dist` 编进二进制并做 SPA 回退（`/api`、`/v1` 不回退，404 返回 JSON）。

### 中继链路（backend/src/handlers/relay.rs，核心）

请求 `/v1/chat/completions | /v1/messages` → `middleware::token_auth_mw` 校验 sk 令牌 → 读 model/stream → 令牌 `model_limits` 检查 → `channel_select`（priority DESC，同优先级按 weight 加权随机；多 key 渠道轮询）→ `model_mapping` 映射上游模型名 → 定价查找（先客户端名后上游名；`price_missing_policy=reject` 时无价拒绝）→ 同格式则透传（重写 model、注入 stream_options），跨格式走 `convert.rs` → 流式经 `stream.rs` 的 `StreamConverter`（四种客户端×渠道组合）逐事件转换 → 结束后按 usage 计费并写 `logs` 表。

- 计费：`(非缓存输入×input + cache_read×cache_read + cache_write×cache_write + 输出×output) / 1M`，单位元/1M tokens。usage 提取兼容 DeepSeek `prompt_cache_hit_tokens`、OpenCode `normalizedUsage`；Claude 的 prompt = input + cache_read + cache_creation（见 `stream.rs::try_claude_usage`）。
- Playground 无专用聊天接口：前端直接带**管理员 JWT** 调 `/v1/chat/completions`（`token_auth_mw` 的 fallback 分支，日志记为 token_id=0「游乐场」），会话/消息由 `/api/admin/playground/*` 落库，自动标题在消息创建接口里做。

### 数据库不变量（改动前必读）

SQLite schema 固定为 GORM 时代的布局（表：`users tokens channels logs settings conversations conversation_messages`），时间戳是**本地时区偏移的 TEXT**（`2026-07-21 09:04:17.701689100-05:00`）。所有写入必须走 `util::now_db_string()` / `to_db_string()`，输出 JSON 用 `db_time_to_rfc3339()` 转 RFC3339——时间筛选靠字符串比较，偏移量不一致会静默出错。首次运行无种子管理员，走 `/api/admin/setup` 初始化；`ADMIN_RESET_PASSWORD=1` 环境变量是找回密码的逃生门。

### 前端约定

- `api/client.ts` 的 axios 拦截器**解包信封**：管理接口返回 `{"success":bool,"data":...}`，拦截器把 `res.data` 替换为内层 `data`（调用点直接 `r.data`），失败自动 toast，401 清登录态跳 `/login`。登录态只通过 `utils/auth.ts` 读写（token + username 成对增删）。
- 共享模块优先：格式化用 `utils/format.ts`（Token 缩写 / `formatCostRMB`——仪表盘 4 位、日志 6 位精度）、渠道 models CSV 解析用 `utils/models.ts`、表单抽屉用 `components/FormDrawer`（antd 6 的 Drawer 用数字 `size`，`width` 已弃用）。
- Playground 聊天是唯一直接 `fetch` 的地方（需要 ReadableStream 读 SSE），其余请求一律走 `api.*`。
- 视觉规范见 `DESIGN.md`（暖米色画布 + 珊瑚色主色的设计 token）。

### 历史

原 Go（Fiber/GORM）后端在 `a04d5fb` 移除，Rust 版是其行为兼容重写（API 与 db schema 一致）；对照旧实现可查 git 历史。
