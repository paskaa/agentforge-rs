<template>
  <el-card shadow="never" style="margin-bottom:16px" v-if="report || traces.length">
    <template #header>
      <div style="display:flex;align-items:center;gap:8px">
        <span>🔬 全链路验证</span>
        <el-tag v-if="report" :type="report.all_passed ? 'success' : 'danger'" size="small" effect="dark">
          {{ report.all_passed ? '✅ 全部通过' : '❌ 验证失败' }}
        </el-tag>
        <span v-if="report" style="color:#64748b;font-size:12px;margin-left:auto">
          {{ report.total_ms }}ms · {{ report.checks?.length || 0 }} 项检查
        </span>
      </div>
    </template>

    <!-- 5项检查结果 -->
    <div v-if="report && report.checks" style="margin-bottom:16px">
      <div v-for="(check, idx) in report.checks" :key="idx" style="margin-bottom:8px">
        <div 
          style="display:flex;align-items:center;gap:12px;padding:10px 12px;border-radius:8px;cursor:pointer"
          :style="{ background: check.passed ? '#f0fdf4' : '#fef2f2', border: '1px solid ' + (check.passed ? '#bbf7d0' : '#fecaca') }"
          @click="toggleCheck(idx)">
          <span style="font-size:18px">{{ check.passed ? '✅' : '❌' }}</span>
          <div style="flex:1">
            <div style="font-weight:600;font-size:13px">{{ check.name }}</div>
            <div style="color:#64748b;font-size:12px;margin-top:2px;white-space:pre-wrap;line-height:1.4">
              {{ expandCheck === idx ? check.message : shortMessage(check.message) }}
            </div>
          </div>
          <el-tag :type="check.passed ? 'success' : 'danger'" size="small" effect="plain">
            {{ formatMs(check.duration_ms) }}
          </el-tag>
          <el-icon v-if="check.message && check.message.length > 60">
            <ArrowDown v-if="expandCheck !== idx" />
            <ArrowUp v-else />
          </el-icon>
        </div>
        <!-- 展开的完整日志 -->
        <div v-if="expandCheck === idx && check.message" 
          style="background:#1e293b;color:#e2e8f0;padding:12px;border-radius:0 0 8px 8px;font-family:monospace;font-size:12px;white-space:pre-wrap;max-height:400px;overflow-y:auto;line-height:1.5">
          {{ check.message }}
        </div>
      </div>
    </div>

    <!-- 验证流程时间线 -->
    <div v-if="traces.length" style="margin-top:12px">
      <div style="font-weight:600;font-size:13px;margin-bottom:8px;color:#475569">📋 验证流程节点</div>
      <el-timeline>
        <el-timeline-item
          v-for="(t, idx) in traces" :key="idx"
          :timestamp="formatTime(t.ts)"
          :type="timelineType(t.event, t.status)"
          :hollow="t.status !== 'ok'"
          placement="top">
          <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
            <el-tag :type="tagType(t.event)" size="small" effect="plain">{{ eventLabel(t.event) }}</el-tag>
            <span style="color:#475569;font-size:13px">{{ t.message || '-' }}</span>
            <el-tag v-if="t.duration_ms > 0" type="info" size="small" effect="plain" style="margin-left:auto">
              {{ formatMs(t.duration_ms) }}
            </el-tag>
          </div>
          <!-- 展示 detail JSON（如果有） -->
          <div v-if="t.detail && t.detail !== 'null' && t.detail !== '{}'" 
            style="margin-top:8px;background:#f8fafc;padding:8px;border-radius:6px;font-size:12px;max-height:200px;overflow-y:auto">
            <div v-for="(val, key) in parseDetail(t.detail)" :key="key" style="margin-bottom:4px">
              <span style="color:#64748b;font-weight:600">{{ key }}:</span>
              <span style="color:#334155">{{ typeof val === 'object' ? JSON.stringify(val) : val }}</span>
            </div>
          </div>
        </el-timeline-item>
      </el-timeline>
    </div>

    <el-empty v-if="!report && !traces.length" description="暂无验证数据" :image-size="40" />
  </el-card>
</template>

<script setup>
import { ref, onMounted, watch } from 'vue'
import { ArrowDown, ArrowUp } from '@element-plus/icons-vue'

const props = defineProps({ bugId: { type: String, required: true } })

const report = ref(null)
const traces = ref([])
const expandCheck = ref(null)

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

const toggleCheck = (idx) => {
  expandCheck.value = expandCheck.value === idx ? null : idx
}

const shortMessage = (msg) => {
  if (!msg) return '-'
  const firstLine = msg.split('\n')[0]
  return firstLine.length > 80 ? firstLine.substring(0, 80) + '...' : firstLine
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
    const obj = typeof detail === 'string' ? JSON.parse(detail) : detail
    if (obj && obj.checks) {
      // VerificationReport 格式
      return {
        'Bug': obj.bug_id,
        'Agent': obj.agent_id,
        '通过': obj.all_passed ? '✅' : '❌',
        '总耗时': obj.total_ms + 'ms',
        '检查项': obj.checks.map(c => `${c.passed ? '✅' : '❌'} ${c.name}: ${c.message?.split('\n')[0]}`).join('\n'),
      }
    }
    return obj || {}
  } catch { return {} }
}

const eventLabel = (ev) => ({
  'verification': '全链路验证',
  'verify_start': '🚀 开始验证',
  'verify_done': '✅ 验证完成',
  'verify_read_testdoc': '📖 读取测试文档',
  'verify_diff': '📊 代码差异',
  'test': '🧪 测试',
  'test_done': '🧪 测试完成',
}[ev] || ev)

const tagType = (ev) => ({
  'verification': '',
  'verify_start': 'warning',
  'verify_done': 'success',
  'verify_read_testdoc': 'info',
  'verify_diff': '',
  'test': 'warning',
  'test_done': 'success',
}[ev] || 'info')

const timelineType = (ev, status) => {
  if (ev === 'verification' || ev === 'verify_done' || ev === 'test_done') 
    return status === 'ok' ? 'success' : 'danger'
  return 'primary'
}

onMounted(fetchVerification)
watch(() => props.bugId, fetchVerification)
</script>
