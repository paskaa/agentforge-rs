<template>
  <div>
    <h1 style="margin-bottom:20px;font-size:22px">🤖 智能体系统</h1>

    <el-row :gutter="16" style="margin-bottom:24px">
      <el-col :span="12" v-for="a in agentData" :key="a.id">
        <router-link :to="'/agent/' + a.id" style="text-decoration:none;color:inherit">
          <el-card shadow="hover" class="agent-card" body-style="padding:20px">
            <div style="display:flex;align-items:center;gap:12px;margin-bottom:12px">
              <span style="font-size:28px">{{ a.icon }}</span>
              <div>
                <div style="font-size:16px;font-weight:600">{{ a.name }}</div>
                <div style="font-size:12px;color:#64748b">{{ a.role }}</div>
              </div>
              <el-tag :type="getScoreType(a.score)" size="large" style="margin-left:auto;font-size:16px;font-weight:700;padding:4px 12px">
                {{ a.score?.toFixed(2) || '--' }}
              </el-tag>
            </div>
            <div style="font-size:12px;color:#94a3b8;margin-bottom:10px">{{ a.description }}</div>
            <div style="display:flex;flex-wrap:wrap;gap:4px;margin-bottom:10px">
              <el-tag v-for="tag in a.expertise" :key="tag" size="small" type="info" effect="plain">{{ tag }}</el-tag>
            </div>
            <div style="display:flex;gap:16px;font-size:11px;color:#475569">
              <span>📁 {{ a.work_dir || '未配置' }}</span>
              <span>🌿 {{ a.git_branch || 'N/A' }}</span>
            </div>
            <div v-if="a.quality_gates?.length" style="margin-top:8px;font-size:11px">
              <span style="color:#475569">质量门禁: </span>
              <el-tag v-for="g in a.quality_gates" :key="g" size="small" type="success" effect="plain" style="margin-right:4px">{{ g }}</el-tag>
            </div>
          </el-card>
        </router-link>
      </el-col>
    </el-row>

    <el-card shadow="never" v-if="scores.length">
      <template #header>🏆 L5 评分排名</template>
      <div v-for="(s, i) in scores" :key="s.agent_id" style="display:flex;align-items:center;gap:12px;margin-bottom:10px">
        <span style="width:30px;font-size:13px;color:#64748b">#{{ i + 1 }}</span>
        <span style="width:100px;font-size:13px">{{ agentLabels[s.agent_id] || s.agent_id }}</span>
        <el-progress :percentage="s.overall_score || 0" :stroke-width="14" style="flex:1" :color="s.overall_score > 50 ? '#22c55e' : s.overall_score > 25 ? '#f59e0b' : '#ef4444'" :format="(p) => p.toFixed(2) + '%'" />
        <span style="width:60px;text-align:right;font-weight:600;font-size:13px">{{ (s.overall_score || 0).toFixed(2) }}%</span>
      </div>
    </el-card>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'

const scores = ref([])
const agentIdMap = { '\u5173\u7fbd':'guanyu','\u8d75\u4e91':'zhaoyun','\u8359\u5f55':'xunyu','\u5f20\u98de':'zhangfei','\u534e\u4f6e':'huatuo','\u9648\u7433':'chenlin','\u5218\u5907':'liubei','\u8bf8\u845b\u4eae':'zhugeliang' }
const agentLabels = { guanyu:'⚔️ 关羽', zhaoyun:'🐉 赵云', xunyu:'📚 荀彧', zhangfei:'🔥 张飞', huatuo:'💊 华佗', chenlin:'📝 陈琳', liubei:'👑 刘备', zhugeliang:'🪶 诸葛亮' }

const agentData = ref([
  { id:'guanyu', icon:'⚔️', name:'关羽', role:'后端修复工程师', description:'负责 Java/Spring 后端修复。精通 MyBatis-Plus、Spring Boot、REST API。', expertise:['Java','Spring','MyBatis','Maven','SQL'], work_dir:'/tmp/agentforge-worktrees/guanyu', git_branch:'guanyu', quality_gates:['mvn clean compile','Spring Boot 启动测试'], score:null },
  { id:'zhaoyun', icon:'🐉', name:'赵云', role:'前端修复工程师', description:'负责 Vue3 前端修复。精通 ElementUI、TypeScript、Vite。', expertise:['Vue3','ElementUI','TypeScript','CSS','Vite'], work_dir:'/tmp/agentforge-worktrees/zhaoyun', git_branch:'zhaoyun', quality_gates:['vue-tsc --noEmit','vite build'], score:null },
  { id:'xunyu', icon:'📚', name:'荀彧', role:'数据库工程师', description:'负责 SQL/数据库修复。精通 PostgreSQL、DDL、索引优化。', expertise:['SQL','PostgreSQL','DDL','索引','迁移'], work_dir:'/tmp/agentforge-worktrees/xunyu', git_branch:'xunyu', quality_gates:[], score:null },
  { id:'zhangfei', icon:'🔥', name:'张飞', role:'QA 测试工程师', description:'负责运行回归测试（Playwright）来验证修复质量。', expertise:['Playwright','E2E','回归测试'], work_dir:null, git_branch:null, quality_gates:['npx playwright test'], score:null },
  { id:'huatuo', icon:'💊', name:'华佗', role:'产品验收员', description:'负责验证修复是否满足业务需求。', expertise:['验收','业务验证','需求符合度'], work_dir:null, git_branch:null, quality_gates:[], score:null },
  { id:'chenlin', icon:'📝', name:'陈琳', role:'文档工程师', description:'负责生成和归档 Bug 修复文档。', expertise:['文档','Markdown','归档'], work_dir:null, git_branch:null, quality_gates:[], score:null },
  { id:'liubei', icon:'👑', name:'刘备', role:'项目经理', description:'负责跟踪进度、协调资源。', expertise:['项目管理','进度跟踪','Pipeline监控'], work_dir:null, git_branch:null, quality_gates:[], score:null },
  { id:'zhugeliang', icon:'🪶', name:'诸葛亮', role:'架构师/协调者', description:'负责分析 Bug、拆解任务、分派给合适的修复 Agent。', expertise:['架构','全链路分析','任务拆解'], work_dir:null, git_branch:null, quality_gates:[], score:null },
])

function getScoreType(s) { return s >= 50 ? 'success' : s >= 25 ? 'warning' : s > 0 ? 'danger' : 'info' }

onMounted(async () => {
  try {
    const r = await fetch('/api/scores')
    const d = await r.json()
    const scoresList = (d.scores || []).sort((a, b) => (b.overall_score || 0) - (a.overall_score || 0))
    scores.value = scoresList
    // Build score map
    const scoreMap = {}
    for (const s of scoresList) {
      const normalizedId = agentIdMap[s.agent_id] || s.agent_id
      scoreMap[normalizedId] = s.overall_score
    }
    // Rebuild agentData with scores
    agentData.value = agentData.value.map(a => ({
      ...a,
      score: scoreMap[a.id] ?? null
    }))
  } catch {}
})
</script>

<style scoped>
.agent-card { transition: all 0.2s; border: 1px solid #334155; }
.agent-card:hover { border-color: #3b82f6; }
</style>
