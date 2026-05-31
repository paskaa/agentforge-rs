<template>
  <div>
    <el-table :data="fixes" stripe style="width: 100%" max-height="60vh">
      <el-table-column prop="bug" label="Bug" width="100">
        <template #default="{ row }">
          <span class="mono">#{{ row.bug }}</span>
        </template>
      </el-table-column>
      <el-table-column prop="agent" label="智能体" width="120" />
      <el-table-column prop="ok" label="状态" width="100">
        <template #default="{ row }">
          <el-tag :type="row.ok ? 'success' : 'danger'" size="small">{{ row.ok ? '✅ 成功' : '❌ 失败' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="dur" label="耗时" width="100" />
      <el-table-column prop="ts" label="时间" width="180">
        <template #default="{ row }">
          <span class="ts">{{ row.ts }}</span>
        </template>
      </el-table-column>
    </el-table>
    <div v-if="fixes.length === 0" class="empty">今日暂无修复记录</div>
  </div>
</template>

<script setup>
defineProps({ fixes: { type: Array, default: () => [] } })
</script>

<style scoped>
.mono { font-family: monospace; color: #60a5fa; }
.ts { color: #64748b; font-size: 12px; }
.empty { text-align: center; padding: 40px; color: #475569; }
:deep(.el-table) { --el-table-bg-color: #1e293b; --el-table-tr-bg-color: #1e293b; --el-table-header-bg-color: #0f172a; --el-table-row-hover-bg-color: rgba(59,130,246,0.08); --el-table-border-color: #334155; --el-table-text-color: #e2e8f0; }
:deep(.el-table .el-table__row--striped td) { background: #0f172a !important; }
</style>
