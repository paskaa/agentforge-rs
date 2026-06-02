<template>
  <div class="dashboard">
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:20px">
      <h1 style="font-size:22px;margin:0">📊 仪表盘</h1>
      <el-button :icon="Refresh" circle :loading="refreshing" @click="refreshAll" size="small" title="刷新禅道数据" />
    </div>

    <!-- 统计卡片 -->
    <el-row :gutter="16" style="margin-bottom:20px">
      <el-col :span="6">
        <router-link to="/bugs/unclosed" style="text-decoration:none;color:inherit">
          <el-card shadow="hover" class="stat-card" body-style="padding:20px">
            <el-statistic title="未关闭 Bug" :value="zentao.unclosed || 0">
              <template #suffix><span style="font-size:12px;color:#f59e0b">🔍</span></template>
            </el-statistic>
          </el-card>
        </router-link>
      </el-col>
      <el-col :span="6">
        <router-link to="/bugs/unresolved" style="text-decoration:none;color:inherit">
          <el-card shadow="hover" class="stat-card warning" body-style="padding:20px">
            <el-statistic title="未解决 Bug" :value="zentao.unresolved || 0">
              <template #suffix><span style="font-size:12px;color:#ef4444">🔍</span></template>
            </el-statistic>
          </el-card>
        </router-link>
      </el-col>
      <el-col :span="6">
        <router-link to="/bugs/fixed_today" style="text-decoration:none;color:inherit">
          <el-card shadow="hover" class="stat-card success" body-style="padding:20px">
            <el-statistic title="今日修复" :value="zentao.fixed_today || 0">
              <template #suffix><span style="font-size:12px;color:#22c55e">🔍</span></template>
            </el-statistic>
          </el-card>
        </router-link>
      </el-col>
      <el-col :span="6">
        <router-link to="/agents" style="text-decoration:none;color:inherit">
          <el-card shadow="hover" class="stat-card info" body-style="padding:20px">
            <el-statistic title="运行 Agent" :value="stats.running || 0">
              <template #suffix><span style="font-size:12px;color:#3b82f6">🔍</span></template>
            </el-statistic>
          </el-card>
        </router-link>
      </el-col>
    </el-row>

    <div style="text-align:right;font-size:11px;color:#475569;margin:-12px 0 16px" v-if="zentao.last_sync">
      禅道同步: {{ zentao.last_sync }} · 活跃: {{ zentao.active || 0 }} · 总计: {{ zentao.total || 0 }}
    </div>

    <!-- 部署状态 -->
    <el-card shadow="never" style="margin-bottom:20px">
      <template #header>
        <span>🚀 部署状态</span>
      </template>
      <el-descriptions :column="2" size="small" border>
        <el-descriptions-item label="后端服务启动">{{ deploy.backend_start }}</el-descriptions-item>
        <el-descriptions-item label="最新提交">{{ deploy.develop_commit_time }}</el-descriptions-item>
        <el-descriptions-item label="最近修复">
          <div v-for="c in deploy.recent_commits" :key="c">{{ c }}</div>
        </el-descriptions-item>
        <el-descriptions-item label="状态">
          <el-tag :type="deploy.deployed ? 'success' : 'danger'">
            {{ deploy.deployed ? '✅ 已部署' : '❌ 未部署' }}
          </el-tag>
        </el-descriptions-item>
      </el-descriptions>
    </el-card>

    <!-- 协调者 + 子智能体 -->
    <el-card shadow="never" style="margin-bottom:20px">
      <template #header>
        <div style="display:flex;align-items:center;gap:8px">
          <span>🏗️ Subagent 架构</span>
          <el-tag type="warning" size="small">1 主 + {{ agents.length - 1 }} 子</el-tag>
        </div>
      </template>

      <!-- 协调者 -->
      <div style="margin-bottom:16px">
        <router-link :to="'/agent/liubei'" style="text-decoration:none;color:inherit">
          <div class="coordinator-card" :class="coordinator.status">
            <div style="display:flex;align-items:center;gap:12px">
              <div style="font-size:28px">{{ coordinator.icon }}</div>
              <div>
                <div style="font-size:16px;font-weight:700">{{ coordinator.name }} <span style="font-size:12px;color:#f59e0b;font-weight:400">👑 协调者</span></div>
                <div style="font-size:12px;color:#94a3b8">{{ coordinator.role }} · 扫描活跃 Bug → 分派给子智能体</div>
              </div>
              <div style="margin-left:auto;text-align:right">
                <el-tag :type="coordinator.status === 'working' ? 'success' : 'info'" size="small" effect="dark">
                  {{ coordinator.status === 'working' ? '🔄 协调中' : '💤 空闲' }}
                </el-tag>
                <div style="font-size:11px;color:#64748b;margin-top:4px">成功率 {{ coordinator.rate }} · 平均 {{ coordinator.avg_s }}</div>
              </div>
            </div>
          </div>
        </router-link>
      </div>

      <!-- 分派箭头 -->
      <div style="text-align:center;color:#475569;font-size:12px;margin-bottom:12px">
        ▼ 刘备分派 → 子智能体执行 → 结果回报 ▼
      </div>

      <!-- 子智能体 -->
      <el-row :gutter="10">
        <el-col :span="3" v-for="a in subagents" :key="a.id">
          <router-link :to="'/agent/' + a.id" style="text-decoration:none;color:inherit">
            <div class="agent-mini" :class="a.status">
              <div style="font-size:20px">{{ a.icon }}</div>
              <div style="font-size:12px;font-weight:600;margin:4px 0">{{ a.name }}</div>
              <el-tag :type="a.status === 'working' ? 'success' : 'info'" size="small" effect="dark">
                {{ a.status === 'working' ? '🔄 工作中' : '💤 空闲' }}
              </el-tag>
              <div style="font-size:10px;color:#64748b;margin-top:4px">{{ a.role }}</div>
              <el-progress :percentage="parseInt(a.rate) || 0" :stroke-width="3" :color="'#3b82f6'" style="margin-top:6px" />
            </div>
          </router-link>
        </el-col>
      </el-row>
    </el-card>

    <!-- 最近修复 -->
    <el-card shadow="never">
      <template #header>📝 最近修复</template>
      <el-table :data="recent" stripe style="width:100%" max-height="300" :default-sort="{ prop: 'ts', order: 'descending' }">
        <el-table-column prop="bug" label="Bug" width="80">
          <template #default="{ row }">
            <span style="font-family:monospace;color:#60a5fa">#{{ row.bug }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="agent" label="智能体" width="100" />
        <el-table-column prop="ok" label="状态" width="100">
          <template #default="{ row }">
            <el-tag :type="row.ok ? 'success' : 'danger'" size="small">{{ row.ok ? '✅' : '❌' }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="dur" label="耗时" width="80" />
        <el-table-column prop="ts" label="时间" sortable>
          <template #default="{ row }">
            <span style="color:#64748b;font-size:12px">{{ row.ts }}</span>
          </template>
        </el-table-column>
      </el-table>
      <el-empty v-if="recent.length === 0" description="暂无修复记录" :image-size="60" />
    </el-card>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { Link } from '@element-plus/icons-vue'

import { Refresh } from '@element-plus/icons-vue'
const stats = ref({})
const zentao = ref({})
const agents = ref([])
const recent = ref([])
const deploy = ref({})

const coordinator = computed(() => {
  return agents.value.find(a => a.id === 'liubei') || { id: 'liubei', name: '刘备', role: '协调者', icon: '👑', status: 'idle', rate: '0%', avg_s: '0s' }
})
const subagents = computed(() => {
  return agents.value.filter(a => a.id !== 'liubei')
})
const detailVisible = ref(false)
const detailLoading = ref(false)
const detailData = ref(null)
const detailTraces = ref([])
const refreshing = ref(false)
let pollTimer = null

async function fetchAll(forceRefresh = false) {
  try {
    const zenUrl = forceRefresh ? '/api/zentao/stats?refresh=true' : '/api/zentao/stats'
    const [dashRes, zenRes] = await Promise.all([
      fetch('/api/dashboard'),
      fetch(zenUrl)
    ])
    const dash = await dashRes.json()
    const zen = await zenRes.json()
    stats.value = dash.stats || {}
    agents.value = dash.agents || []
    recent.value = dash.recent || []
    zentao.value = zen
    try {
      const depRes = await fetch('/api/deploy-status')
      deploy.value = await depRes.json()
    } catch {}
  } catch {}
}

async function refreshAll() {
  refreshing.value = true
  await fetchAll(true)
  refreshing.value = false
}

onMounted(() => {
  fetchAll()
  pollTimer = setInterval(fetchAll, 15000)
})
function zentaoBugUrl(bugId) {
  const id = String(bugId || '').replace('Bug#', '')
  return `https://zentao.gentronhealth.com/index.php?m=bug&f=view&bugID=${id}`
}

async function showDetail(row) {
  detailVisible.value = true
  detailLoading.value = true
  detailData.value = row
  detailTraces.value = []
  try {
    const bugId = row.bug
    // Fetch all traces for this bug
    const [trRes, zenRes] = await Promise.all([
      fetch(`/api/agent/${row.agent}/traces/rt`),
      fetch('/api/zentao/stats')
    ])
    const trData = await trRes.json()
    const zenData = await zenRes.json()
    // Filter traces for this bug
    detailTraces.value = (trData.traces || []).filter(t => t.task_id && t.task_id.includes(bugId))
    // Find bug info from zentao
    detailData.value = { ...row, zentaoBug: (zenData.bugs || []).find(b => String(b.id) === String(bugId)) || null }
  } catch {}
  detailLoading.value = false
}

onUnmounted(() => clearInterval(pollTimer))
</script>

<style scoped>
.coordinator-card {
  background: #1e293b; border: 2px solid #f59e0b; border-radius: 10px;
  padding: 16px 20px; cursor: pointer; transition: all 0.2s;
}
.coordinator-card:hover { border-color: #fbbf24; box-shadow: 0 0 12px rgba(245,158,11,0.2); }
.coordinator-card.working { border-color: #22c55e; box-shadow: 0 0 12px rgba(34,197,94,0.15); }
.stat-card { border-radius: 10px; cursor: pointer; transition: transform 0.15s; border: 1px solid #334155; background: #1e293b; }
.stat-card:hover { transform: translateY(-2px); }
.stat-card.warning { border-left: 3px solid #f59e0b; }
.stat-card.success { border-left: 3px solid #22c55e; }
.stat-card.info { border-left: 3px solid #3b82f6; }

.agent-mini {
  background: #1e293b; border: 1px solid #334155; border-radius: 8px;
  padding: 12px; text-align: center; transition: all 0.2s; cursor: pointer;
}
.agent-mini:hover { border-color: #3b82f6; }
.agent-mini.working { border-color: #22c55e; box-shadow: 0 0 8px rgba(34,197,94,0.15); }
</style>
