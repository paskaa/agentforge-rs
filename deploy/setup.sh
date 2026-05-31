#!/usr/bin/env bash
# AgentForge-RS 一键部署脚本
# 用法: sudo bash deploy/setup.sh
set -euo pipefail

REPO_DIR="/root/agentforge-rs"
INSTALL_DIR="/usr/local/bin"
SERVICE_DIR="/etc/systemd/system"
AGENTS="guanyu zhaoyun xunyu zhangfei huatuo chenlin liubei zhugeliang"

echo "🔧 AgentForge-RS 部署脚本"
echo "========================="

# 1. Build
echo ""
echo "📦 Step 1: 构建 Release..."
cd "$REPO_DIR"
cargo build --release
cp target/release/agentforge "$INSTALL_DIR/agentforge"
echo "✅ 二进制已安装到 $INSTALL_DIR/agentforge"

# 2. Config
echo ""
echo "⚙️  Step 2: 检查配置..."
if [ ! -f "$REPO_DIR/config/agentforge.yaml" ]; then
  echo "⚠️  config/agentforge.yaml 不存在，从模板复制..."
  cp "$REPO_DIR/config/agentforge.yaml.example" "$REPO_DIR/config/agentforge.yaml"
  echo "📝 请编辑 $REPO_DIR/config/agentforge.yaml 填入实际凭据"
fi

# 3. Codex config
echo ""
echo "🤖 Step 3: 检查 Codex 配置..."
CODEX_HOME="${HOME}/.codex"
if [ ! -f "$CODEX_HOME/config.toml" ]; then
  echo "⚠️  Codex config.toml 不存在，从模板复制..."
  mkdir -p "$CODEX_HOME"
  cp "$REPO_DIR/codex-config/config.toml.example" "$CODEX_HOME/config.toml"
fi

# Copy skills
echo "📋 同步 Skills..."
mkdir -p "$CODEX_HOME/skills"
cp -r "$REPO_DIR/skills/"* "$CODEX_HOME/skills/"

# 4. Systemd services
echo ""
echo "🔧 Step 4: 安装 systemd 服务..."
cp "$REPO_DIR/deploy/systemd/agentforge-rust@.service" "$SERVICE_DIR/"
cp "$REPO_DIR/deploy/systemd/agentforge-pipeline.service" "$SERVICE_DIR/"
cp "$REPO_DIR/deploy/systemd/agentforge-ws@.service" "$SERVICE_DIR/"
systemctl daemon-reload

# 5. Enable services
echo ""
echo "🚀 Step 5: 启用服务..."
systemctl enable agentforge-pipeline.service
for agent in $AGENTS; do
  systemctl enable "agentforge-rust@${agent}.service"
done
echo "✅ 所有服务已启用"

echo ""
echo "========================="
echo "✅ 部署完成！"
echo ""
echo "启动命令:"
echo "  systemctl start agentforge-pipeline"
echo "  systemctl start agentforge-rust@guanyu"
echo "  systemctl start agentforge-rust@zhaoyun"
echo "  # ... 或批量启动:"
echo "  for a in $AGENTS; do systemctl start agentforge-rust@\$a; done"
