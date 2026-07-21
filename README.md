# OpenGate — OpenAI / Claude 聚合网关

精简版 AI API 聚合网关：客户端可用 **OpenAI** 或 **Claude** 格式请求，网关转发到上游（OpenAI / Claude 协议），负责**格式转换**与 **Token / 人民币用量统计**（**不扣费、无限额**）。

## 技术栈

| 层 | 技术 |
|----|------|
| 后端 | Rust · axum · sqlx · SQLite |
| 前端 | React · TypeScript · Vite · Ant Design |
| 部署 | 本地直接运行（无 Docker） |

## 功能

- 兼容端点：`POST /v1/chat/completions`、`POST /v1/messages`、`GET /v1/models`
- 流式 SSE；OpenAI ↔ Claude 请求/响应双向转换
- 渠道：协议类型、BaseURL、APIKey、模型列表、模型映射、**按模型定价（¥/1M tokens）**
- 令牌：仅鉴权与日志归属（无额度）
- 日志：时间、渠道、令牌、模型、耗时、Token 明细、**费用(¥)**、请求/响应 body
- 管理后台：仪表盘 / 渠道 / 令牌 / 日志 / 设置（单管理员）

## 快速开始

### 1. 后端

```bash
cd backend
cp .env.example .env   # 可改 JWT_SECRET / 端口
cargo run
# 或: cargo build --release && ./target/release/gateway.exe
```

默认：

- 地址：`http://127.0.0.1:3000`
- 首次启动访问管理台会引导初始化管理员账号（`/setup`）

### 2. 前端

```bash
cd frontend
npm install
npm run dev
```

打开 `http://127.0.0.1:5173`，Vite 已代理 `/api`、`/v1` 到后端 `:3000`。

### 3. 配置并调用

1. 登录后台 → **渠道** → 新建（填 BaseURL、Key、模型、定价 JSON）
2. **令牌** → 新建，复制 `sk-...`
3. 调用：

```bash
curl http://127.0.0.1:3000/v1/chat/completions \
  -H "Authorization: Bearer sk-你的令牌" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-v4-flash",
    "messages": [{"role":"user","content":"hi"}],
    "stream": false
  }'
```

Claude 格式：

```bash
curl http://127.0.0.1:3000/v1/messages \
  -H "x-api-key: sk-你的令牌" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-v4-flash",
    "max_tokens": 256,
    "messages": [{"role":"user","content":"hi"}]
  }'
```

## 定价 JSON 示例（渠道字段 `pricing`）

单位：**元人民币 / 1M tokens**

```json
{
  "deepseek-v4-flash": {
    "input": 0.5,
    "output": 2.0,
    "cache_read": 0.05,
    "cache_write": 0.5
  }
}
```

计费（仅统计）：

```
cost_rmb = (non_cached_input * input
          + cache_read * cache_read
          + cache_write * cache_write
          + completion * output) / 1_000_000
```

例：输出 1M tokens、output=2 → 日志 `cost_rmb = 2`。

## 环境变量

见 [backend/.env.example](backend/.env.example)：

| 变量 | 默认 | 说明 |
|------|------|------|
| `PORT` | `3000` | 监听端口 |
| `DB_PATH` | `data/gateway.db` | SQLite 路径 |
| `JWT_SECRET` | … | 管理端 JWT |
| `ADMIN_USER` / `ADMIN_PASSWORD` | admin / admin123 | 仅配合 `ADMIN_RESET_PASSWORD=1` 找回密码用（管理员通过 `/setup` 页初始化） |
| `REQUEST_TIMEOUT` | `300` | 上游超时秒 |

## 构建产物

`scripts/` 下提供四种构建脚本（PowerShell）：

| 脚本 | 产物 | 说明 |
|------|------|------|
| `build-frontend.ps1` | `frontend/dist/` | 纯前端静态站点 |
| `build-backend.ps1` | `backend/target/release/gateway.exe` | 纯后端 API 网关（不含 UI） |
| `build-fullstack.ps1` | `backend/target/release/gateway.exe` | **单文件全栈**：前端嵌入二进制（`--features embed-frontend`），`/` 直接出管理台，SPA 路由自动回退 |
| `build-desktop.ps1` | `desktop/src-tauri/target/release/opengate-desktop.exe` + NSIS 安装包 | Tauri 2 桌面端：进程内启动网关，WebView 加载本地端口；数据库在系统应用数据目录（加 `-NoBundle` 跳过安装包） |

```powershell
# 例：构建单文件全栈版
powershell -ExecutionPolicy Bypass -File scripts/build-fullstack.ps1
```

## 目录

```
backend/          Rust axum 服务（lib + bin；embed-frontend 特性可嵌入前端）
frontend/         React 管理台
desktop/          Tauri 2 桌面端外壳
scripts/          四种构建产物脚本
```

## 说明

- **不做**：Docker、多用户注册、额度扣费、支付
- 参考项目：`new-api` 的渠道/转换/日志思路；本项目为精简重写
