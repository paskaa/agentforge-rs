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
              {{ (row.success_rate || 0).toFixed(2) }}%
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="平均耗时" width="100">
          <template #default="{ row }">
            {{ (row.avg_duration_s || 0).toFixed(2) }}s
          </template>
        </el-table-column>
        <el-table-column label="Bug 类型评分">
          <template #default="{ row }">
            <div style="display:flex;gap:4px;flex-wrap:wrap">
              <el-tag v-for="(score, type) in row.bug_type_scores" :key="type" size="small" effect="plain"
                :type="score > 60 ? 'success' : score > 40 ? 'warning' : 'danger'">
                {{ type }}: {{ score.toFixed(2) }}
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


    <!-- L5 优化记录 -->
    <el-card shadow="never" style="margin-bottom:20px" v-if="l5History.length > 0">
      <template #header>
        <span style="display:flex;align-items:center;gap:8px">
          📜 L5 优化记录
          <el-tag type="info" size="small">最近 {{ l5History.length }} 次</el-tag>
        </span>
      </template>

      <div v-for="(entry, idx) in l5History.slice().reverse()" :key="idx"
        style="margin-bottom:20px;padding:16px;border:1px solid #334155;border-radius:10px;background:rgba(15,23,42,0.5)">

        <!-- Header: time + action count -->
        <div style="display:flex;align-items:center;gap:10px;margin-bottom:12px">
          <el-tag :type="entry.actions_count > 0 ? 'warning' : 'success'" size="small" effect="dark">
            {{ entry.actions_count > 0 ? entry.actions_count + ' 项调整' : '✅ 无需优化' }}
          </el-tag>
          <span style="color:#64748b;font-size:12px">{{ formatTime(entry.timestamp) }}</span>
        </div>

        <!-- Score changes (before → after) -->
        <div v-if="entry.score_changes && entry.score_changes.length > 0" style="margin-bottom:12px">
          <div style="font-size:12px;color:#94a3b8;margin-bottom:6px">📊 评分变化</div>
          <div style="display:flex;gap:8px;flex-wrap:wrap">
            <div v-for="sc in entry.score_changes" :key="sc.agent"
              style="padding:6px 10px;border-radius:6px;background:rgba(30,41,59,0.8);font-size:12px;min-width:140px">
              <div style="font-weight:600;margin-bottom:2px">{{ agentName(sc.agent) }}</div>
              <div>
                成功率
                <span :style="{color: sc.success_rate_delta > 0 ? '#22c55e' : sc.success_rate_delta < 0 ? '#ef4444' : '#94a3b8'}">
                  {{ (sc.success_rate_before || 0).toFixed(0) }}% → {{ (sc.success_rate_after || 0).toFixed(0) }}%
                  {{ sc.success_rate_delta > 0 ? '↑' : sc.success_rate_delta < 0 ? '↓' : '' }}
                </span>
              </div>
              <div>
                综合分
                <span :style="{color: sc.overall_score_delta > 0 ? '#22c55e' : sc.overall_score_delta < 0 ? '#ef4444' : '#94a3b8'}">
                  {{ (sc.overall_score_before || 0).toFixed(1) }} → {{ (sc.overall_score_after || 0).toFixed(1) }}
                  {{ sc.overall_score_delta > 0 ? '↑' : sc.overall_score_delta < 0 ? '↓' : '' }}
                </span>
              </div>
            </div>
          </div>
        </div>

        <!-- Optimization actions -->
        <div v-if="entry.actions && entry.actions.length > 0" style="margin-bottom:12px">
          <div style="font-size:12px;color:#94a3b8;margin-bottom:6px">🔧 优化动作</div>
          <div v-for="(action, ai) in entry.actions" :key="ai"
            style="padding:6px 10px;margin:4px 0;border-radius:6px;background:rgba(30,41,59,0.8);font-size:12px;border-left:3px solid"
            :style="{ borderColor: action.confidence > 0.7 ? '#22c55e' : '#f59e0b' }">
            <div style="display:flex;align-items:center;gap:6px;margin-bottom:2px">
              <el-tag size="small" :type="actionTypeColor(action.type)">{{ actionTypeLabel(action.type) }}</el-tag>
              <span style="color:#94a3b8">→</span>
              <span style="font-weight:600">{{ agentName(action.target) }}</span>
              <el-tag size="small" type="info" effect="plain">置信度 {{ (action.confidence * 100).toFixed(0) }}%</el-tag>
            </div>
            <div style="color:#64748b;font-size:11px">原因: {{ action.reason }}</div>
            <div style="color:#94a3b8;font-size:11px">建议: {{ action.change }}</div>
          </div>
        </div>

        <!-- Git commits (his-repo) -->
        <div v-if="entry.git_commits && entry.git_commits.length > 0" style="margin-bottom:12px">
          <div style="font-size:12px;color:#94a3b8;margin-bottom:6px">📝 代码提交 (his-repo)</div>
          <div v-for="commit in entry.git_commits.slice(0, 8)" :key="commit.hash"
            style="display:flex;align-items:center;gap:8px;padding:4px 0;font-size:12px;border-bottom:1px solid rgba(51,65,85,0.3)">
            <code style="color:#60a5fa;font-family:monospace;font-size:11px;min-width:60px">{{ commit.short }}</code>
            <span style="color:#e2e8f0;flex:1">{{ commit.message }}</span>
            <span style="color:#475569;font-size:11px">{{ commit.date ? commit.date.substring(0, 16) : '' }}</span>
          </div>
        </div>

        <!-- Git diff stats -->
        <div v-if="entry.git_diff_stats && entry.git_diff_stats.length > 0" style="margin-bottom:12px">
          <div style="font-size:12px;color:#94a3b8;margin-bottom:6px">📊 变更统计</div>
          <div v-for="(ds, di) in entry.git_diff_stats.slice(0, 5)" :key="di"
            style="display:flex;align-items:center;gap:8px;padding:4px 8px;margin:3px 0;border-radius:4px;background:rgba(30,41,59,0.6);font-size:11px">
            <el-tag size="small" type="info" effect="plain">{{ ds.files_changed }} 文件</el-tag>
            <span style="color:#22c55e">+{{ ds.insertions }}</span>
            <span style="color:#ef4444">-{{ ds.deletions }}</span>
            <span style="color:#475569;margin-left:auto">
              {{ ds.files ? ds.files.slice(0, 3).join(', ') : '' }}{{ ds.files && ds.files.length > 3 ? '...' : '' }}
            </span>
          </div>
        </div>

        <!-- Framework commits -->
        <div v-if="entry.framework_commits && entry.framework_commits.length > 0">
          <div style="font-size:12px;color:#94a3b8;margin-bottom:6px">🏗️ 框架变更 (agentforge-rs)</div>
          <div v-for="fc in entry.framework_commits.slice(0, 5)" :key="fc.short"
            style="display:flex;align-items:center;gap:8px;padding:3px 0;font-size:11px;border-bottom:1px solid rgba(51,65,85,0.2)">
            <code style="color:#60a5fa;font-family:monospace;min-width:50px">{{ fc.short }}</code>
            <span style="color:#e2e8f0;flex:1">{{ fc.message }}</span>
            <span style="color:#475569">{{ fc.date ? fc.date.substring(0, 16) : '' }}</span>
          </div>
        </div>
      </div>
    </el-card>

    <!-- L5 优化成果对比 -->
    <el-card shadow="never" style="margin-bottom:20px" v-if="l5History.length >= 2">
      <template #header>
        <span style="display:flex;align-items:center;gap:8px">
          📈 L5 优化成果
          <el-tag type="success" size="small" effect="dark">趋势</el-tag>
        </span>
      </template>
      <el-row :gutter="16">
        <el-col :span="8" v-for="metric in l5Metrics" :key="metric.label">
          <div style="text-align:center;padding:12px">
            <div style="font-size:12px;color:#64748b;margin-bottom:4px">{{ metric.label }}</div>
            <div style="font-size:24px;font-weight:700" :style="{color: metric.trend > 0 ? '#22c55e' : metric.trend < 0 ? '#ef4444' : '#94a3b8'}">
              {{ metric.current }}
            </div>
            <div style="font-size:12px;margin-top:2px">
              <span :style="{color: metric.trend > 0 ? '#22c55e' : metric.trend < 0 ? '#ef4444' : '#94a3b8'}">
                {{ metric.trend > 0 ? '↑' : metric.trend < 0 ? '↓' : '→' }} {{ metric.delta }}
              </span>
              <span style="color:#475569;margin-left:4px">vs 上次</span>
            </div>
          </div>
        </el-col>
      </el-row>
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
const l5History = ref([])
const loading = ref(false)

const pipeline = computed(() => report.value.pipeline || {})
const avgTime = computed(() => ((pipeline.value.avg_fix_time_ms || 0) / 1000).toFixed(0) + 's')
const totalConstraints = computed(() => Object.values(constraints.value).reduce((s, c) => s + c.length, 0))
const l5Metrics = computed(() => {
  if (l5History.value.length < 2) return []
  const latest = l5History.value[l5History.value.length - 1]
  const prev = l5History.value[l5History.value.length - 2]
  const latestScores = latest.scores_snapshot || []
  const prevScores = prev.scores_snapshot || []
  const avgLatest = latestScores.length ? latestScores.reduce((s, a) => s + (a.success_rate || 0), 0) / latestScores.length : 0
  const avgPrev = prevScores.length ? prevScores.reduce((s, a) => s + (a.success_rate || 0), 0) / prevScores.length : 0
  const avgDurLatest = latestScores.length ? latestScores.reduce((s, a) => s + (a.avg_duration_s || 0), 0) / latestScores.length : 0
  const avgDurPrev = prevScores.length ? prevScores.reduce((s, a) => s + (a.avg_duration_s || 0), 0) / prevScores.length : 0
  return [
    { label: '平均成功率', current: avgLatest.toFixed(0) + '%', trend: avgLatest - avgPrev, delta: Math.abs(avgLatest - avgPrev).toFixed(1) + '%' },
    { label: '平均耗时', current: avgDurLatest.toFixed(0) + 's', trend: avgDurPrev - avgDurLatest, delta: Math.abs(avgDurLatest - avgDurPrev).toFixed(0) + 's' },
    { label: '优化次数', current: latest.actions_count, trend: latest.actions_count - prev.actions_count, delta: Math.abs(latest.actions_count - prev.actions_count) },
  ]
})

const agentIcons = { guanyu:'⚔️', zhaoyun:'🐉', xunyu:'📚', zhangfei:'🔥', huatuo:'💊', chenlin:'📝', liubei:'👑', zhugeliang:'🪶' }
const agentNames = { guanyu:'关羽', zhaoyun:'赵云', xunyu:'荀彧', zhangfei:'张飞', huatuo:'华佗', chenlin:'陈琳', liubei:'刘备', zhugeliang:'诸葛亮' }
function agentIcon(id) { return agentIcons[id] || '🤖' }
function agentName(id) {
    const map = { '关羽':'guanyu','赵云':'zhaoyun','荙录':'xunyu','张飞':'zhangfei','华佮':'huatuo','陈琳':'chenlin','刘备':'liubei','诸葛亮':'zhugeliang' };
    return agentNames[map[id] || id] || id;
}
function formatTime(ts) {
  if (!ts) return ''
  const d = new Date(ts)
  return d.toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
}
function actionTypeColor(t) {
  return { adjust_constraint: 'warning', reroute: 'danger', retry_strategy: 'info', prompt_boost: 'success' }[t] || 'info'
}
function actionTypeLabel(t) {
  return { adjust_constraint: '约束调整', reroute: '路由调整', retry_strategy: '重试策略', prompt_boost: '提示增强' }[t] || t
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

    // Load L5 constraints + history
    try {
      const [consRes, l5Res] = await Promise.all([
        fetch('/api/constraints'),
        fetch('/api/l5/history')
      ])
      constraints.value = await consRes.json()
      const l5Data = await l5Res.json()
      l5History.value = l5Data.history || []
    } catch {}
  } catch {}
  loading.value = false
}

onMounted(refresh)
</script>
