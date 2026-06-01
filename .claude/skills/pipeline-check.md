---
name: pipeline-check
description: 检查管线状态和智能体健康度
---

# 管线检查流程

1. 检查所有服务状态: `systemctl status agentforge-rust@*.service`
2. 检查 Redis 队列: `redis-cli -p 16379 llen agent-work-queue:fix:*`
3. 检查最近 traces: `sqlite3 /var/lib/agentforge/traces.db "SELECT ..."`
4. 检查 Zentao 同步状态
5. 生成报告
