<template>
  <div class="agents-page">
    <h1>🤖 智能体系统</h1>

    <div class="agents-grid">
      <div v-for="a in agents" :key="a.id" class="agent-detail-card">
        <div class="agent-top">
          <span class="agent-icon">{{ a.icon }}</span>
          <div>
            <div class="agent-name">{{ a.name }}</div>
            <div class="agent-role">{{ a.role }}</div>
          </div>
          <div class="agent-score" :class="scoreClass(a.score)">
            {{ a.score?.toFixed(1) || '--' }}
          </div>
        </div>
        <div class="agent-desc">{{ a.description }}</div>
        <div class="agent-tags">
          <span v-for="tag in a.expertise" :key="tag" class="tag">{{ tag }}</span>
        </div>
        <div class="agent-meta">
          <span>📁 {{ a.work_dir || '未配置' }}</span>
          <span>🌿 {{ a.git_branch || 'N/A' }}</span>
        </div>
        <div class="agent-gates" v-if="a.quality_gates?.length">
          <div class="gates-label">质量门禁:</div>
          <div v-for="g in a.quality_gates" :key="g" class="gate-item">{{ g }}</div>
        </div>
      </div>
    </div>

    <!-- L5 Scores -->
    <div class="section" v-if="scores.length">
      <h2>🏆 L5 评分排名</h2>
      <div class="scores-table">
        <div v-for="(s, i) in scores" :key="s.agent_id" class="score-row">
          <div class="score-rank">#{{ i + 1 }}</div>
          <div class="score-name">{{ agentLabel(s.agent_id) }}</div>
          <div class="score-bar-wrap">
            <div class="score-bar" :style="{ width: s.overall_score + '%' }"></div>
          </div>
          <div class="score-value">{{ s.overall_score?.toFixed(1) }}</div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'

const agents = ref([])
const scores = ref([])

const agentData = [
  { id: 'guanyu', icon: '⚔️', name: '关羽', role: '后端修复工程师', description: '负责 Java/Spring 后端修复。精通 MyBatis-Plus、Spring Boot、REST API、Maven。', expertise: ['Java', 'Spring', 'MyBatis', 'Maven', 'SQL'], work_dir: '/tmp/agentforge-worktrees/guanyu/openhis-server-new', git_branch: 'guanyu', quality_gates: ['mvn clean compile', 'Spring Boot 启动测试'] },
  { id: 'zhaoyun', icon: '🐉', name: '赵云', role: '前端修复工程师', description: '负责 Vue3 前端修复。精通 ElementUI、TypeScript、Axios、Vite。', expertise: ['Vue3', 'ElementUI', 'TypeScript', 'CSS', 'Vite'], work_dir: '/tmp/agentforge-worktrees/zhaoyun/openhis-ui-vue3', git_branch: 'zhaoyun', quality_gates: ['vue-tsc --noEmit', 'vite build', 'npm run lint'] },
  { id: 'xunyu', icon: '📚', name: '荀彧', role: '数据库工程师', description: '负责 SQL/数据库修复。精通 PostgreSQL、DDL、DML、索引优化。', expertise: ['SQL', 'PostgreSQL', 'DDL', '索引', '迁移'], work_dir: '/tmp/agentforge-worktrees/xunyu/openhis-server-new', git_branch: 'xunyu', quality_gates: [] },
  { id: 'zhangfei', icon: '🔥', name: '张飞', role: 'QA 测试工程师', description: '负责运行回归测试（Playwright）来验证修复质量。', expertise: ['Playwright', 'E2E', '回归测试'], work_dir: '/tmp/agentforge-worktrees/zhangfei', git_branch: 'zhangfei', quality_gates: ['npx playwright test'] },
  { id: 'huatuo', icon: '💊', name: '华佗', role: '产品验收员', description: '负责验证修复是否满足业务需求。关注用户场景和验收标准。', expertise: ['验收', '业务验证', '需求符合度'], work_dir: null, git_branch: null, quality_gates: [] },
  { id: 'chenlin', icon: '📝', name: '陈琳', role: '文档工程师', description: '负责生成和归档 Bug 修复文档。', expertise: ['文档', 'Markdown', '归档'], work_dir: null, git_branch: null, quality_gates: [] },
  { id: 'liubei', icon: '👑', name: '刘备', role: '项目经理', description: '负责跟踪进度、协调资源、管理需求优先级。', expertise: ['项目管理', '进度跟踪', 'Pipeline监控'], work_dir: null, git_branch: null, quality_gates: [] },
  { id: 'zhugeliang', icon: '🪶', name: '诸葛亮', role: '架构师/协调者', description: '负责分析 Bug、拆解任务、分派给合适的修复 Agent。', expertise: ['架构', '全链路分析', '任务拆解'], work_dir: null, git_branch: null, quality_gates: [] },
]

const agentLabels = { guanyu: '⚔️ 关羽', zhaoyun: '🐉 赵云', xunyu: '📚 荀彧', zhangfei: '🔥 张飞', huatuo: '💊 华佗', chenlin: '📝 陈琳', liubei: '👑 刘备', zhugeliang: '🪶 诸葛亮' }
function agentLabel(id) { return agentLabels[id] || id }
function scoreClass(s) { return s >= 50 ? 'score-high' : s >= 25 ? 'score-mid' : 'score-low' }

onMounted(async () => {
  agents.value = agentData
  try {
    const r = await fetch('/api/scores')
    const d = await r.json()
    scores.value = (d.scores || []).sort((a, b) => (b.overall_score || 0) - (a.overall_score || 0))
    // Merge scores into agents
    for (const s of scores.value) {
      const a = agents.value.find(x => x.id === s.agent_id)
      if (a) a.score = s.overall_score
    }
  } catch {}
})
</script>

<style scoped>
.agents-page h1 { font-size: 24px; margin-bottom: 24px; }

.agents-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 16px; margin-bottom: 32px; }
.agent-detail-card {
  background: #1e293b; border-radius: 12px; padding: 20px; border: 1px solid #334155;
}
.agent-top { display: flex; align-items: center; gap: 12px; margin-bottom: 12px; }
.agent-icon { font-size: 32px; }
.agent-name { font-size: 16px; font-weight: 600; }
.agent-role { font-size: 12px; color: #94a3b8; }
.agent-score {
  margin-left: auto; font-size: 20px; font-weight: 700; width: 50px; text-align: center;
  padding: 4px; border-radius: 8px;
}
.score-high { color: #22c55e; background: #052e16; }
.score-mid { color: #f59e0b; background: #422006; }
.score-low { color: #ef4444; background: #450a0a; }
.agent-desc { font-size: 13px; color: #94a3b8; margin-bottom: 12px; line-height: 1.5; }
.agent-tags { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 12px; }
.tag { background: #334155; color: #cbd5e1; padding: 2px 8px; border-radius: 4px; font-size: 11px; }
.agent-meta { display: flex; gap: 16px; font-size: 11px; color: #64748b; margin-bottom: 8px; }
.agent-gates { font-size: 12px; }
.gates-label { color: #64748b; margin-bottom: 4px; }
.gate-item { color: #94a3b8; padding-left: 8px; }

.section { margin-bottom: 32px; }
.section h2 { font-size: 18px; margin-bottom: 16px; color: #cbd5e1; }

.scores-table { background: #1e293b; border-radius: 12px; padding: 20px; border: 1px solid #334155; }
.score-row { display: flex; align-items: center; gap: 12px; margin-bottom: 10px; }
.score-rank { width: 30px; font-size: 13px; color: #64748b; }
.score-name { width: 100px; font-size: 13px; }
.score-bar-wrap { flex: 1; height: 16px; background: #334155; border-radius: 4px; overflow: hidden; }
.score-bar { height: 100%; background: linear-gradient(90deg, #3b82f6, #8b5cf6); border-radius: 4px; }
.score-value { width: 40px; text-align: right; font-weight: 600; font-size: 13px; }
</style>
