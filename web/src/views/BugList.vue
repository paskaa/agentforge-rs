<template>
  <div>
    <div style="display:flex;align-items:center;gap:12px;margin-bottom:16px">
      <router-link to="/" style="text-decoration:none">
        <el-button size="small" :icon="ArrowLeft">返回仪表盘</el-button>
      </router-link>
      <h1 style="font-size:20px">📋 Bug 明细</h1>
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
        <BugTable :bugs="unresolvedBugs" />
      </el-tab-pane>

      <el-tab-pane name="fixed_today">
        <template #label>
          <span>✅ 今日修复 <el-badge :value="recentFixes.length" type="success" class="tab-badge" /></span>
        </template>
        <FixTable :fixes="recentFixes" />
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

onMounted(async () => {
  try {
    const [zenRes, dashRes] = await Promise.all([
      fetch('/api/zentao/stats'),
      fetch('/api/dashboard')
    ])
    zentao.value = await zenRes.json()
    const dash = await dashRes.json()
    recentFixes.value = dash.recent || []
  } catch {}
})
</script>

<style scoped>
.tab-badge { margin-left: 4px; vertical-align: middle; }
</style>
