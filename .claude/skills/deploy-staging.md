---
name: deploy-staging
description: 部署到生产环境的标准流程
---

# 部署流程

1. 先跑 `cargo check && cargo test`，全过才往下
2. 构建: `cargo build --release`
3. 停止所有服务: `systemctl stop agentforge-web && systemctl stop agentforge-rust@*.service`
4. 替换二进制: `cp target/release/agentforge /usr/local/bin/agentforge`
5. 构建前端: `cd web && npm run build && cp dist/* ../static/assets/`
6. 启动服务: `systemctl start agentforge-web && systemctl start agentforge-rust@*.service`
7. 验证: `curl -s http://localhost:18081/api/health`
8. 报告结果: 通过了说通过, 哪个失败了说哪个
