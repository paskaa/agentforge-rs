<template>
  <el-card shadow="never" style="margin-bottom:16px" v-if="report || traces.length">
    <template #header>
      <div style="display:flex;align-items:center;gap:8px">
        <span>🧪 测试 & 验证流程</span>
        <el-tag v-if="report" :type="report.all_passed ? 'success' : 'danger'" size="small" effect="dark">
          {{ report.all_passed ? '✅ 全部通过' : '❌ 验证失败' }}
        </el-tag>
        <span style="color:#64748b;font-size:12px;margin-left:auto">
          {{ traces.length }} 个步骤 · {{ totalDuration }}
        </span>
      </div>
    </template>

    <!-- 测试生命周期时间线 -->
    <el-alert v-if="isRejected" type="error" :title="rejectReason" show-icon style="margin-bottom:12px" />

    <el-timeline v-if="traces.length">
      <el-timeline-item
        v-for="(t, idx) in traces" :key="idx"
        :timestamp="formatTime(t.ts)"
        :type="timelineType(t.event, t.status)"
        :hollow="t.status !== 'ok' && t.status !== 'failed'"
        placement="top">
        
        <!-- 事件标题行 -->
        <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
          <el-tag :type="tagType(t.event)" size="small" effect="plain" style="font-weight:600">
            {{ eventIcon(t.event) }} {{ eventLabel(t.event) }}
          </el-tag>
          <span style="color:#334155;font-size:13px;font-weight:500">{{ t.message || '-' }}</span>
          <el-tag v-if="t.duration_ms > 0" type="info" size="small" effect="plain" style="margin-left:auto">
            {{ formatMs(t.duration_ms) }}
          </el-tag>
        </div>

        <!-- 展开详情按钮 -->
        <div v-if="hasDetail(t)" style="margin-top:6px">
          <el-button size="small" text type="primary" @click="toggleDetail(idx)" style="padding:0;font-size:12px">
            {{ expandedIdx === idx ? '收起详情' : '展开详情' }}
            <el-icon style="margin-left:2px;font-size:10px">
              <ArrowDown v-if="expandedIdx !== idx" />
              <ArrowUp v-else />
            </el-icon>
          </el-button>
        </div>

        <!-- 展开的详情 -->
        <div v-if="expandedIdx === idx" style="margin-top:8px">
          <!-- 如果有 detail JSON -->
          <div v-if="t.detail && t.detail !== 'null' && t.detail !== '{}'" 
            style="background:#0f172a;color:#e2e8f0;padding:12px;border-radius:8px;font-family:'Courier New',monospace;font-size:12px;white-space:pre-wrap;max-height:400px;overflow-y:auto;line-height:1.6">
            <template v-if="parseDetail(t.detail).log">
              {{ parseDetail(t.detail).log }}
            </template>
            <template v-else>
              {{ formatDetailJson(t.detail) }}
            </template>
          </div>
          <!-- 如果没有 detail，显示 message 全文 -->
          <div v-else-if="t.message && t.message.length > 60"
            style="background:#f8fafc;padding:10px;border-radius:8px;font-size:12px;white-space:pre-wrap;max-height:300px;overflow-y:auto;color:#475569;line-height:1.5;border:1px solid #e2e8f0">
            {{ t.message }}
          </div>
        </div>
      </el-timeline-item>
    </el-timeline>

    <!-- 5项检查结果（来自完整报告） -->
    <div v-if="report && report.checks" style="margin-top:16px">
      <div style="font-weight:600;font-size:13px;margin-bottom:8px;color:#475569">📋 全链路验证详情</div>
      <div v-for="(check, idx) in report.checks" :key="idx" style="margin-bottom:8px">
        <div 
          style="display:flex;align-items:center;gap:12px;padding:10px 12px;border-radius:8px;cursor:pointer"
          :style="{ background: check.passed ? '#f0fdf4' : '#fef2f2', border: '1px solid ' + (check.passed ? '#bbf7d0' : '#fecaca') }"
          @click="toggleCheck(idx)">
          <span style="font-size:16px">{{ check.passed ? '✅' : '❌' }}</span>
          <div style="flex:1">
            <div style="font-weight:600;font-size:13px">{{ check.name }}</div>
          </div>
          <el-tag :type="check.passed ? 'success' : 'danger'" size="small" effect="plain">
            {{ formatMs(check.duration_ms) }}
          </el-tag>
        </div>
        <div v-if="expandedCheck === idx && check.message" 
          style="background:#0f172a;color:#e2e8f0;padding:12px;border-radius:0 0 8px 8px;font-family:'Courier New',monospace;font-size:12px;white-space:pre-wrap;max-height:400px;overflow-y:auto;line-height:1.5">
          {{ check.message }}
        </div>
      </div>
    </div>

    <el-empty v-if="!report && !traces.length" description="暂无测试数据" :image-size="40" />
  </el-card>
</template>

<script setup>
import { ref, computed, onMounted, watch } from 'vue'
import { CircleCloseFilled } from '@element-plus/icons-vue'
import { ArrowDown, ArrowUp } from '@element-plus/icons-vue'

const props = defineProps({ bugId: { type: String, required: true } })

const report = ref(null)
const traces = ref([])
const expandedIdx = ref(null)
const expandedCheck = ref(null)

const isRejected = computed(() => {
  return traces.value.some(t => {
    const msg = (t.message || '') + (t.event || '')
    return msg.includes('rejected: not on develop') || msg.includes('rejected: not deployed')
  })
})

const rejectReason = computed(() => {
  const rejected = traces.value.find(t => {
    const msg = (t.message || '') + (t.event || '')
    return msg.includes('rejected: not on develop') || msg.includes('rejected: not deployed')
  })
  if (!rejected) return ''
  const msg = rejected.message || rejected.event || ''
  if (msg.includes('not on develop')) return '❌ 代码未合入 develop 分支'
  if (msg.includes('not deployed')) return '❌ 代码未部署到测试环境'
  return '❌ 验证被拒绝'
})

const fetchVerification = async () => {
  if (!props.bugId) return
  try {
    const resp = await fetch(`/api/bugs/${props.bugId}/verification`)
    const data = await resp.json()
    report.value = data.full_report || null
    traces.value = data.traces || []
  } catch (e) {
    console.error('Failed to fetch verification:', e)
  }
}

const totalDuration = computed(() => {
  const total = traces.value.reduce((sum, t) => sum + (t.duration_ms || 0), 0)
  return formatMs(total)
})

const toggleDetail = (idx) => {
  expandedIdx.value = expandedIdx.value === idx ? null : idx
}

const toggleCheck = (idx) => {
  expandedCheck.value = expandedCheck.value === idx ? null : idx
}

const hasDetail = (t) => {
  return (t.detail && t.detail !== 'null' && t.detail !== '{}') || 
         (t.message && t.message.length > 60)
}

const formatMs = (ms) => {
  if (!ms || ms === 0) return '-'
  if (ms < 1000) return ms + 'ms'
  return (ms / 1000).toFixed(1) + 's'
}

const formatTime = (ts) => {
  if (!ts) return '-'
  return ts.replace('T', ' ').substring(0, 19)
}

const parseDetail = (detail) => {
  try {
    return typeof detail === 'string' ? JSON.parse(detail) : detail
  } catch { return {} }
}

const formatDetailJson = (detail) => {
  try {
    const obj = typeof detail === 'string' ? JSON.parse(detail) : detail
    if (obj.checks) {
      return obj.checks.map(c => 
        `${c.passed ? '✅' : '❌'} ${c.name}\n   ${c.message?.split('\n')[0] || ''} (${c.duration_ms}ms)`
      ).join('\n\n')
    }
    return JSON.stringify(obj, null, 2)
  } catch { return detail }
}

const eventIcon = (ev) => ({
  'test_generated': '📝',
  'baseline_test': '🔬',
  'regression_test': '🔄',
  'verification': '🧪',
  'verify_start': '🚀',
  'verify_done': '✅',
  'verify_read_testdoc': '📖',
  'verify_diff': '📊',
  'test_done': '🧪',
}[ev] || '📋')

const eventLabel = (ev) => ({
  'test_generated': '测试用例生成',
  'baseline_test': '基线测试',
  'regression_test': '回归测试',
  'verification': '全链路验证',
  'verify_start': '开始验证',
  'verify_done': '验证完成',
  'verify_read_testdoc': '读取测试文档',
  'verify_diff': '代码差异',
  'test_done': '测试完成',
}[ev] || ev)

const tagType = (ev) => ({
  'test_generated': 'success',
  'baseline_test': 'warning',
  'regression_test': '',
  'verification': '',
  'verify_start': 'warning',
  'verify_done': 'success',
  'test_done': 'success',
}[ev] || 'info')

const timelineType = (ev, status) => {
  if (status === 'ok') return 'success'
  if (status === 'failed') return 'danger'
  if (status === 'pending') return 'warning'
  return 'primary'
}

onMounted(fetchVerification)
watch(() => props.bugId, fetchVerification)
</script>
