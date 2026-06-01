#!/bin/bash
# 清理 agentforge-rs 相关资源（谨慎使用）
set -euo pipefail

echo "⚠️  即将清理以下资源："
echo "  - 所有 agentforge systemd 服务"
echo "  - /tmp/agentforge-worktrees/ 下的 worktree"
echo "  - /var/lib/agentforge/ 下的数据"
echo "  - Docker 容器 agentforge-redis / agentforge-pg"
echo ""
read -p "确认清理？(y/N): " confirm
if [ "$confirm" != "y" ]; then echo "取消"; exit 0; fi

# 停止服务
for svc in agentforge-web agentforge-pipeline; do
    systemctl stop "$svc" 2>/dev/null || true
    systemctl disable "$svc" 2>/dev/null || true
done
for agent in guanyu zhaoyun xunyu zhangfei huatuo chenlin liubei zhugeliang; do
    systemctl stop "agentforge-rust@${agent}" 2>/dev/null || true
    systemctl disable "agentforge-rust@${agent}" 2>/dev/null || true
done
systemctl daemon-reload

# 清理文件
rm -rf /tmp/agentforge-worktrees/*
rm -rf /var/lib/agentforge/*
rm -f /etc/systemd/system/agentforge-*.service

echo "✅ 清理完成"
