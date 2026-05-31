<template>
  <div class="dashboard">
    <h1>📊 仪表盘</h1>

    <!-- Top stats -->
    <div class="stats-grid">
      <div class="stat-card">
        <div class="stat-value">{{ stats.total_bugs || 0 }}</div>
        <div class="stat-label">活跃 Bug</div>
      </div>
      <div class="stat-card success">
        <div class="stat-value">{{ stats.fixed_today || 0 }}</div>
        <div class="stat-label">今日修复</div>
      </div>
      <div class="stat-card warning">
        <div class="stat-value">{{ stats.running_agents || 0 }}</div>
        <div class="stat-label">运行中 Agent</div>
      </div>
      <div class="stat-card info">
        <div class="stat-value">{{ stats.success_rate || '0%' }}</div>
        <div class="stat-label">总成功率</div>
      </div>
    </div>

    <!-- Agent status -->
    <div class="section">
      <h2>🤖 智能体状态</h2>
      <div class="agent-grid">
        <div v-for="agent in agents" :key="agent.id" class="agent-card" :class="agent.status">
          <div class="agent-header">
            <span class="agent-icon">{{ agent.icon }}</span>
            <span class="agent-name">{{ agent.name }}</span>
            <span class="agent-status-badge" :class="agent.status">{{ statusText(agent.status) }}</span>
          </div>
          <div class="agent-role">{{ agent.role }}</div>
          <div class="agent-stats">
            <span>成功率 {{ agent.success_rate }}</span>
            <span>耗时 {{ agent.avg_time }}</span>
          </div>
          <div class="agent-bar">
            <div class="agent-bar-fill" :style="{ width: agent.success_rate }"></div>
          </div>
        </div>
      </div>
    </div>

    <!-- Recent fixes -->
    <div class="section">
      <h2>📝 最近修复</h2>
      <table class="fix-table">
        <thead>
          <tr><th>Bug</th><th>Agent</th><th>状态</th><th>耗时</th><th>时间</th></tr>
        </thead>
        <tbody>
          <tr v-for="fix in recent_fixes" :key="fix.bug_id">
            <td class="bug-id">#{{ fix.bug_id }}</td>
            <td>{{ fix.agent }}</td>
            <td><span class="status-badge" :class="fix.success ? 'ok' : 'fail'">{{ fix.success ? '✅ 成功' : '❌ 失败' }}</span></td>
            <td>{{ fix.duration }}</td>
            <td>{{ fix.time }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- Pipeline status -->
    <div class="section">
      <h2>🔄 Pipeline 状态</h2>
      <div class="pipeline-info">
        <div class="pipeline-item">
          <span class="pipeline-label">当前任务</span>
          <span class="pipeline-value">{{ pipeline.current || '空闲' }}</span>
        </div>
        <div class="pipeline-item">
          <span class="pipeline-label">队列长度</span>
          <span class="pipeline-value">{{ pipeline.queue_length || 0 }}</span>
        </div>
        <div class="pipeline-item">
          <span class="pipeline-label">今日处理</span>
          <span class="pipeline-value">{{ pipeline.processed_today || 0 }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'

const stats = ref({})
const agents = ref([])
const recent_fixes = ref([])
const pipeline = ref({})

const agentIcons = { guanyu: '⚔️', zhaoyun: '🐉', xunyu: '📚', zhangfei: '🔥', huatuo: '💊', chenlin: '📝', liubei: '👑', zhugeliang: '🪶' }
const agentNames = { guanyu: '关羽', zhaoyun: '赵云', xunyu: '荀彧', zhangfei: '张飞', huatuo: '华佗', chenlin: '陈琳', liubei: '刘备', zhugeliang: '诸葛亮' }
const agentRoles = { guanyu: '后端开发', zhaoyun: '前端开发', xunyu: 'DBA', zhangfei: '测试', huatuo: '产品经理', chenlin: '文档', liubei: '项目经理', zhugeliang: '架构师' }

function statusText(s) { return { working: '工作中', idle: '空闲', error: '异常' }[s] || '未知' }

onMounted(async () => {
  try {
    const r = await fetch('/api/dashboard')
    const d = await r.json()
    stats.value = d.stats || {}
    pipeline.value = d.pipeline || {}
    recent_fixes.value = d.recent_fixes || []

    agents.value = (d.agents || []).map(a => ({
      ...a,
      icon: agentIcons[a.id] || '🤖',
      name: agentNames[a.id] || a.id,
      role: agentRoles[a.id] || a.role,
    }))
  } catch (e) {
    // Fallback mock data
    agents.value = Object.keys(agentIcons).map(id => ({
      id, icon: agentIcons[id], name: agentNames[id], role: agentRoles[id],
      status: 'idle', success_rate: '0%', avg_time: '0s'
    }))
  }
})
</script>

<style scoped>
.dashboard h1 { font-size: 24px; margin-bottom: 24px; }

.stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; margin-bottom: 32px; }
.stat-card {
  background: #1e293b; border-radius: 12px; padding: 20px; border: 1px solid #334155;
}
.stat-card.success { border-left: 4px solid #22c55e; }
.stat-card.warning { border-left: 4px solid #f59e0b; }
.stat-card.info { border-left: 4px solid #3b82f6; }
.stat-value { font-size: 32px; font-weight: 700; color: #f8fafc; }
.stat-label { font-size: 13px; color: #94a3b8; margin-top: 4px; }

.section { margin-bottom: 32px; }
.section h2 { font-size: 18px; margin-bottom: 16px; color: #cbd5e1; }

.agent-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
.agent-card {
  background: #1e293b; border-radius: 10px; padding: 16px; border: 1px solid #334155;
  transition: transform 0.2s;
}
.agent-card:hover { transform: translateY(-2px); }
.agent-card.working { border-color: #22c55e; }
.agent-header { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.agent-icon { font-size: 20px; }
.agent-name { font-weight: 600; font-size: 14px; }
.agent-status-badge {
  margin-left: auto; font-size: 11px; padding: 2px 8px; border-radius: 10px;
}
.agent-status-badge.working { background: #052e16; color: #22c55e; }
.agent-status-badge.idle { background: #1e293b; color: #64748b; }
.agent-status-badge.error { background: #450a0a; color: #ef4444; }
.agent-role { font-size: 12px; color: #64748b; margin-bottom: 8px; }
.agent-stats { display: flex; justify-content: space-between; font-size: 12px; color: #94a3b8; margin-bottom: 6px; }
.agent-bar { height: 4px; background: #334155; border-radius: 2px; }
.agent-bar-fill { height: 100%; background: linear-gradient(90deg, #3b82f6, #22c55e); border-radius: 2px; transition: width 0.5s; }

.fix-table { width: 100%; border-collapse: collapse; }
.fix-table th, .fix-table td { padding: 10px 14px; text-align: left; border-bottom: 1px solid #334155; font-size: 13px; }
.fix-table th { color: #64748b; font-weight: 500; }
.bug-id { font-family: monospace; color: #60a5fa; }
.status-badge { padding: 2px 8px; border-radius: 4px; font-size: 12px; }
.status-badge.ok { background: #052e16; color: #22c55e; }
.status-badge.fail { background: #450a0a; color: #ef4444; }

.pipeline-info { display: flex; gap: 24px; }
.pipeline-item { background: #1e293b; padding: 16px 24px; border-radius: 10px; border: 1px solid #334155; }
.pipeline-label { display: block; font-size: 12px; color: #64748b; margin-bottom: 4px; }
.pipeline-value { font-size: 20px; font-weight: 600; }
</style>
