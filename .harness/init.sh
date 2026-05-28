#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> 当前目录: $PWD"
echo "==> Git 状态"
git status --short 2>/dev/null || true
git log --oneline -3 2>/dev/null || true

echo ""
echo "==> 编译检查"
cargo check 2>&1 | tail -5

echo ""
echo "==> 测试"
cargo test 2>&1 | tail -5

echo ""
echo "==> 读取进度"
if [ -f .harness/PROGRESS.md ]; then
    head -20 .harness/PROGRESS.md
fi

echo ""
echo "==> 环境就绪 ✅"
