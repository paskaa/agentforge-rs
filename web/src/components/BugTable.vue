<template>
  <div>
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px">
      <el-input v-model="search" placeholder="搜索 Bug 标题或 ID..." clearable prefix-icon="Search" style="width:300px" />
      <el-tag type="info">共 {{ filtered.length }} 条</el-tag>
    </div>
    <el-table :data="paginated" stripe style="width:100%" :default-sort="{ prop: 'id', order: 'descending' }">
      <el-table-column prop="id" label="#" width="80" sortable>
        <template #default="{ row }">
          <a :href="row.url" target="_blank" rel="noopener" class="bug-link">
            #{{ row.id }}
            <el-icon style="font-size:10px;margin-left:2px"><Link /></el-icon>
          </a>
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
          <span v-else style="color:#475569">-</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="80" fixed="right">
        <template #default="{ row }">
          <el-button v-if="onEnqueue" size="small" type="primary" :icon="Plus" circle @click="onEnqueue(row.id)" title="入列修复" />
        </template>
      </el-table-column>
    </el-table>
    <div style="display:flex;justify-content:center;margin-top:12px" v-if="filtered.length > pageSize">
      <el-pagination v-model:current-page="page" :page-size="pageSize" :total="filtered.length" layout="prev, pager, next" background small />
    </div>
  </div>
</template>

<script setup>
import { ref, computed, inject } from 'vue'
import { Link, Plus } from '@element-plus/icons-vue'

const props = defineProps({ bugs: { type: Array, default: () => [] } })
const onEnqueue = inject('onEnqueue', null)
const search = ref('')
const page = ref(1)
const pageSize = 20

const filtered = computed(() => {
  if (!search.value) return props.bugs
  const q = search.value.toLowerCase()
  return props.bugs.filter(b => b.title.toLowerCase().includes(q) || String(b.id).includes(q))
})
const paginated = computed(() => {
  const start = (page.value - 1) * pageSize
  return filtered.value.slice(start, start + pageSize)
})

function statusType(s) { return { active: 'warning', resolved: 'success', closed: 'info' }[s] || 'info' }
function severityType(s) { return parseInt(s) >= 4 ? 'danger' : parseInt(s) >= 3 ? 'warning' : 'info' }
</script>

<style scoped>
.bug-link {
  font-family: monospace; color: #60a5fa; text-decoration: none;
  display: inline-flex; align-items: center; transition: color 0.15s;
}
.bug-link:hover { color: #93c5fd; text-decoration: underline; }
</style>
