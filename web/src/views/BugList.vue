<template>
  <div class="bug-list-page">
    <div class="page-header">
      <router-link to="/" class="back-link">
        <el-icon><ArrowLeft /></el-icon> 返回仪表盘
      </router-link>
      <h1>📋 Bug 明细</h1>
    </div>

    <el-tabs v-model="activeTab" type="border-card" @tab-change="onTabChange">
      <el-tab-pane label="未关闭 Bug" name="unclosed">
        <template #label>
          <span>🔒 未关闭 <el-badge :value="unclosedBugs.length" type="warning" /></span>
        </template>
        <BugTable :bugs="unclosedBugs" />
      </el-tab-pane>

      <el-tab-pane label="未解决 Bug" name="unresolved">
        <template #label>
          <span>⚠️ 未解决 <el-badge :value="unresolvedBugs.length" type="danger" /></span>
        </template>
        <BugTable :bugs="unresolvedBugs" />
      </el-tab-pane>

      <el-tab-pane label="今日修复" name="fixed_today">
        <template #label>
          <span>✅ 今日修复 <el-badge :value="recentFixes.length" type="success" /></span>
        </template>
        <FixTable :fixes="recentFixes" />
      </el-tab-pane>

      <el-tab-pane label="全部 Bug" name="all">
        <template #label>
          <span>📊 全部 <el-badge :value="allBugs.length" /></span>
        </template>
        <BugTable :bugs="allBugs" />
      </el-tab-pane>
    </el-tabs>

    <div class="sync-info" v-if="zentao.last_sync">
      禅道同步: {{ zentao.last_sync }} · 活跃: {{ zentao.active }} · 总计: {{ zentao.total }}
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ArrowLeft } from '@element-plus/icons-vue'
import BugTable from '../components/BugTable.vue'
import FixTable from '../components/FixTable.vue'

const route = useRoute()
const router = useRouter()
const activeTab = ref(route.params.filter || 'unclosed')
const zentao = ref({})
const recentFixes = ref([])

const allBugs = computed(() => zentao.value.bugs || [])
const unclosedBugs = computed(() => allBugs.value.filter(b => b.status !== 'closed'))
const unresolvedBugs = computed(() => allBugs.value.filter(b => b.status === 'active'))

function onTabChange(tab) {
  router.replace({ params: { filter: tab } })
}

async function fetchZentao() {
  try {
    const r = await fetch('/api/zentao/stats')
    zentao.value = await r.json()
  } catch {}
}

async function fetchRecent() {
  try {
    const r = await fetch('/api/dashboard')
    const d = await r.json()
    recentFixes.value = d.recent || []
  } catch {}
}

onMounted(() => {
  fetchZentao()
  fetchRecent()
})
</script>

<style scoped>
.bug-list-page { max-width: 1200px; }
.page-header { display: flex; align-items: center; gap: 16px; margin-bottom: 20px; }
.page-header h1 { font-size: 20px; font-weight: 600; }
.back-link {
  display: inline-flex; align-items: center; gap: 4px;
  color: #64748b; text-decoration: none; font-size: 13px;
  padding: 6px 10px; border-radius: 6px; transition: all 0.15s;
}
.back-link:hover { background: #334155; color: #e2e8f0; }

:deep(.el-tabs) { border-radius: 10px; overflow: hidden; }
:deep(.el-tabs__header) { background: #1e293b; border-color: #334155; }
:deep(.el-tabs__item) { color: #94a3b8; }
:deep(.el-tabs__item.is-active) { color: #60a5fa; background: #0f172a; }
:deep(.el-tabs__content) { background: #0f172a; padding: 16px; }
:deep(.el-badge__content) { font-size: 10px; }

.sync-info {
  margin-top: 12px; font-size: 11px; color: #475569; text-align: right;
}
</style>
