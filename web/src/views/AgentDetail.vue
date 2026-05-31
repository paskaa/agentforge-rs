<template>
  <div class="agent-detail-page">
    <div class="detail-header">
      <router-link to="/" class="back-link">← 返回仪表盘</router-link>
      <div class="agent-title">
        <span class="title-icon">{{ agentInfo?.icon || '🤖' }}</span>
        <div>
          <h1>{{ agentInfo?.name || agentId }}</h1>
          <div class="title-role">{{ agentInfo?.role || '' }}</div>
        </div>
        <span class="status-badge" :class="agentInfo?.status || 'idle'">
          {{ agentInfo?.status === 'working' ? '🔄 工作中' : '💤 空闲' }}
        </span>
      </div>
    </div>

    <!-- Stats cards -->
    <div class="stats-row">
      <div class="stat-card">
        <div class="stat-value" :class="successRate > 50 ? 'green' : 'red'">{{ successRate }}%</div>
        <div class="stat-label">成功率</div>
      </div>
      <div class="stat-card green-border">
        <div class="stat-value green">{{ successCount }}</div>
        <div class="stat-label">成功修复</div>
      </div>
      <div class="stat-card red-border">
        <div class="stat-value red">{{ failCount }}</div>
        <div class="stat-label">失败</div>
      </div>
      <div class="stat-card blue-border">
        <div class="stat-value blue">{{ agentInfo?.avg_s || '-' }}</div>
        <div class="stat-label">平均耗时</div>
      </div>
    </div>

    <!-- Queue info -->
    <div class="section" v-if="queueInfo">
      <h2>📋 当前队列 <span class="q-count">{{ queueInfo.queue_len }}</span></h2>
      <div class="queue-panel" v-if="queueInfo.items.length > 0">
        <div v-for="(item, i) in queueInfo.items" :key="i" class="queue-row">
          <span class="q-bug">#{{ item.bug_id }}</span>
          <span class="q-source">{{ item.source }}</span>
        </div>
      </div>
      <div class="empty" v-else>队列为空</div>
    </div>

    <!-- Live traces -->
    <div class="section">
      <h2><span class="live-dot"></span>最近活动 (实时)</h2>
      <div class="traces-panel">
        <table class="traces-table">
          <thead>
            <tr>
              <th>时间</th>
              <th>事件</th>
              <th>Bug</th>
              <th>状态</th>
              <th>耗时</th>
              <th>详情</th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="traces.length === 0">
              <td colspan="6" class="empty-row">暂无活动记录</td>
            </tr>
            <tr v-for="(t, i) in traces" :key="i">
              <td class="ts">{{ formatTime(t.ts) }}</td>
              <td>
                <span :class="eventClass(t.event)">{{ eventLabel(t.event) }}</span>
              </td>
              <td class="mono">{{ t.task_id || '-' }}</td>
              <td>
                <span :class="'status-' + (t.status || 'pending')">{{ statusLabel(t.status) }}</span>
              </td>
              <td>{{ t.duration_ms > 0 ? (t.duration_ms / 1000).toFixed(0) + 's' : '-' }}</td>
              <td class="msg-cell" :title="t.message">{{ t.message || '-' }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted, computed } from 'vue'
import { useRoute } from 'vue-router'

const route = useRoute()
const agentId = computed(() => route.params.id)
const traces = ref([])
const agentInfo = ref(null)
const queueInfo = ref(null)
let pollTimer = null

const agentData = {
  guanyu: { icon: '⚔️', name: '关羽', role: '后端开发', avg_s: '-' },
  zhaoyun: { icon: '🐉', name: '赵云', role: '前端开发', avg_s: '-' },
  xunyu: { icon: '📚', name: '荀彧', role: 'DBA', avg_s: '-' },
  zhangfei: { icon: '🔥', name: '张飞', role: '测试', avg_s: '-' },
  huatuo: { icon: '💊', name: '华佗', role: '产品', avg_s: '-' },
  chenlin: { icon: '📝', name: '陈琳', role: '文档', avg_s: '-' },
  liubei: { icon: '👑', name: '刘备', role: '项目管理', avg_s: '-' },
  zhugeliang: { icon: '🪶', name: '诸葛亮', role: '架构', avg_s: '-' },
}

const successCount = computed(() => traces.value.filter(t => t.event === 'fix_done' && t.status === 'ok').length)
const failCount = computed(() => traces.value.filter(t => t.event === 'fix_done' && t.status !== 'ok').length)
const totalFixes = computed(() => successCount.value + failCount.value)
const successRate = computed(() => totalFixes.value > 0 ? Math.round(successCount.value / totalFixes.value * 100) : 0)

function formatTime(ts) {
  if (!ts) return '-'
  return ts.length > 19 ? ts.substring(11, 19) : ts
}

function eventLabel(e) {
  const map = { fix_start: '🚀 开始', fix_done: '✅ 完成', error: '❌ 错误', task_start: '📋 任务', pm_routed: '📤 分派', feishu_reply: '📨 飞书' }
  return map[e] || e
}

function eventClass(e) {
  if (e === 'fix_done') return 'ev-success'
  if (e === 'error') return 'ev-error'
  return 'ev-pending'
}

function statusLabel(s) {
  const map = { ok: '✅', error: '❌', pending: '⏳', raw: '📝' }
  return map[s] || s || '-'
}

async function fetchTraces() {
  try {
    const r = await fetch(`/api/agent/${agentId.value}/traces`)
    const d = await r.json()
    traces.value = d.traces || []
  } catch {}
}

async function fetchQueue() {
  try {
    const r = await fetch('/api/queues')
    const queues = await r.json()
    queueInfo.value = queues.find(q => q.agent === agentId.value) || { queue_len: 0, items: [] }
  } catch {}
}

async function fetchAgentInfo() {
  try {
    const r = await fetch('/api/dashboard')
    const d = await r.json()
    agentInfo.value = d.agents?.find(a => a.id === agentId.value) || agentData[agentId.value] || null
  } catch {
    agentInfo.value = agentData[agentId.value] || null
  }
}

async function refresh() {
  await Promise.all([fetchTraces(), fetchQueue(), fetchAgentInfo()])
}

onMounted(() => {
  refresh()
  pollTimer = setInterval(refresh, 5000)
})

onUnmounted(() => {
  clearInterval(pollTimer)
})
</script>

<style scoped>
.agent-detail-page { max-width: 1000px; }

.detail-header { margin-bottom: 24px; }
.back-link {
  display: inline-block; margin-bottom: 12px; color: #64748b; text-decoration: none;
  font-size: 13px; padding: 4px 8px; border-radius: 4px; transition: all 0.15s;
}
.back-link:hover { background: #334155; color: #e2e8f0; }

.agent-title { display: flex; align-items: center; gap: 12px; }
.title-icon { font-size: 36px; }
.agent-title h1 { font-size: 22px; font-weight: 700; }
.title-role { font-size: 13px; color: #94a3b8; }
.status-badge {
  margin-left: auto; padding: 4px 12px; border-radius: 12px; font-size: 12px; font-weight: 500;
}
.status-badge.working { background: #052e16; color: #22c55e; }
.status-badge.idle { background: #1e293b; color: #64748b; }

.stats-row { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin-bottom: 24px; }
.stat-card { background: #1e293b; border: 1px solid #334155; border-radius: 10px; padding: 16px; text-align: center; }
.stat-card.green-border { border-left: 3px solid #22c55e; }
.stat-card.red-border { border-left: 3px solid #ef4444; }
.stat-card.blue-border { border-left: 3px solid #3b82f6; }
.stat-value { font-size: 28px; font-weight: 700; }
.stat-value.green { color: #22c55e; }
.stat-value.red { color: #ef4444; }
.stat-value.blue { color: #3b82f6; }
.stat-label { font-size: 12px; color: #94a3b8; margin-top: 4px; }

.section { margin-bottom: 24px; }
.section h2 { font-size: 15px; margin-bottom: 12px; color: #cbd5e1; display: flex; align-items: center; gap: 8px; }
.q-count { background: #3b82f6; color: white; padding: 1px 8px; border-radius: 10px; font-size: 12px; }

.queue-panel { background: #1e293b; border: 1px solid #334155; border-radius: 10px; overflow: hidden; }
.queue-row { display: flex; gap: 12px; padding: 10px 14px; border-bottom: 1px solid #334155; font-size: 13px; }
.queue-row:last-child { border-bottom: none; }
.q-bug { color: #60a5fa; font-family: monospace; font-weight: 500; }
.q-source { color: #64748b; }

.live-dot {
  display: inline-block; width: 8px; height: 8px; border-radius: 50%;
  background: #22c55e; animation: pulse 1.5s infinite;
}
@keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.3; } }

.traces-panel { background: #1e293b; border: 1px solid #334155; border-radius: 10px; overflow: hidden; }
.traces-table { width: 100%; border-collapse: collapse; }
.traces-table th {
  text-align: left; padding: 10px 14px; font-size: 11px; color: #64748b;
  text-transform: uppercase; letter-spacing: 0.5px; border-bottom: 1px solid #334155;
  background: rgba(15,23,42,0.5);
}
.traces-table td { padding: 8px 14px; font-size: 13px; border-bottom: 1px solid #1e293b; }
.traces-table tr:hover { background: rgba(59,130,246,0.05); }
.empty-row { text-align: center; color: #475569; padding: 30px !important; }

.ts { color: #64748b; font-family: monospace; white-space: nowrap; }
.mono { font-family: monospace; color: #60a5fa; }
.msg-cell { max-width: 250px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #94a3b8; }

.ev-success { color: #22c55e; }
.ev-error { color: #ef4444; }
.ev-pending { color: #f59e0b; }

.status-ok { color: #22c55e; }
.status-error { color: #ef4444; }
.status-pending { color: #f59e0b; }

.empty { color: #475569; font-size: 13px; padding: 16px; text-align: center; }
</style>
