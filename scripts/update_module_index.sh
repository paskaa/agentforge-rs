#!/bin/bash
# 自动更新 HealthLink-HIS 代码模块索引
# 扫描 Controller → 提取模块名 → 更新 MODULE_INDEX.md

HIS_REPO="/root/.openclaw/workspace/his-repo"
INDEX_FILE="$HIS_REPO/MD/MODULE_INDEX.md"
BACKUP_DIR="/tmp/agentforge-worktrees"

cd "$HIS_REPO" || exit 1

# 统计当前 Controller 数量
TOTAL=$(find . -name "*Controller.java" -path "*/controller/*" | wc -l)
echo "[update_module_index] 扫描到 $TOTAL 个 Controller"

# 提取所有 Controller 模块名
CONTROLLERS=$(find . -name "*Controller.java" -path "*/controller/*" \
  | sed 's|.*/||; s|Controller\.java||' \
  | sort -u)

# 生成模块列表（追加到文件末尾的详细区域）
TIMESTAMP=$(date '+%Y-%m-%d %H:%M')

# 更新文件中的时间戳
sed -i "s/> 最后更新:.*/> 最后更新: $TIMESTAMP ($TOTAL 个 Controller)/" "$INDEX_FILE" 2>/dev/null

# 如果没有时间戳行，在第3行后插入
if ! grep -q "最后更新" "$INDEX_FILE"; then
  sed -i "3a> 最后更新: $TIMESTAMP ($TOTAL 个 Controller)" "$INDEX_FILE"
fi

# 同步到所有 agent worktree
for agent in guanyu zhaoyun xunyu zhugeliang zhangfei huatuo chenlin; do
  dest="$BACKUP_DIR/$agent/MD"
  if [ -d "$BACKUP_DIR/$agent" ]; then
    mkdir -p "$dest"
    cp "$INDEX_FILE" "$dest/MODULE_INDEX.md"
  fi
done

echo "[update_module_index] 完成: $TIMESTAMP, $TOTAL modules, synced to all agents"
