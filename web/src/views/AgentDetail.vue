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
        <el-card shadow="never" body-style="padding:16px;text-align:center">
          <el-statistic title="成功率" :value="successRate" :suffix="'%'">
            <template #prefix><span :style="{color: successRate > 50 ? '#22c55e' : '#ef4444'}">▸</span></template>
          </el-statistic>
        </el-card>
      </el-col>
      <el-col :span="6">
        <el-card shadow="never" body-style="padding:16px;text-align:center">
          <el-statistic title="成功修复" :value="successCount" />
        </el-card>
      </el-col>
      <el-col :span="6">
        <el-card shadow="never" body-style="padding:16px;text-align:center">
          <el-statistic title="失败" :value="failCount" />
        </el-card>
      </el-col>
      <el-col :span="6">
        <el-card shadow="never" body-style="padding:16px;text-align:center">
          <el-statistic title="平均耗时" :value="agentInfo?.avg_s || '-'" />
        </el-card>
      </el-col>
    </el-row>

    <el-card shadow="never" style="margin-bottom:16px" v-if="queueInfo && queueInfo.queue_len > 0">
      <template #header>📋 当前队列 <el-tag type="primary" size="small">{{ queueInfo.queue_len }}</el-tag></template>
      <el-table :data="queueInfo.items" stripe size="small">
        <el-table-column prop="bug_id" label="Bug" width="80">
          <template #default="{row}"><span style="color:#60a5fa;font-family:monospace">#{{ row.bug_id }}</span></template>
        </el-table-column>
        <el-table-column prop="source" label="来源" />
      </el-table>
    </el-card>

    <el-card shadow="never">
      <template #header>
        <span style="display:flex;align-items:center;gap:6px">
          <span class="live-dot"></span>最近活动 (实时)
        </span>
      </template>
      <el-table :data="traces" stripe style="width:100%" max-height="500">
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
        <el-table-column prop="task_id" label="Bug" width="100">
          <template #default="{row}"><span style="color:#60a5fa;font-family:monospace">{{ row.task_id || '-' }}</span></template>
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
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRoute } from 'vue-router'
import { ArrowLeft } from '@element-plus/icons-vue'

const route = useRoute()
const agentId = computed(() => route.params.id)
const traces = ref([])
const agentInfo = ref(null)
const queueInfo = ref(null)
let pollTimer = null

const successCount = computed(() => traces.value.filter(t => t.event === 'fix_done' && t.status === 'ok').length)
const failCount = computed(() => traces.value.filter(t => t.event === 'fix_done' && t.status !== 'ok').length)
const totalFixes = computed(() => successCount.value + failCount.value)
const successRate = computed(() => totalFixes.value > 0 ? Math.round(successCount.value / totalFixes.value * 100) : 0)

const agentIdMap = { '\u5173\u7fbd':'guanyu','\u8d75\u4e91':'zhaoyun','\u8359\u5f55':'xunyu','\u5f20\u98de':'zhangfei','\u534e\u4f6e':'huatuo','\u9648\u7433':'chenlin','\u5218\u5907':'liubei','\u8bf8\u845b\u4eae':'zhugeliang' }
const agentData = {
  guanyu: { icon: '⚔️', name: '关羽', role: '后端开发' }, zhaoyun: { icon: '🐉', name: '赵云', role: '前端开发' },
  xunyu: { icon: '📚', name: '荀彧', role: 'DBA' }, zhangfei: { icon: '🔥', name: '张飞', role: '测试' },
  huatuo: { icon: '💊', name: '华佗', role: '产品' }, chenlin: { icon: '📝', name: '陈琳', role: '文档' },
  liubei: { icon: '👑', name: '刘备', role: '项目管理' }, zhugeliang: { icon: '🪶', name: '诸葛亮', role: '架构' },
}

function formatTime(ts) { return ts ? ts.substring(11, 19) : '-' }
function eventLabel(e) { return { fix_start: '🚀 开始', fix_done: '✅ 完成', error: '❌ 错误', fix_retry: '🔄 重试' }[e] || e }

async function refresh() {
  try {
    const [trRes, dashRes, qRes] = await Promise.all([
      fetch(`/api/agent/${agentId.value}/traces`),
      fetch('/api/dashboard'),
      fetch('/api/queues')
    ])
    traces.value = (await trRes.json()).traces || []
    const dash = await dashRes.json()
    agentInfo.value = dash.agents?.find(a => a.id === agentId.value) || agentData[agentId.value] || null
    const queues = await qRes.json()
    queueInfo.value = queues.find(q => q.agent === agentId.value) || null
  } catch {}
}

onMounted(() => { refresh(); pollTimer = setInterval(refresh, 5000) })
onUnmounted(() => clearInterval(pollTimer))
</script>

<style scoped>
.live-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #22c55e; animation: pulse 1.5s infinite; }
@keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.3; } }
</style>
