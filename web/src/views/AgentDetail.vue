<template>
  <div>
    <div style="display:flex;align-items:center;gap:12px;margin-bottom:16px">
      <router-link to="/"><el-button size="small" :icon="ArrowLeft">返回</el-button></router-link>
      <span style="font-size:28px">{{ agentInfo?.icon || '🤖' }}</span>
      <div>
        <h1 style="font-size:20px">{{ agentInfo?.name || agentId }}</h1>
        <div style="color:#64748b;font-size:13px">{{ agentInfo?.role || '' }}</div>
      </div>
      <el-tag :type="agentInfo?.status === 'working' ? 'success' : 'info'" style="margin-left:auto" effect="dark">
        {{ agentInfo?.status === 'working' ? '🔄 工作中' : '💤 空闲' }}
      </el-tag>
    </div>

    <el-row :gutter="16" style="margin-bottom:20px">
      <el-col :span="6">
        <router-link :to="'/agent/' + agentId" style="text-decoration:none;color:inherit">
          <el-card shadow="hover" body-style="padding:16px;text-align:center;cursor:pointer">
            <el-statistic title="成功率" :value="successRate" :suffix="'%'">
              <template #prefix><span :style="{color: successRate > 50 ? '#22c55e' : '#ef4444'}">▸</span></template>
            </el-statistic>
          </el-card>
        </router-link>
      </el-col>
      <el-col :span="6">
        <el-card shadow="hover" body-style="padding:16px;text-align:center;cursor:pointer">
          <el-statistic title="成功修复" :value="successCount" />
        </el-card>
      </el-col>
      <el-col :span="6">
        <el-card shadow="hover" body-style="padding:16px;text-align:center;cursor:pointer">
          <el-statistic title="失败" :value="failCount" />
        </el-card>
      </el-col>
      <el-col :span="6">
        <el-card shadow="hover" body-style="padding:16px;text-align:center;cursor:pointer">
          <el-statistic title="平均耗时" :value="agentInfo?.avg_s || '-'" />
        </el-card>
      </el-col>
    </el-row>

    <!-- 当前队列 -->
    <el-card shadow="never" style="margin-bottom:16px" v-if="queueInfo && queueInfo.queue_len > 0">
      <template #header>📋 当前队列 <el-tag type="primary" size="small">{{ queueInfo.queue_len }}</el-tag></template>
      <el-table :data="queueInfo.items" stripe size="small">
        <el-table-column label="Bug" width="100">
          <template #default="{row}">
            <a :href="zentaoBugUrl(row.bug_id)" target="_blank" rel="noopener"
              style="color:#60a5fa;font-family:monospace;text-decoration:none">
              #{{ row.bug_id }}
              <el-icon style="font-size:10px;margin-left:2px"><Link /></el-icon>
            </a>
          </template>
        </el-table-column>
        <el-table-column prop="source" label="来源" width="100" />
        <el-table-column prop="queued_at" label="入列时间">
          <template #default="{row}"><span style="color:#64748b;font-size:12px">{{ row.queued_at ? row.queued_at.substring(11, 19) : '-' }}</span></template>
        </el-table-column>
      </el-table>
    </el-card>

    <!-- 实时日志 WebSocket -->
    <el-card shadow="never" style="margin-bottom:16px">
      <template #header>
        <span style="display:flex;align-items:center;gap:8px">
          <span class="live-dot"></span>实时日志
          <el-tag :type="wsConnected ? 'success' : 'danger'" size="small" effect="dark">
            {{ wsConnected ? '已连接' : '断开' }}
          </el-tag>
          <span style="color:#475569;font-size:11px;margin-left:auto">{{ wsStatus }}</span>
        </span>
      </template>
      <div ref="logContainer" class="log-container">
        <div v-for="(log, i) in realtimeLogs" :key="i" class="log-line">
          <span class="log-time">{{ log.time }}</span>
          <el-tag :type="logType(log.event)" size="small" effect="plain" class="log-tag">{{ log.event }}</el-tag>
          <span class="log-bug" v-if="log.bug_id">
            <a :href="zentaoBugUrl(log.bug_id)" target="_blank" style="color:#60a5fa;text-decoration:none">#{{ log.bug_id }}</a>
          </span>
          <span class="log-status" v-if="log.status">
            <el-tag :type="log.status === 'ok' ? 'success' : 'danger'" size="small" effect="dark">{{ log.status }}</el-tag>
          </span>
          <span class="log-dur" v-if="log.duration_ms > 0">{{ (log.duration_ms / 1000).toFixed(0) }}s</span>
          <span class="log-msg" :title="log.message">{{ log.message }}</span>
        </div>
        <div v-if="realtimeLogs.length === 0" style="text-align:center;color:#475569;padding:20px">等待实时数据...</div>
      </div>
    </el-card>

    <!-- 历史 Traces -->
    <el-card shadow="never">
      <template #header>📜 历史记录 (最近50条)</template>
      <el-table :data="traces" stripe style="width:100%" max-height="500" :default-sort="{ prop: 'ts', order: 'descending' }">
        <el-table-column label="时间" width="80">
          <template #default="{row}"><span style="font-family:monospace;color:#64748b;font-size:12px">{{ formatTime(row.ts) }}</span></template>
        </el-table-column>
        <el-table-column label="事件" width="100">
          <template #default="{row}">
            <el-tag :type="row.event === 'fix_done' ? (row.status === 'ok' ? 'success' : 'danger') : 'warning'" size="small">
              {{ eventLabel(row.event) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="Bug" width="100">
          <template #default="{row}">
            <a v-if="row.task_id && row.task_id !== '?'" :href="zentaoBugUrl(row.task_id)" target="_blank"
              style="color:#60a5fa;font-family:monospace;text-decoration:none;font-size:12px">
              #{{ row.task_id.replace('Bug#','') }}
            </a>
            <span v-else style="color:#475569">-</span>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="80">
          <template #default="{row}">
            <el-tag :type="row.status === 'ok' ? 'success' : row.status === 'pending' ? 'warning' : 'danger'" size="small">{{ row.status || '-' }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="耗时" width="80">
          <template #default="{row}">{{ row.duration_ms > 0 ? (row.duration_ms / 1000).toFixed(0) + 's' : '-' }}</template>
        </el-table-column>
        <el-table-column prop="message" label="详情" show-overflow-tooltip />
      </el-table>
      <el-empty v-if="traces.length === 0" description="暂无活动记录" :image-size="60" />
    </el-card>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { useRoute } from 'vue-router'
import { ArrowLeft, Link } from '@element-plus/icons-vue'

const route = useRoute()
const agentId = computed(() => route.params.id)
const traces = ref([])
const agentInfo = ref(null)
const queueInfo = ref(null)
const realtimeLogs = ref([])
const wsConnected = ref(false)
const wsStatus = ref('')
const logContainer = ref(null)
let ws = null
let pollTimer = null
let lastTs = ''

const successCount = computed(() => traces.value.filter(t => t.event === 'fix_done' && t.status === 'ok').length)
const failCount = computed(() => traces.value.filter(t => t.event === 'fix_done' && t.status !== 'ok').length)
const totalFixes = computed(() => successCount.value + failCount.value)
const successRate = computed(() => totalFixes.value > 0 ? Math.round(successCount.value / totalFixes.value * 100) : 0)

const agentData = {
  guanyu: { icon: '⚔️', name: '关羽', role: '后端开发' }, zhaoyun: { icon: '🐉', name: '赵云', role: '前端开发' },
  xunyu: { icon: '📚', name: '荀彧', role: 'DBA' }, zhangfei: { icon: '🔥', name: '张飞', role: '测试' },
  huatuo: { icon: '💊', name: '华佗', role: '产品' }, chenlin: { icon: '📝', name: '陈琳', role: '文档' },
  liubei: { icon: '👑', name: '刘备', role: '项目管理' }, zhugeliang: { icon: '🪶', name: '诸葛亮', role: '架构' },
}

function zentaoBugUrl(bugId) {
  const id = String(bugId || '').replace('Bug#', '')
  return `https://zentao.gentronhealth.com/index.php?m=bug&f=view&bugID=${id}`
}
function formatTime(ts) { return ts ? ts.substring(11, 19) : '-' }
function eventLabel(e) { return { fix_start: '🚀 开始', fix_done: '✅ 完成', fix_retry: '🔄 重试', fix_attempt: '📝 尝试' }[e] || e }
function logType(e) { return { fix_done: 'success', fix_start: 'warning', fix_retry: 'danger', fix_attempt: 'info' }[e] || 'info' }

function scrollLogs() {
  nextTick(() => {
    if (logContainer.value) logContainer.value.scrollTop = 0
  })
}

function connectWs() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  ws = new WebSocket(`${proto}://${location.host}/ws`)
  ws.onopen = () => { wsConnected.value = true; wsStatus.value = '已连接' }
  ws.onclose = () => {
    wsConnected.value = false; wsStatus.value = '重连中...'
    setTimeout(connectWs, 3000)
  }
  ws.onerror = () => { wsConnected.value = false }
  ws.onmessage = (e) => {
    try {
      const msg = JSON.parse(e.data)
      if (msg.event === 'tick' || msg.event === 'init') {
        // Global status, ignore for agent detail
      } else if (msg.event === 'trace' && msg.data?.agent_id === agentId.value) {
        const d = msg.data
        const bugMatch = (d.task_id || '').match(/Bug#(\d+)/)
        realtimeLogs.value.unshift({
          time: d.ts ? d.ts.substring(11, 19) : new Date().toLocaleTimeString(),
          event: d.event || '?',
          bug_id: bugMatch ? bugMatch[1] : '',
          status: d.status || '',
          duration_ms: d.duration_ms || 0,
          message: d.message || '',
        })
        if (realtimeLogs.value.length > 100) realtimeLogs.value.pop()
        scrollLogs()
      }
    } catch {}
  }
}

async function refresh() {
  try {
    const [trRes, dashRes, qRes] = await Promise.all([
      fetch(`/api/agent/${agentId.value}/traces/rt`),
      fetch('/api/dashboard'),
      fetch(`/api/agent/${agentId.value}/queue`)
    ])
    const trData = await trRes.json()
    traces.value = trData.traces || []
    const dash = await dashRes.json()
    agentInfo.value = dash.agents?.find(a => a.id === agentId.value) || agentData[agentId.value] || null
    queueInfo.value = await qRes.json()
  } catch {}
}

onMounted(() => { refresh(); connectWs(); pollTimer = setInterval(refresh, 5000) })
onUnmounted(() => { clearInterval(pollTimer); if (ws) ws.close() })
</script>

<style scoped>
.live-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #22c55e; animation: pulse 1.5s infinite; }
@keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.3; } }
.log-container {
  max-height: 400px;
  min-height: 120px;
  overflow-y: auto;
  font-family: 'Courier New', monospace;
  font-size: 12px;
  background: #0f172a;
  border-radius: 6px;
  padding: 8px 12px;
  border: 1px solid #1e293b;
}
.log-line {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 4px 0;
  border-bottom: 1px solid rgba(51,65,85,0.3);
  min-height: 24px;
  flex-wrap: wrap;
}
.log-time { color: #94a3b8; min-width: 70px; font-size: 11px; white-space: nowrap; }
.log-tag { min-width: 60px; text-align: center; flex-shrink: 0; }
.log-bug { min-width: 50px; flex-shrink: 0; }
.log-status { flex-shrink: 0; }
.log-dur { color: #94a3b8; min-width: 50px; flex-shrink: 0; }
.log-msg {
  color: #cbd5e1;
  flex: 1;
  min-width: 0;
  word-break: break-all;
  white-space: pre-wrap;
  max-height: 60px;
  overflow-y: auto;
  line-height: 1.4;
}
</style>
