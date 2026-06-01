<template>
  <div>
    <div style="display:flex;align-items:center;gap:12px;margin-bottom:16px">
      <router-link to="/" style="text-decoration:none">
        <el-button size="small" :icon="ArrowLeft">返回仪表盘</el-button>
      </router-link>
      <h1 style="font-size:20px">📋 Bug 明细</h1>
      <el-button size="small" :icon="Refresh" :loading="loading" @click="fetchData" style="margin-left:auto">刷新</el-button>
    </div>

    <el-tabs v-model="activeTab" type="border-card" @tab-change="onTabChange">
      <el-tab-pane name="unclosed">
        <template #label>
          <span>🔒 未关闭 <el-badge :value="unclosedBugs.length" type="warning" class="tab-badge" /></span>
        </template>
        <BugTable :bugs="unclosedBugs" />
      </el-tab-pane>

      <el-tab-pane name="unresolved">
        <template #label>
          <span>⚠️ 未解决 <el-badge :value="unresolvedBugs.length" type="danger" class="tab-badge" /></span>
        </template>
        <div style="margin-bottom:12px;display:flex;align-items:center;gap:12px">
          <el-button type="warning" :icon="Plus" :loading="batchEnqueueLoading" :disabled="unresolvedBugs.length === 0" @click="batchEnqueue">
            全部加入队列 ({{ unresolvedBugs.length }})
          </el-button>
        </div>
        <BugTable :bugs="unresolvedBugs" />
      </el-tab-pane>

      <el-tab-pane name="fixed_today">
        <template #label>
          <span>✅ 今日修复 <el-badge :value="todayFixedBugs.length" type="success" class="tab-badge" /></span>
        </template>
        <div v-if="todayFixedBugs.length > 0" style="margin-bottom:12px">
          <h3 style="font-size:14px;color:#94a3b8;margin-bottom:8px">📋 禅道今日已解决 ({{ todayFixedBugs.length }})</h3>
          <BugTable :bugs="todayFixedBugs" />
        </div>
        <div v-if="recentFixes.length > 0">
          <h3 style="font-size:14px;color:#94a3b8;margin:12px 0 8px">🤖 智能体最近修复记录 ({{ recentFixes.length }})</h3>
          <FixTable :fixes="recentFixes" />
        </div>
        <el-empty v-if="todayFixedBugs.length === 0 && recentFixes.length === 0" description="今日暂无修复记录" :image-size="60" />
      </el-tab-pane>

      <el-tab-pane name="all">
        <template #label>
          <span>📊 全部 <el-badge :value="allBugs.length" class="tab-badge" /></span>
        </template>
        <BugTable :bugs="allBugs" />
      </el-tab-pane>
    </el-tabs>

    <div style="margin-top:8px;font-size:11px;color:#475569;text-align:right" v-if="zentao.last_sync">
      禅道同步: {{ zentao.last_sync }} · 活跃: {{ zentao.active }} · 总计: {{ zentao.total }}
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, provide } from 'vue'
import { ElMessage } from 'element-plus'
import { useRoute, useRouter } from 'vue-router'
import { ArrowLeft, Refresh, Plus } from '@element-plus/icons-vue'
import BugTable from '../components/BugTable.vue'
import FixTable from '../components/FixTable.vue'

const route = useRoute()
const router = useRouter()
const activeTab = ref(route.params.filter || 'unclosed')
const zentao = ref({})
const recentFixes = ref([])
const loading = ref(false)
const batchEnqueueLoading = ref(false)

const allBugs = computed(() => zentao.value.bugs || [])
const unclosedBugs = computed(() => allBugs.value.filter(b => b.status !== 'closed'))
const unresolvedBugs = computed(() => allBugs.value.filter(b => b.status === 'active'))
const todayFixedBugs = computed(() => zentao.value.today_fixed || [])

async function batchEnqueue() {
  if (!unresolvedBugs.value.length) return
  batchEnqueueLoading.value = true
  try {
    const bugIds = unresolvedBugs.value.map(b => b.id)
    const res = await fetch('/api/bugs/batch-enqueue', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ bug_ids: bugIds })
    })
    const data = await res.json()
    if (data.ok) {
      ElMessage.success(`已入列 ${data.enqueued} 个 Bug`)
    } else {
      ElMessage.warning(`入列 ${data.enqueued}/${data.total} 个，${data.errors?.length || 0} 个失败`)
    }
  } catch (e) {
    ElMessage.error('批量入列失败: ' + e.message)
  }
  batchEnqueueLoading.value = false
}

async function fetchData() {
  loading.value = true
  try {
    const [zenRes, dashRes] = await Promise.all([
      fetch('/api/zentao/stats?refresh=true'),
      fetch('/api/dashboard')
    ])
    zentao.value = await zenRes.json()
    const dash = await dashRes.json()
    recentFixes.value = dash.recent || []
  } catch {}
  loading.value = false
}

async function onEnqueue(bugId) {
  try {
    const res = await fetch('/api/bugs/enqueue', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ bug_id: bugId })
    })
    const data = await res.json()
    if (data.ok) {
      ElMessage.success(`Bug #${bugId} 已入列`)
    } else {
      ElMessage.error(data.error || '入列失败')
    }
  } catch (e) {
    ElMessage.error('入列失败: ' + e.message)
  }
}

function onTabChange(tab) {
  router.replace({ params: { filter: tab } })
}

provide('onEnqueue', onEnqueue)
onMounted(fetchData)
</script>

<style scoped>
.tab-badge { margin-left: 4px; vertical-align: middle; }
</style>
