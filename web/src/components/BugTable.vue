<template>
  <div>
    <div class="table-toolbar">
      <el-input v-model="search" placeholder="搜索 Bug 标题..." clearable prefix-icon="Search" size="default" style="width:300px" />
      <el-tag type="info">共 {{ filtered.length }} 条</el-tag>
    </div>
    <el-table :data="filtered" stripe style="width: 100%" max-height="60vh" :default-sort="{ prop: 'id', order: 'descending' }">
      <el-table-column prop="id" label="#" width="80" sortable>
        <template #default="{ row }">
          <span class="mono">#{{ row.id }}</span>
        </template>
      </el-table-column>
      <el-table-column prop="title" label="标题" min-width="300" show-overflow-tooltip />
      <el-table-column prop="status" label="状态" width="100">
        <template #default="{ row }">
          <el-tag :type="statusType(row.status)" size="small">{{ row.status }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="assigned_to" label="指派" width="120" />
      <el-table-column prop="severity" label="严重程度" width="100">
        <template #default="{ row }">
          <el-tag v-if="row.severity" :type="severityType(row.severity)" size="small">{{ row.severity }}</el-tag>
          <span v-else>-</span>
        </template>
      </el-table-column>
    </el-table>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue'

const props = defineProps({ bugs: { type: Array, default: () => [] } })
const search = ref('')

const filtered = computed(() => {
  if (!search.value) return props.bugs
  const q = search.value.toLowerCase()
  return props.bugs.filter(b => b.title.toLowerCase().includes(q) || String(b.id).includes(q))
})

function statusType(s) {
  return { active: 'warning', resolved: 'success', closed: 'info' }[s] || 'info'
}
function severityType(s) {
  const n = parseInt(s) || 0
  if (n >= 4) return 'danger'
  if (n >= 3) return 'warning'
  return 'info'
}
</script>

<style scoped>
.table-toolbar { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
.mono { font-family: monospace; color: #60a5fa; }
:deep(.el-table) { --el-table-bg-color: #1e293b; --el-table-tr-bg-color: #1e293b; --el-table-header-bg-color: #0f172a; --el-table-row-hover-bg-color: rgba(59,130,246,0.08); --el-table-border-color: #334155; --el-table-text-color: #e2e8f0; }
:deep(.el-table .el-table__row--striped td) { background: #0f172a !important; }
</style>
