<template>
  <div>
    <h1 style="margin-bottom:20px;font-size:22px">🤖 智能体系统</h1>

    <!-- 总协调者 -->
    <div style="margin-bottom:24px">
      <router-link :to="'/agent/liubei'" style="text-decoration:none;color:inherit">
        <el-card shadow="hover" class="agent-card coordinator-highlight" body-style="padding:24px">
          <div style="display:flex;align-items:center;gap:16px">
            <span style="font-size:40px">👑</span>
            <div style="flex:1">
              <div style="font-size:20px;font-weight:700">刘备 <span style="font-size:14px;color:#f59e0b;font-weight:400">🎯 总协调者</span></div>
              <div style="font-size:13px;color:#94a3b8;margin-top:4px">Subagent 架构主代理 — 扫描禅道 → 分析优先级 → 分派给 7 个子智能体</div>
              <div style="display:flex;flex-wrap:wrap;gap:6px;margin-top:8px">
                <el-tag v-for="tag in ['项目管理','任务分派','进度跟踪','Pipeline调度']" :key="tag" size="small" type="warning" effect="plain">{{ tag }}</el-tag>
              </div>
            </div>
            <el-tag :type="'info'" size="large" style="font-size:18px;font-weight:700;padding:6px 16px">
              {{ (agentData.find(a => a.id === 'liubei')?.score)?.toFixed(2) || '--' }}
            </el-tag>
          </div>
        </el-card>
      </router-link>
    </div>

    <!-- 分派箭头 -->
    <div style="text-align:center;color:#475569;font-size:13px;margin-bottom:16px">
      ▼ 刘备分派 → 7 个子智能体各司其职 → 结果回报 ▼
    </div>

    <!-- 子智能体 -->
    <el-row :gutter="16" style="margin-bottom:24px">
      <el-col :span="12" v-for="a in subagents" :key="a.id">
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
import { ref, computed, onMounted } from 'vue'

const scores = ref([])
const subagents = computed(() => agentData.value.filter(a => !a.isCoordinator))
const agentIdMap = { '\u5173\u7fbd':'guanyu','\u8d75\u4e91':'zhaoyun','\u8359\u5f55':'xunyu','\u5f20\u98de':'zhangfei','\u534e\u4f6e':'huatuo','\u9648\u7433':'chenlin','\u5218\u5907':'liubei','\u8bf8\u845b\u4eae':'zhugeliang' }
const agentLabels = { guanyu:'⚔️ 关羽', zhaoyun:'🐉 赵云', xunyu:'📚 荀彧', zhangfei:'🔥 张飞', huatuo:'💊 华佗', chenlin:'📝 陈琳', liubei:'👑 刘备', zhugeliang:'🪶 诸葛亮' }

const agentData = ref([
  { id:'liubei', icon:'👑', name:'刘备', role:'🎯 总协调者', description:'Subagent 架构主代理。扫描禅道活跃 Bug → 分析优先级 → 分派给子智能体。自身不修复 Bug，专注调度和进度跟踪。', expertise:['项目管理','任务分派','进度跟踪','Pipeline调度'], work_dir:null, git_branch:null, quality_gates:['coordinator_scan'], score:null, isCoordinator:true },
  { id:'guanyu', icon:'⚔️', name:'关羽', role:'🔧 后端修复', description:'子智能体 — Java/Spring 后端修复。精通 MyBatis-Plus、Spring Boot、REST API。', expertise:['Java','Spring','MyBatis','Maven','SQL'], work_dir:'/tmp/agentforge-worktrees/guanyu', git_branch:'guanyu', quality_gates:['mvn compile','Spring Boot 启动'], score:null },
  { id:'zhaoyun', icon:'🐉', name:'赵云', role:'🎨 前端修复', description:'子智能体 — Vue3 前端修复。精通 ElementUI、TypeScript、Vite。', expertise:['Vue3','ElementUI','TypeScript','CSS','Vite'], work_dir:'/tmp/agentforge-worktrees/zhaoyun', git_branch:'zhaoyun', quality_gates:['vue-tsc','vite build'], score:null },
  { id:'xunyu', icon:'📚', name:'荀彧', role:'🗄️ DB审查', description:'子智能体 — 数据库变更审查。精通 PostgreSQL、DDL、索引优化。', expertise:['SQL','PostgreSQL','DDL','索引','迁移'], work_dir:'/tmp/agentforge-worktrees/xunyu', git_branch:'xunyu', quality_gates:['SQL语法检查'], score:null },
  { id:'zhugeliang', icon:'🪶', name:'诸葛亮', role:'🧠 架构分析', description:'子智能体 — 分析 Bug 根因、判断是否需要 DB 审查、路由到合适的测试/修复智能体。', expertise:['架构','全链路分析','任务拆解','路由决策'], work_dir:null, git_branch:null, quality_gates:['analyze_done'], score:null },
  { id:'zhangfei', icon:'🔥', name:'张飞', role:'🧪 Playwright测试', description:'子智能体 — 运行回归测试验证修复质量。生成 Playwright 测试用例。', expertise:['Playwright','E2E','回归测试','BDT'], work_dir:null, git_branch:null, quality_gates:['npx playwright test'], score:null },
  { id:'huatuo', icon:'💊', name:'华佗', role:'✅ 产品验收', description:'子智能体 — 验证修复是否满足业务需求，确认后分配给提出人。', expertise:['验收','业务验证','需求符合度'], work_dir:null, git_branch:null, quality_gates:['verify_done'], score:null },
  { id:'chenlin', icon:'📝', name:'陈琳', role:'📄 文档归档', description:'子智能体 — 生成修复文档、归档到禅道、全流程闭环记录。', expertise:['文档','Markdown','归档','禅道备注'], work_dir:null, git_branch:null, quality_gates:['doc_done'], score:null },
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
.coordinator-highlight { border: 2px solid #f59e0b !important; background: linear-gradient(135deg, #1e293b 0%, #1a1f2e 100%); }
.coordinator-highlight:hover { border-color: #fbbf24 !important; box-shadow: 0 0 16px rgba(245,158,11,0.2); }
</style>
