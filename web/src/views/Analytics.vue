<template>
  <div>
    <div style="display:flex;align-items:center;gap:12px;margin-bottom:20px">
      <h1 style="font-size:22px">📈 L4/L5 分析</h1>
      <el-button :icon="Refresh" circle @click="refresh" :loading="loading" />
    </div>

    <!-- Pipeline 概览 -->
    <el-card shadow="never" style="margin-bottom:20px">
      <template #header>Pipeline 概览</template>
      <el-row :gutter="16">
        <el-col :span="6">
          <el-statistic title="扫描总数" :value="pipeline.total_scanned || 0" />
        </el-col>
        <el-col :span="6">
          <el-statistic title="成功" :value="pipeline.total_success || 0">
            <template #suffix><span style="color:#22c55e;font-size:12px">✅</span></template>
          </el-statistic>
        </el-col>
        <el-col :span="6">
          <el-statistic title="失败" :value="pipeline.total_failed || 0">
            <template #suffix><span style="color:#ef4444;font-size:12px">❌</span></template>
          </el-statistic>
        </el-col>
        <el-col :span="6">
          <el-statistic title="平均耗时" :value="avgTime" />
        </el-col>
      </el-row>
    </el-card>

    <!-- L5 自优化评分 -->
    <el-card shadow="never" style="margin-bottom:20px">
      <template #header>
        <span style="display:flex;align-items:center;gap:8px">
          🧠 L5 自优化评分
          <el-tag type="success" size="small" effect="dark">AI 驱动</el-tag>
        </span>
      </template>
      <el-table :data="scores" stripe style="width:100%">
        <el-table-column label="排名" width="60">
          <template #default="{ $index }">
            <el-tag :type="$index < 3 ? 'warning' : 'info'" size="small" effect="dark">#{{ $index + 1 }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="智能体" width="140">
          <template #default="{ row }">
            <span style="display:flex;align-items:center;gap:6px">
              <span>{{ agentIcon(row.agent_id) }}</span>
              <span>{{ agentName(row.agent_id) }}</span>
            </span>
          </template>
        </el-table-column>
        <el-table-column label="综合评分" width="200">
          <template #default="{ row }">
            <el-progress
              :percentage="Math.round(row.overall_score || 0)"
              :stroke-width="16"
              :color="scoreColor(row.overall_score)"
              text-inside
            />
          </template>
        </el-table-column>
        <el-table-column label="成功率" width="100">
          <template #default="{ row }">
            <el-tag :type="row.success_rate > 50 ? 'success' : row.success_rate > 20 ? 'warning' : 'danger'" size="small">
              {{ (row.success_rate || 0).toFixed(0) }}%
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="平均耗时" width="100">
          <template #default="{ row }">
            {{ (row.avg_duration_s || 0).toFixed(0) }}s
          </template>
        </el-table-column>
        <el-table-column label="Bug 类型评分">
          <template #default="{ row }">
            <div style="display:flex;gap:4px;flex-wrap:wrap">
              <el-tag v-for="(score, type) in row.bug_type_scores" :key="type" size="small" effect="plain"
                :type="score > 60 ? 'success' : score > 40 ? 'warning' : 'danger'">
                {{ type }}: {{ score.toFixed(0) }}
              </el-tag>
            </div>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <!-- L5 生成的额外约束 -->
    <el-card shadow="never" style="margin-bottom:20px" v-if="Object.keys(constraints).length > 0">
      <template #header>
        <span style="display:flex;align-items:center;gap:8px">
          📋 L5 自动优化约束
          <el-tag type="primary" size="small">{{ totalConstraints }} 条</el-tag>
        </span>
      </template>
      <div v-for="(cons, agent) in constraints" :key="agent" style="margin-bottom:12px">
        <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px">
          <span>{{ agentIcon(agent) }}</span>
          <span style="font-weight:600;font-size:13px">{{ agentName(agent) }}</span>
          <el-tag size="small">{{ cons.length }} 条</el-tag>
        </div>
        <div style="padding-left:28px">
          <el-tag v-for="(c, i) in cons" :key="i" type="info" effect="plain" style="margin:2px 4px 2px 0;font-size:11px">
            {{ c.substring(0, 80) }}{{ c.length > 80 ? '...' : '' }}
          </el-tag>
        </div>
      </div>
    </el-card>

    <!-- 智能体绩效 -->
    <el-card shadow="never" style="margin-bottom:20px">
      <template #header>📊 智能体绩效</template>
      <div v-for="am in report.agent_metrics || []" :key="am.agent_id" style="display:flex;align-items:center;gap:12px;margin-bottom:10px">
        <span style="width:100px;font-size:13px;display:flex;align-items:center;gap:4px">
          <span>{{ agentIcon(am.agent_id) }}</span>
          <span>{{ agentName(am.agent_id) }}</span>
        </span>
        <el-progress
          :percentage="Math.round(am.success_rate)"
          :stroke-width="18"
          :color="am.success_rate > 50 ? '#22c55e' : am.success_rate > 20 ? '#f59e0b' : '#ef4444'"
          style="flex:1"
        />
        <div style="width:160px;display:flex;gap:8px;font-size:12px;color:#64748b">
          <span>{{ am.success_count }}/{{ am.total_fixes }}</span>
          <span>{{ (am.avg_duration_s || 0).toFixed(0) }}s</span>
        </div>
      </div>
    </el-card>

    <!-- 失败模式 -->
    <el-card shadow="never" style="margin-bottom:20px" v-if="report.failure_patterns?.length">
      <template #header>🔍 失败模式</template>
      <el-table :data="report.failure_patterns.slice(0, 10)" stripe size="small">
        <el-table-column prop="error_category" label="错误类别" min-width="200" show-overflow-tooltip />
        <el-table-column prop="count" label="次数" width="80" sortable>
          <template #default="{ row }">
            <el-tag type="danger" size="small">{{ row.count }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="涉及 Agent" width="200">
          <template #default="{ row }">
            <el-tag v-for="a in row.agents" :key="a" size="small" type="info" effect="plain" style="margin:1px">{{ agentName(a) }}</el-tag>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <!-- 优化建议 -->
    <el-card shadow="never" v-if="report.recommendations?.length">
      <template #header>💡 优化建议</template>
      <div v-for="(rec, i) in report.recommendations" :key="i" style="padding:8px 0;border-bottom:1px solid #334155;font-size:13px">
        {{ rec }}
      </div>
    </el-card>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { Refresh } from '@element-plus/icons-vue'

const report = ref({})
const scores = ref([])
const constraints = ref({})
const loading = ref(false)

const pipeline = computed(() => report.value.pipeline || {})
const avgTime = computed(() => ((pipeline.value.avg_fix_time_ms || 0) / 1000).toFixed(0) + 's')
const totalConstraints = computed(() => Object.values(constraints.value).reduce((s, c) => s + c.length, 0))

const agentIcons = { guanyu:'⚔️', zhaoyun:'🐉', xunyu:'📚', zhangfei:'🔥', huatuo:'💊', chenlin:'📝', liubei:'👑', zhugeliang:'🪶' }
const agentNames = { guanyu:'关羽', zhaoyun:'赵云', xunyu:'荀彧', zhangfei:'张飞', huatuo:'华佗', chenlin:'陈琳', liubei:'刘备', zhugeliang:'诸葛亮' }
function agentIcon(id) { return agentIcons[id] || '🤖' }
function agentName(id) {
    const map = { '关羽':'guanyu','赵云':'zhaoyun','荙录':'xunyu','张飞':'zhangfei','华佮':'huatuo','陈琳':'chenlin','刘备':'liubei','诸葛亮':'zhugeliang' };
    return agentNames[map[id] || id] || id;
}
function scoreColor(s) {
  if (s >= 60) return '#22c55e'
  if (s >= 40) return '#3b82f6'
  if (s >= 25) return '#f59e0b'
  return '#ef4444'
}

async function refresh() {
  loading.value = true
  try {
    const [analyticsRes, scoresRes] = await Promise.all([
      fetch('/api/analytics'),
      fetch('/api/scores')
    ])
    report.value = await analyticsRes.json()
    const scoresData = await scoresRes.json()
    scores.value = (scoresData.scores || []).sort((a, b) => (b.overall_score || 0) - (a.overall_score || 0))

    // Load L5 constraints
    try {
      const consRes = await fetch('/api/constraints')
      constraints.value = await consRes.json()
    } catch {}
  } catch {}
  loading.value = false
}

onMounted(refresh)
</script>
