# OpenGate — OpenAI / Claude 聚合网关

精简版 AI API 聚合网关：客户端可用 **OpenAI** 或 **Claude** 格式请求，网关转发到上游（OpenAI / Claude 协议），负责**格式转换**与 **Token / 人民币用量统计**（**不扣费、无限额**）。

## 技术栈

| 层 | 技术 |
|----|------|
| 后端 | Rust · axum · sqlx · SQLite（`backend-rust/`，由原 Go 版重写） |
| 前端 | React · TypeScript · Vite · Ant Design |
| 部署 | 本地直接运行（无 Docker） |

> 原 Go（Fiber · GORM）实现保留在 `backend/`，API 与数据库 schema 与 Rust 版完全兼容，两者可指向同一个 `gateway.db` 互换运行。

## 功能

- 兼容端点：`POST /v1/chat/completions`、`POST /v1/messages`、`GET /v1/models`
- 流式 SSE；OpenAI ↔ Claude 请求/响应双向转换
- 渠道：协议类型、BaseURL、APIKey、模型列表、模型映射、**按模型定价（¥/1M tokens）**
- 令牌：仅鉴权与日志归属（无额度）
- 日志：时间、渠道、令牌、模型、耗时、Token 明细、**费用(¥)**、请求/响应 body
- 管理后台：仪表盘 / 渠道 / 令牌 / 日志 / 设置（单管理员）

## 快速开始

### 1. 后端（Rust）

```bash
cd backend-rust
cp .env.example .env   # 可改 ADMIN_PASSWORD / JWT_SECRET
cargo run
# 或: cargo build --release && ./target/release/gateway.exe
```

默认：

- 地址：`http://127.0.0.1:3000`
- 管理员：`admin` / `admin123`（首次启动会打印）

> 沿用旧 Go 版数据：把 `.env` 里的 `DB_PATH` 指向原来的 `backend/data/gateway.db` 即可，表结构与密码哈希完全兼容。
>
> 旧 Go 版启动方式：`cd backend && go run ./cmd/server`。

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

见 [backend-rust/.env.example](backend-rust/.env.example)：

| 变量 | 默认 | 说明 |
|------|------|------|
| `PORT` | `3000` | 监听端口 |
| `DB_PATH` | `data/gateway.db` | SQLite 路径 |
| `JWT_SECRET` | … | 管理端 JWT |
| `ADMIN_USER` / `ADMIN_PASSWORD` | admin / admin123 | 种子管理员 |
| `REQUEST_TIMEOUT` | `300` | 上游超时秒 |

## 目录

```
backend-rust/     Rust axum 服务（当前后端）
backend/          Go Fiber 服务（原实现，保留作参照）
frontend/         React 管理台
```

## 说明

- **不做**：Docker、多用户注册、额度扣费、支付
- 参考项目：`new-api` 的渠道/转换/日志思路；本项目为精简重写
