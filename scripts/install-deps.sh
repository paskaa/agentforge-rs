#!/bin/bash
# ============================================================
# AgentForge-RS 全量依赖安装脚本
# 用法：bash scripts/install-deps.sh
# ============================================================
set -euo pipefail

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
log()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
HIS_REPO="/root/.openclaw/workspace/his-repo"
HIS_UI="${HIS_REPO}/openhis-ui-vue3"

echo "========================================="
echo " AgentForge-RS 全量依赖安装"
echo "========================================="

# 1. Rust
if ! command -v cargo &>/dev/null; then
    log "安装 Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi
log "Rust: $(cargo -v 2>&1 | head -1)"

# 2. Node.js
if ! command -v node &>/dev/null; then
    log "安装 Node.js..."
    curl -fsSL https://deb.nodesource.com/setup_22.x | bash - > /dev/null 2>&1
    apt-get install -y -qq nodejs > /dev/null 2>&1
fi
log "Node.js: $(node -v)"

# 3. Java 17 + Maven
if ! command -v java &>/dev/null; then
    log "安装 Java 17..."
    apt-get update -qq && apt-get install -y -qq openjdk-17-jdk maven > /dev/null 2>&1
fi
log "Java: $(java -version 2>&1 | head -1)"

# 4. Docker
if ! command -v docker &>/dev/null; then
    log "安装 Docker..."
    curl -fsSL https://get.docker.com | sh
fi
log "Docker: $(docker --version 2>/dev/null | head -1)"

# 5. Redis (Docker)
if ! docker ps --format '{{.Names}}' | grep -q agentforge-redis; then
    log "启动 Redis..."
    docker run -d --name agentforge-redis --restart=always \
        -p 16379:6379 -v /data/agentforge-redis:/data redis:7-alpine redis-server --appendonly yes
fi
log "Redis: localhost:16379"

# 6. PostgreSQL (Docker)
if ! docker ps --format '{{.Names}}' | grep -q agentforge-pg; then
    log "启动 PostgreSQL..."
    docker run -d --name agentforge-pg --restart=always \
        -p 15432:5432 -e POSTGRES_PASSWORD=Jchl1528 -e POSTGRES_DB=postgresql \
        -v /data/agentforge-pg:/var/lib/postgresql/data postgres:16-alpine
fi
log "PostgreSQL: localhost:15432"

# 7. HIS 前端依赖
if [ -d "$HIS_UI" ]; then
    log "安装 HIS 前端依赖..."
    cd "$HIS_UI" && npm install --no-fund --no-audit 2>/dev/null || true
    log "HIS 前端依赖完成"
fi

# 8. Playwright 浏览器
if [ -d "$HIS_UI" ]; then
    cd "$HIS_UI"
    if [ ! -d "$HOME/.cache/ms-playwright/chromium"* ]; then
        log "安装 Playwright Chromium..."
        npx playwright install chromium 2>/dev/null || true
    fi
    log "Playwright 已就绪"
fi

# 9. AgentForge 编译
log "编译 agentforge-rs..."
cd "$PROJECT_DIR" && cargo build --release 2>&1 | tail -3
cp target/release/agentforge /usr/local/bin/agentforge
chmod +x /usr/local/bin/agentforge
log "agentforge 已安装到 /usr/local/bin/"

# 10. Systemd 服务
cat > /etc/systemd/system/agentforge-web.service << 'EOF'
[Unit]
Description=AgentForge-RS Dashboard
After=network.target docker.service
[Service]
Type=simple
ExecStart=/usr/local/bin/agentforge web --port 18081
Restart=always
RestartSec=5
Environment=RUST_LOG=info
[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/agentforge-pipeline.service << 'EOF'
[Unit]
Description=AgentForge-RS Pipeline
After=network.target docker.service
[Service]
Type=simple
ExecStart=/usr/local/bin/agentforge pipeline --max-bugs 5
Restart=always
RestartSec=30
Environment=RUST_LOG=info
[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/agentforge-rust@.service << 'EOF'
[Unit]
Description=AgentForge Rust Executor - %i
After=network.target docker.service
[Service]
Type=simple
ExecStart=/usr/local/bin/agentforge executor --agent %i
Restart=always
RestartSec=10
Environment=RUST_LOG=info
[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload

# 11. 启动服务
systemctl enable --now agentforge-web.service 2>/dev/null
for agent in guanyu zhaoyun xunyu zhangfei huatuo chenlin liubei zhugeliang; do
    mkdir -p "/tmp/agentforge-worktrees/${agent}"
    systemctl enable --now "agentforge-rust@${agent}.service" 2>/dev/null
done
log "所有服务已启动"

# 12. HIS dev server
if [ -d "$HIS_UI" ]; then
    if ! curl -s -o /dev/null -w "%{http_code}" http://localhost:81 2>/dev/null | grep -q 200; then
        cd "$HIS_UI"
        nohup npx vite --mode dev --port 81 --host 0.0.0.0 > /tmp/his-dev.log 2>&1 &
        sleep 3
        log "HIS dev server: http://localhost:81"
    fi
fi

echo ""
echo "========================================="
echo " ✅ 安装完成！"
echo "========================================="
echo " Dashboard:  http://localhost:18081"
echo " HIS 前端:   http://localhost:81"
echo " Redis:      localhost:16379"
echo " PostgreSQL: localhost:15432"
