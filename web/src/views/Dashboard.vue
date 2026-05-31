<template>
  <div class="dashboard">
    <div class="header">
      <h1>📊 AgentForge 监控面板</h1>
      <div class="header-right">
        <span class="ws-status" :class="wsConnected ? 'online' : 'offline'">
          {{ wsConnected ? '🟢 实时连接' : '🔴 断开' }}
        </span>
        <span class="last-tick" v-if="lastTick">{{ lastTick }}</span>
      </div>
    </div>

    <!-- Stats row -->
    <div class="stats-grid">
      <router-link to="/bugs/unclosed" class="stat-card clickable"><div class="stat-value">{{ zentao.unclosed || 0 }}</div><div class="stat-label">未关闭 Bug 🔍</div></router-link>
      <router-link to="/bugs/unresolved" class="stat-card warning-border clickable"><div class="stat-value">{{ zentao.unresolved || 0 }}</div><div class="stat-label">未解决 Bug 🔍</div></router-link>
      <router-link to="/bugs/fixed_today" class="stat-card success clickable"><div class="stat-value">{{ stats.fixed_today || 0 }}</div><div class="stat-label">今日修复 🔍</div></router-link>
      <div class="stat-card info"><div class="stat-value">{{ stats.running || 0 }}</div><div class="stat-label">运行 Agent</div></div>
    </div>
    <div style="font-size:11px;color:#64748b;margin:-16px 0 20px;text-align:right" v-if="zentao.last_sync">
      禅道同步: {{ zentao.last_sync }} · 活跃: {{ zentao.active || 0 }} · 总计: {{ zentao.total || 0 }}
    </div>

    <!-- Agent cards -->
    <div class="section">
      <h2>🤖 智能体状态</h2>
      <div class="agent-grid">
        <router-link v-for="a in agents" :key="a.id" :to="'/agent/' + a.id" class="agent-card" :class="a.status" tag="div">
          <div class="agent-head">
            <span class="agent-icon">{{ a.icon }}</span>
            <span class="agent-name">{{ a.name }}</span>
            <span class="status-dot" :class="a.status"></span>
          </div>
          <div class="agent-role">{{ a.role }}</div>
          <div class="agent-meta">
            <span v-if="a.current_bug" class="current-bug">🔧 {{ a.current_bug }}</span>
            <span v-else class="idle-text">空闲</span>
          </div>
          <div class="agent-stats-row">
            <span>{{ a.rate }}</span>
            <span>{{ a.avg_s }}</span>
          </div>
          <div class="bar"><div class="bar-fill" :style="{width: a.rate}"></div></div>
        </router-link>
      </div>
    </div>

    <!-- Queue + Dispatcher -->
    <div class="two-col">
      <div class="section">
        <h2>📋 触发队列</h2>
        <div class="queue-scroll" ref="queueRef">
          <div v-if="queue.length === 0" class="empty">队列为空</div>
          <div v-for="(q, i) in queue" :key="i" class="queue-item">
            <span class="q-bug">#{{ q.bug_id }}</span>
            <span class="q-agent">{{ q.agent }}</span>
            <span class="q-source">{{ q.source }}</span>
          </div>
        </div>
      </div>
      <div class="section">
        <h2>⚡ Dispatcher</h2>
        <div class="dispatcher-box">
          <div class="d-row"><span class="d-label">模式</span><span class="d-value">{{ dispatcher.mode || 'N/A' }}</span></div>
          <div class="d-row"><span class="d-label">活跃任务</span><span class="d-value">{{ dispatcher.active_tasks || 0 }}</span></div>
          <div class="d-row"><span class="d-label">Redis 队列</span><span class="d-value">{{ dispatcher.redis_queues || 0 }}</span></div>
        </div>
        <h2 style="margin-top:20px">📨 飞书触发</h2>
        <div class="feishu-box">
          <div v-for="(f, i) in feishuEvents" :key="i" class="feishu-item">
            <span class="f-time">{{ f.time }}</span>
            <span class="f-msg">{{ f.message }}</span>
          </div>
          <div v-if="feishuEvents.length === 0" class="empty">暂无触发记录</div>
        </div>
      </div>
    </div>

    <!-- Recent fixes -->
    <div class="section">
      <h2>📝 最近修复</h2>
      <table class="fix-table">
        <thead><tr><th>Bug</th><th>Agent</th><th>状态</th><th>耗时</th><th>时间</th></tr></thead>
        <tbody>
          <tr v-for="f in recent" :key="f.bug + f.ts">
            <td class="mono">#{{ f.bug }}</td>
            <td>{{ f.agent }}</td>
            <td><span class="badge" :class="f.ok ? 'ok' : 'fail'">{{ f.ok ? '✅' : '❌' }}</span></td>
            <td>{{ f.dur }}</td>
            <td class="ts">{{ f.ts }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted, nextTick } from 'vue'

const stats = ref({})
const zentao = ref({})
const agents = ref([])
const recent = ref([])
const queue = ref([])
const dispatcher = ref({})
const feishuEvents = ref([])
const wsConnected = ref(false)
const lastTick = ref('')
const queueRef = ref(null)

let ws = null
let pollTimer = null

async function fetchZentao() {
  try {
    const r = await fetch('/api/zentao/stats')
    zentao.value = await r.json()
  } catch {}
}



async function fetchDashboard() {
  try {
    const r = await fetch('/api/dashboard')
    const d = await r.json()
    stats.value = d.stats || {}
    agents.value = d.agents || []
    recent.value = d.recent || []
    queue.value = d.queue || []
    dispatcher.value = d.dispatcher || {}
    await nextTick()
    if (queueRef.value) queueRef.value.scrollTop = queueRef.value.scrollHeight
  } catch {}
}

function connectWs() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  ws = new WebSocket(`${proto}://${location.host}/ws`)
  ws.onopen = () => { wsConnected.value = true }
  ws.onclose = () => { wsConnected.value = false; setTimeout(connectWs, 3000) }
  ws.onmessage = (e) => {
    try {
      const msg = JSON.parse(e.data)
      if (msg.event === 'tick') {
        lastTick.value = msg.data.ts
        stats.value.running = msg.data.agents
      } else if (msg.event === 'fix_done') {
        fetchDashboard()
      } else if (msg.event === 'feishu_trigger') {
        feishuEvents.value.unshift({ time: msg.data.ts, message: msg.data.message })
        if (feishuEvents.value.length > 20) feishuEvents.value.pop()
      }
    } catch {}
  }
}

onMounted(() => {
  fetchDashboard()
  fetchZentao()
  connectWs()
  pollTimer = setInterval(() => { fetchDashboard(); fetchZentao(); }, 15000)
})

onUnmounted(() => {
  if (ws) ws.close()
  clearInterval(pollTimer)
})
</script>

<style scoped>
.dashboard { padding: 0; }
.header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px; }
.header h1 { font-size: 22px; }
.header-right { display: flex; align-items: center; gap: 12px; }
.ws-status { font-size: 12px; padding: 4px 10px; border-radius: 12px; }
.ws-status.online { background: #052e16; color: #22c55e; }
.ws-status.offline { background: #450a0a; color: #ef4444; }
.last-tick { font-size: 12px; color: #64748b; font-family: monospace; }

.stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin-bottom: 24px; }
.stat-card { background: #1e293b; border-radius: 10px; padding: 16px; border: 1px solid #334155; }
.stat-card.success { border-left: 3px solid #22c55e; }
.stat-card.warning { border-left: 3px solid #f59e0b; }
.stat-card.info { border-left: 3px solid #3b82f6; }
.stat-card.warning-border { border-left: 3px solid #f59e0b; }
.stat-value { font-size: 28px; font-weight: 700; }
.stat-label { font-size: 12px; color: #94a3b8; margin-top: 2px; }

.section { margin-bottom: 24px; }
.section h2 { font-size: 15px; margin-bottom: 12px; color: #cbd5e1; }

.agent-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
.agent-card { background: #1e293b; border-radius: 10px; padding: 14px; border: 1px solid #334155; transition: all 0.2s; cursor: pointer; text-decoration: none; color: inherit; display: block; }
.agent-card.working { border-color: #22c55e; box-shadow: 0 0 12px rgba(34,197,94,0.1); }
.agent-head { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
.agent-icon { font-size: 18px; }
.agent-name { font-weight: 600; font-size: 13px; }
.status-dot { width: 8px; height: 8px; border-radius: 50%; margin-left: auto; }
.status-dot.working { background: #22c55e; animation: pulse 1.5s infinite; }
.status-dot.idle { background: #475569; }
@keyframes pulse { 0%,100% { opacity:1; } 50% { opacity:0.4; } }
.agent-role { font-size: 11px; color: #64748b; margin-bottom: 6px; }
.agent-meta { font-size: 11px; margin-bottom: 6px; min-height: 16px; }
.current-bug { color: #22c55e; }
.idle-text { color: #475569; }
.agent-stats-row { display: flex; justify-content: space-between; font-size: 11px; color: #94a3b8; margin-bottom: 4px; }
.bar { height: 3px; background: #334155; border-radius: 2px; }
.bar-fill { height: 100%; background: linear-gradient(90deg, #3b82f6, #22c55e); border-radius: 2px; }

.two-col { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-bottom: 24px; }

.queue-scroll { background: #1e293b; border: 1px solid #334155; border-radius: 10px; max-height: 200px; overflow-y: auto; }
.queue-item { display: flex; gap: 12px; padding: 8px 14px; border-bottom: 1px solid #1e293b; font-size: 13px; }
.queue-item:nth-child(even) { background: #0f172a; }
.q-bug { color: #60a5fa; font-family: monospace; width: 50px; }
.q-agent { color: #f59e0b; width: 60px; }
.q-source { color: #64748b; }

.dispatcher-box, .feishu-box { background: #1e293b; border: 1px solid #334155; border-radius: 10px; padding: 14px; }
.d-row { display: flex; justify-content: space-between; padding: 6px 0; border-bottom: 1px solid #334155; font-size: 13px; }
.d-label { color: #64748b; }
.d-value { color: #e2e8f0; font-weight: 500; }
.feishu-item { display: flex; gap: 10px; padding: 6px 0; font-size: 12px; border-bottom: 1px solid #334155; }
.f-time { color: #64748b; font-family: monospace; width: 60px; }
.f-msg { color: #94a3b8; }
.empty { color: #475569; font-size: 13px; padding: 12px; text-align: center; }

.fix-table { width: 100%; border-collapse: collapse; }
.fix-table th, .fix-table td { padding: 8px 12px; text-align: left; border-bottom: 1px solid #334155; font-size: 13px; }
.fix-table th { color: #64748b; font-weight: 500; }
.mono { font-family: monospace; color: #60a5fa; }
.ts { color: #64748b; font-size: 11px; }
.badge { padding: 2px 6px; border-radius: 4px; font-size: 11px; }
.badge.ok { background: #052e16; }
.badge.fail { background: #450a0a; }

.clickable { cursor: pointer; text-decoration: none; color: inherit; display: block; transition: transform 0.15s, box-shadow 0.15s; }
.clickable:hover { transform: translateY(-2px); box-shadow: 0 4px 12px rgba(0,0,0,0.3); }
</style>
