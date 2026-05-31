<template>
  <div class="pipeline-progress">
    <div v-for="(node, i) in nodes" :key="node.key" class="pipeline-node" :class="node.status">
      <div class="node-dot" :class="node.status">
        <span v-if="node.status === 'done'">✓</span>
        <span v-else-if="node.status === 'active'">●</span>
        <span v-else-if="node.status === 'failed'">✗</span>
        <span v-else>○</span>
      </div>
      <div class="node-info">
        <div class="node-label">{{ node.label }}</div>
        <div v-if="node.detail" class="node-detail">{{ node.detail }}</div>
      </div>
      <div v-if="i < nodes.length - 1" class="node-line" :class="node.status === 'done' ? 'done' : ''"></div>
    </div>
  </div>
</template>

<script setup>
import { ref, watch, onMounted } from 'vue'

const props = defineProps({ bugId: { type: String, required: true } })
const nodes = ref([])

// 拆分后的处理节点
const PIPELINE_STEPS = [
  { key: 'pipeline_assign',  label: '分配',   event: 'pipeline_assign',  group: 'dispatch' },
  { key: 'fix_start',        label: '分析',   event: 'fix_start',        group: 'fix' },
  { key: 'fix_attempt',      label: '尝试',   event: 'fix_attempt',      group: 'fix' },
  { key: 'llm_call',         label: 'LLM',    event: 'llm_call',         group: 'fix' },
  { key: 'llm_done',         label: '生成',   event: 'llm_done',         group: 'fix' },
  { key: 'fix_retry',        label: '重试',   event: 'fix_retry',        group: 'fix' },
  { key: 'fix_done',         label: '完成',   event: 'fix_done',         group: 'fix' },
  { key: 'test_done',        label: '测试',   event: 'test_done',        group: 'verify' },
  { key: 'verify_start',     label: '验证',   event: 'verify_start',     group: 'verify' },
  { key: 'verify_diff',      label: 'Diff',   event: 'verify_diff',      group: 'verify' },
  { key: 'verify_done',      label: '验收',   event: 'verify_done',      group: 'verify' },
  { key: 'doc_done',         label: '归档',   event: 'doc_done',         group: 'archive' },
  { key: 'resolve',          label: '解决',   event: 'resolve',          group: 'archive' },
]

function formatDuration(ms) {
  if (!ms || ms <= 0) return ''
  if (ms < 1000) return ms + 'ms'
  return (ms / 1000).toFixed(0) + 's'
}

async function loadTraces() {
  try {
    const res = await fetch(`/api/bugs/${props.bugId}/traces`)
    const data = await res.json()
    const traces = data.traces || []

    // 统计每个 event 出现的次数和最后一次的状态
    const eventMap = {}
    for (const t of traces) {
      const ev = t.event
      if (!eventMap[ev]) eventMap[ev] = { count: 0, lastStatus: '', lastDuration: 0, lastTs: '' }
      eventMap[ev].count++
      eventMap[ev].lastStatus = t.status || ''
      eventMap[ev].lastDuration = t.duration_ms || 0
      eventMap[ev].lastTs = t.ts || ''
    }

    // 找到最后一个有事件的步骤
    const lastIdx = (() => {
      for (let i = PIPELINE_STEPS.length - 1; i >= 0; i--) {
        if (eventMap[PIPELINE_STEPS[i].event]) return i
      }
      return -1
    })()

    nodes.value = PIPELINE_STEPS.map((step, idx) => {
      const ev = eventMap[step.event]
      let status = 'pending'
      let detail = ''

      if (ev) {
        if (idx === lastIdx) {
          // 最后一个有事件的步骤
          status = ev.lastStatus === 'ok' ? 'done' : ev.lastStatus === 'failed' ? 'failed' : 'active'
        } else if (idx < lastIdx) {
          status = 'done'
        }

        // 显示次数和耗时
        if (ev.count > 1) detail = `${ev.count}次`
        if (ev.lastDuration > 0) detail += (detail ? ' ' : '') + formatDuration(ev.lastDuration)
      }

      return { ...step, status, detail }
    })
  } catch {
    nodes.value = PIPELINE_STEPS.map(s => ({ ...s, status: 'pending', detail: '' }))
  }
}

onMounted(loadTraces)
watch(() => props.bugId, loadTraces)
</script>

<style scoped>
.pipeline-progress { display: flex; align-items: center; gap: 0; padding: 6px 0; overflow-x: auto; }
.pipeline-node { display: flex; align-items: center; gap: 0; flex-shrink: 0; }
.node-dot { width: 22px; height: 22px; border-radius: 50%; display: flex; align-items: center; justify-content: center; font-size: 10px; font-weight: 700; border: 2px solid #334155; background: #1e293b; color: #475569; transition: all 0.3s; }
.node-dot.done { border-color: #22c55e; background: rgba(34,197,94,0.15); color: #22c55e; }
.node-dot.active { border-color: #3b82f6; background: rgba(59,130,246,0.15); color: #3b82f6; animation: pulse 1.5s infinite; }
.node-dot.failed { border-color: #ef4444; background: rgba(239,68,68,0.15); color: #ef4444; }
.node-info { margin-left: 3px; margin-right: 3px; min-width: 0; }
.node-label { font-size: 11px; color: #64748b; white-space: nowrap; }
.node-detail { font-size: 9px; color: #475569; white-space: nowrap; }
.pipeline-node.done .node-label { color: #22c55e; }
.pipeline-node.active .node-label { color: #3b82f6; font-weight: 600; }
.pipeline-node.failed .node-label { color: #ef4444; }
.node-line { width: 14px; height: 2px; background: #334155; margin: 0 1px; flex-shrink: 0; }
.node-line.done { background: #22c55e; }
@keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.5; } }
</style>
