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

    <!-- 5项检查结果 (来自完整报告) -->
    <div v-if="report && report.checks" style="margin-bottom:16px">
      <div v-for="(check, idx) in report.checks" :key="idx"
        style="display:flex;align-items:center;gap:12px;padding:10px 12px;margin-bottom:6px;border-radius:8px"
        :style="{ background: check.passed ? '#f0fdf4' : '#fef2f2', border: '1px solid ' + (check.passed ? '#bbf7d0' : '#fecaca') }">
        <span style="font-size:18px">{{ check.passed ? '✅' : '❌' }}</span>
        <div style="flex:1">
          <div style="font-weight:600;font-size:13px">{{ check.name }}</div>
          <div style="color:#64748b;font-size:12px;margin-top:2px">{{ check.message }}</div>
        </div>
        <el-tag :type="check.passed ? 'success' : 'danger'" size="small" effect="plain">
          {{ check.duration_ms }}ms
        </el-tag>
      </div>
    </div>

    <!-- 验证流程时间线 (来自 traces) -->
    <div v-if="traces.length" style="margin-top:12px">
      <div style="font-weight:600;font-size:13px;margin-bottom:8px;color:#475569">📋 验证流程节点</div>
      <el-timeline>
        <el-timeline-item
          v-for="(t, idx) in traces" :key="idx"
          :timestamp="formatTime(t.ts)"
          :type="timelineType(t.event, t.status)"
          :hollow="t.status !== 'ok'"
          placement="top">
          <div style="display:flex;align-items:center;gap:8px">
            <el-tag :type="tagType(t.event)" size="small" effect="plain">{{ eventLabel(t.event) }}</el-tag>
            <span style="color:#475569;font-size:13px">{{ t.message || '-' }}</span>
          </div>
        </el-timeline-item>
      </el-timeline>
    </div>

    <el-empty v-if="!report && !traces.length" description="暂无验证数据" :image-size="40" />
  </el-card>
</template>

<script setup>
import { ref, onMounted, watch } from 'vue'

const props = defineProps({ bugId: { type: String, required: true } })

const report = ref(null)
const traces = ref([])

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

const formatTime = (ts) => {
  if (!ts) return '-'
  return ts.replace('T', ' ').substring(0, 19)
}

const eventLabel = (ev) => {
  const map = {
    'verification': '全链路验证',
    'verify_start': '🚀 开始验证',
    'verify_done': '✅ 验证完成',
    'verify_read_testdoc': '📖 读取测试文档',
    'verify_diff': '📊 代码差异',
  }
  return map[ev] || ev
}

const tagType = (ev) => {
  const map = {
    'verification': '',
    'verify_start': 'warning',
    'verify_done': 'success',
    'verify_read_testdoc': 'info',
    'verify_diff': '',
  }
  return map[ev] || 'info'
}

const timelineType = (ev, status) => {
  if (ev === 'verification' || ev === 'verify_done') return status === 'ok' ? 'success' : 'danger'
  return 'primary'
}

onMounted(fetchVerification)
watch(() => props.bugId, fetchVerification)
</script>
