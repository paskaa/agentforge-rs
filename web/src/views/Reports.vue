<template>
  <div class="reports">
    <h1 style="margin-bottom:20px;font-size:22px">📚 修复报告归档</h1>

    <el-card shadow="never">
      <template #header>
        <div style="display:flex;align-items:center;justify-content:space-between">
          <span>归档列表（共 {{ reports.length }} 份）</span>
          <el-button :icon="Refresh" circle :loading="loading" @click="fetchReports" size="small" />
        </div>
      </template>
      <el-table :data="reports" stripe style="width:100%" :default-sort="{ prop: 'created_at', order: 'descending' }">
        <el-table-column prop="bug_id" label="Bug" width="80">
          <template #default="{ row }">
            <router-link :to="'/report/' + row.bug_id" style="color:#60a5fa;text-decoration:none;font-family:monospace">
              #{{ row.bug_id }}
            </router-link>
          </template>
        </el-table-column>
        <el-table-column prop="title" label="标题" min-width="200" show-overflow-tooltip />
        <el-table-column prop="reporter" label="提出人" width="100" />
        <el-table-column prop="test_result" label="测试" width="80">
          <template #default="{ row }">
            <el-tag :type="row.test_result === 'ok' ? 'success' : 'danger'" size="small">
              {{ row.test_result === 'ok' ? '✅' : '❌' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="duration_ms" label="耗时" width="80">
          <template #default="{ row }">
            {{ (row.duration_ms / 1000).toFixed(0) }}s
          </template>
        </el-table-column>
        <el-table-column prop="commit_hash" label="Commit" width="120">
          <template #default="{ row }">
            <span style="font-family:monospace;font-size:12px;color:#94a3b8">{{ row.commit_hash?.slice(0,10) }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="created_at" label="归档时间" width="160" sortable />
      </el-table>
      <el-empty v-if="reports.length === 0" description="暂无归档报告" :image-size="60" />
    </el-card>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { Refresh } from '@element-plus/icons-vue'

const reports = ref([])
const loading = ref(false)

async function fetchReports() {
  loading.value = true
  try {
    const res = await fetch('/api/bugs/reports')
    const data = await res.json()
    reports.value = data.reports || []
  } catch {}
  loading.value = false
}

onMounted(fetchReports)
</script>
