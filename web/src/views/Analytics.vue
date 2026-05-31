<template>
  <div class="analytics">
    <h1>📈 L4 量化分析</h1>

    <div class="controls">
      <button @click="refresh" class="btn" :disabled="loading">
        {{ loading ? '加载中...' : '🔄 刷新数据' }}
      </button>
    </div>

    <!-- Pipeline overview -->
    <div class="section">
      <h2>Pipeline 概览</h2>
      <div class="stats-grid">
        <div class="stat-card"><div class="stat-value">{{ report.pipeline?.total_scanned || 0 }}</div><div class="stat-label">扫描总数</div></div>
        <div class="stat-card success"><div class="stat-value">{{ report.pipeline?.total_success || 0 }}</div><div class="stat-label">成功</div></div>
        <div class="stat-card danger"><div class="stat-value">{{ report.pipeline?.total_failed || 0 }}</div><div class="stat-label">失败</div></div>
        <div class="stat-card info"><div class="stat-value">{{ avgTime }}</div><div class="stat-label">平均耗时</div></div>
      </div>
    </div>

    <!-- Agent performance -->
    <div class="section">
      <h2>智能体绩效</h2>
      <div class="chart-container">
        <div v-for="am in report.agent_metrics || []" :key="am.agent_id" class="metric-row">
          <div class="metric-name">{{ agentLabel(am.agent_id) }}</div>
          <div class="metric-bar-wrap">
            <div class="metric-bar" :style="{ width: am.success_rate + '%' }" :class="barClass(am.success_rate)"></div>
          </div>
          <div class="metric-values">
            <span class="metric-rate" :class="textClass(am.success_rate)">{{ am.success_rate.toFixed(1) }}%</span>
            <span class="metric-count">{{ am.success_count }}/{{ am.total_fixes }}</span>
            <span class="metric-time">{{ (am.avg_duration_s || 0).toFixed(0) }}s</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Failure patterns -->
    <div class="section" v-if="report.failure_patterns?.length">
      <h2>失败模式</h2>
      <table class="data-table">
        <thead><tr><th>错误类别</th><th>次数</th><th>涉及 Agent</th></tr></thead>
        <tbody>
          <tr v-for="(fp, i) in report.failure_patterns.slice(0, 10)" :key="i">
            <td>{{ fp.error_category?.substring(0, 60) || '?' }}</td>
            <td class="count">{{ fp.count }}</td>
            <td>{{ fp.agents?.join(', ') }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- Recommendations -->
    <div class="section" v-if="report.recommendations?.length">
      <h2>优化建议</h2>
      <div class="rec-list">
        <div v-for="(rec, i) in report.recommendations" :key="i" class="rec-item">{{ rec }}</div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'

const report = ref({})
const loading = ref(false)

const agentLabels = { guanyu: '⚔️ 关羽', zhaoyun: '🐉 赵云', xunyu: '📚 荀彧', zhangfei: '🔥 张飞', huatuo: '💊 华佗', chenlin: '📝 陈琳', liubei: '👑 刘备', zhugeliang: '🪶 诸葛亮' }
function agentLabel(id) { return agentLabels[id] || id }
function barClass(r) { return r >= 70 ? 'bar-good' : r >= 40 ? 'bar-mid' : 'bar-bad' }
function textClass(r) { return r >= 70 ? 'text-good' : r >= 40 ? 'text-mid' : 'text-bad' }

const avgTime = computed(() => {
  const t = report.value.pipeline?.avg_fix_time_ms || 0
  return (t / 1000).toFixed(0) + 's'
})

async function refresh() {
  loading.value = true
  try {
    const r = await fetch('/api/analytics')
    report.value = await r.json()
  } catch { report.value = {} }
  loading.value = false
}

onMounted(refresh)
</script>

<style scoped>
.analytics h1 { font-size: 24px; margin-bottom: 16px; }
.controls { margin-bottom: 24px; }
.btn { background: #3b82f6; color: white; border: none; padding: 8px 16px; border-radius: 8px; cursor: pointer; font-size: 14px; }
.btn:hover { background: #2563eb; }
.btn:disabled { opacity: 0.5; }

.section { margin-bottom: 32px; }
.section h2 { font-size: 18px; margin-bottom: 16px; color: #cbd5e1; }

.stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; }
.stat-card { background: #1e293b; border-radius: 12px; padding: 20px; border: 1px solid #334155; }
.stat-card.success { border-left: 4px solid #22c55e; }
.stat-card.danger { border-left: 4px solid #ef4444; }
.stat-card.info { border-left: 4px solid #3b82f6; }
.stat-value { font-size: 28px; font-weight: 700; }
.stat-label { font-size: 13px; color: #94a3b8; margin-top: 4px; }

.chart-container { background: #1e293b; border-radius: 12px; padding: 20px; border: 1px solid #334155; }
.metric-row { display: flex; align-items: center; gap: 12px; margin-bottom: 12px; }
.metric-name { width: 100px; font-size: 13px; font-weight: 500; }
.metric-bar-wrap { flex: 1; height: 20px; background: #334155; border-radius: 4px; overflow: hidden; }
.metric-bar { height: 100%; border-radius: 4px; transition: width 0.5s; }
.bar-good { background: linear-gradient(90deg, #22c55e, #16a34a); }
.bar-mid { background: linear-gradient(90deg, #f59e0b, #d97706); }
.bar-bad { background: linear-gradient(90deg, #ef4444, #dc2626); }
.metric-values { width: 200px; display: flex; gap: 12px; font-size: 12px; color: #94a3b8; }
.metric-rate { font-weight: 600; width: 50px; }
.text-good { color: #22c55e; }
.text-mid { color: #f59e0b; }
.text-bad { color: #ef4444; }
.metric-count { width: 50px; }
.metric-time { width: 50px; }

.data-table { width: 100%; border-collapse: collapse; background: #1e293b; border-radius: 12px; overflow: hidden; }
.data-table th, .data-table td { padding: 10px 14px; text-align: left; border-bottom: 1px solid #334155; font-size: 13px; }
.data-table th { color: #64748b; background: #0f172a; }
.count { font-weight: 600; color: #f59e0b; }

.rec-list { display: flex; flex-direction: column; gap: 8px; }
.rec-item { background: #1e293b; border: 1px solid #334155; border-radius: 8px; padding: 12px 16px; font-size: 13px; }
</style>
