#!/bin/bash
# AgentForge-RS systemd 服务安装脚本
# 用法: sudo bash deploy/systemd/install.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SERVICE_DIR="/etc/systemd/system"
AGENTS="guanyu zhaoyun xunyu zhangfei huatuo chenlin liubei zhugeliang"

echo "📦 安装 AgentForge-RS systemd 服务..."

# Copy service files
cp "$SCRIPT_DIR/agentforge-rust@.service" "$SERVICE_DIR/"
cp "$SCRIPT_DIR/agentforge-pipeline.service" "$SERVICE_DIR/"

# Enable and start pipeline
systemctl daemon-reload
systemctl enable agentforge-pipeline.service
echo "✅ Pipeline 服务已安装"

# Enable all agent executors
for agent in $AGENTS; do
  systemctl enable agentforge-rust@${agent}.service
  echo "✅ Agent ${agent} 服务已安装"
done

echo ""
echo "🚀 启动服务:"
echo "  systemctl start agentforge-pipeline"
echo "  systemctl start agentforge-rust@{guanyu,zhaoyun,...}"
echo ""
echo "📋 查看日志:"
echo "  journalctl -u agentforge-pipeline -f"
echo "  journalctl -u agentforge-rust@guanyu -f"
