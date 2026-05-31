<template>
  <div class="queues-page">
    <h1>📋 队列状态</h1>
    <div class="summary">
      共 <strong>{{ totalItems }}</strong> 个待处理任务
    </div>

    <div v-for="q in queues" :key="q.agent" class="queue-panel">
      <div class="queue-header">
        <span class="q-icon">{{ agentIcon(q.agent) }}</span>
        <span class="q-name">{{ agentName(q.agent) }}</span>
        <span class="q-badge" :class="q.queue_len > 0 ? 'has-items' : 'empty'">{{ q.queue_len }}</span>
        <router-link :to="'/agent/' + q.agent" class="q-link">查看活动 →</router-link>
      </div>
      <div v-if="q.items.length > 0" class="queue-items">
        <div v-for="(item, i) in q.items" :key="i" class="queue-row">
          <span class="item-bug">#{{ item.bug_id }}</span>
          <span class="item-source">{{ item.source || 'pipeline' }}</span>
        </div>
      </div>
      <div v-else class="queue-empty">队列为空</div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'

const queues = ref([])
let pollTimer = null

const totalItems = computed(() => queues.value.reduce((s, q) => s + q.queue_len, 0))

const agentIcons = { guanyu: '⚔️', zhaoyun: '🐉', xunyu: '📚', zhangfei: '🔥', huatuo: '💊', chenlin: '📝', liubei: '👑', zhugeliang: '🪶' }
const agentNames = { guanyu: '关羽', zhaoyun: '赵云', xunyu: '荀彧', zhangfei: '张飞', huatuo: '华佗', chenlin: '陈琳', liubei: '刘备', zhugeliang: '诸葛亮' }

function agentIcon(id) { return agentIcons[id] || '🤖' }
function agentName(id) { return agentNames[id] || id }

async function fetchQueues() {
  try {
    const r = await fetch('/api/queues')
    queues.value = await r.json()
  } catch {}
}

onMounted(() => {
  fetchQueues()
  pollTimer = setInterval(fetchQueues, 5000)
})

onUnmounted(() => clearInterval(pollTimer))
</script>

<style scoped>
.queues-page h1 { font-size: 22px; margin-bottom: 8px; }
.summary { color: #94a3b8; font-size: 14px; margin-bottom: 24px; }
.summary strong { color: #60a5fa; font-size: 18px; }

.queue-panel { background: #1e293b; border: 1px solid #334155; border-radius: 10px; margin-bottom: 12px; overflow: hidden; }
.queue-header {
  display: flex; align-items: center; gap: 10px; padding: 12px 16px;
  border-bottom: 1px solid #334155;
}
.q-icon { font-size: 20px; }
.q-name { font-weight: 600; font-size: 14px; }
.q-badge {
  padding: 2px 8px; border-radius: 10px; font-size: 11px; font-weight: 600;
}
.q-badge.has-items { background: #3b82f6; color: white; }
.q-badge.empty { background: #334155; color: #64748b; }
.q-link { margin-left: auto; font-size: 12px; color: #60a5fa; text-decoration: none; }
.q-link:hover { text-decoration: underline; }

.queue-items { }
.queue-row {
  display: flex; gap: 12px; padding: 8px 16px; border-bottom: 1px solid #1e293b;
  font-size: 13px;
}
.queue-row:nth-child(even) { background: rgba(15,23,42,0.3); }
.item-bug { color: #60a5fa; font-family: monospace; font-weight: 500; min-width: 50px; }
.item-source { color: #64748b; }

.queue-empty { padding: 12px 16px; color: #475569; font-size: 13px; text-align: center; }
</style>
