# AgentForge-RS 依赖清单

## 系统依赖

| 组件 | 版本 | 用途 | 安装方式 |
|------|------|------|---------|
| Rust | 1.75+ | agentforge 编译 | rustup |
| Node.js | 22+ | HIS 前端 + Playwright | nvm / nodesource |
| Java | 17 | HIS 后端 (Spring Boot) | apt |
| Maven | 3.8+ | HIS 后端构建 | apt |
| Docker | 24+ | Redis + PostgreSQL | get.docker.com |

## Docker 服务

| 服务 | 镜像 | 端口 | 用途 |
|------|------|------|------|
| Redis | redis:7-alpine | 16379 | 任务队列 + 缓存 |
| PostgreSQL | postgres:16-alpine | 15432 | 数据库 |

## HIS 前端 npm 关键依赖

| 包 | 版本 | 用途 |
|---|------|------|
| vue | 3.x | 前端框架 |
| element-plus | ^2.12 | UI 组件库 |
| axios | 0.27 | HTTP 客户端 |
| echarts | ^5.4 | 图表 |
| playwright | (devDep) | E2E 测试 |

## HIS 后端 Maven 关键依赖

| 组件 | 用途 |
|------|------|
| Spring Boot 2.x | Web 框架 |
| MyBatis-Plus | ORM |
| PostgreSQL Driver | 数据库驱动 |

## AgentForge-RS Rust 关键依赖

| crate | 版本 | 用途 |
|-------|------|------|
| tokio | 1 | 异步运行时 |
| redis | 0.25 | Redis 客户端 |
| reqwest | 0.12 | HTTP 客户端 (禅道 API) |
| sqlx | 0.7 | SQLite (traces) |
| axum | 0.7 | Web 服务 (Dashboard) |
| serde_json | 1 | JSON 序列化 |
| tracing | 0.1 | 日志 |

## 端口清单

| 端口 | 服务 |
|------|------|
| 81 | HIS 前端 dev server |
| 8650 | HIS 后端 (Spring Boot) |
| 16379 | Redis |
| 15432 | PostgreSQL |
| 18081 | AgentForge Dashboard |
