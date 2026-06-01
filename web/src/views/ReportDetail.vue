<template>
  <div class="report-detail" v-loading="loading">
    <div style="margin-bottom:16px">
      <el-button @click="$router.back()" size="small">← 返回列表</el-button>
    </div>

    <el-card shadow="never" v-if="report.bug_id">
      <template #header>
        <div style="display:flex;align-items:center;gap:8px">
          <span style="font-family:monospace;color:#60a5fa;font-size:18px">#{{ report.bug_id }}</span>
          <el-tag :type="report.test_result === 'ok' ? 'success' : 'danger'" size="small">
            {{ report.test_result === 'ok' ? '✅ 测试通过' : '❌ 测试失败' }}
          </el-tag>
          <span style="margin-left:auto;font-size:12px;color:#64748b">{{ report.created_at }}</span>
        </div>
      </template>

      <div style="display:flex;gap:16px;margin-bottom:16px">
        <el-statistic title="修复耗时" :value="(report.duration_ms / 1000).toFixed(0)" suffix="s" />
        <el-statistic title="提出人" :value="report.reporter" />
        <el-statistic title="Commit" :value="report.commit_hash?.slice(0,10) || '-'" />
      </div>

      <el-divider />

      <div class="report-content" v-html="renderedMd"></div>
    </el-card>

    <el-empty v-else-if="!loading" description="报告不存在" />
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'

const route = useRoute()
const report = ref({})
const loading = ref(true)

const renderedMd = computed(() => {
  const md = report.value.report_md || ''
  return md
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2 style="border-bottom:1px solid #334155;padding-bottom:8px">$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/`([^`]+)`/g, '<code style="background:#1e293b;padding:2px 6px;border-radius:4px;font-size:13px">$1</code>')
    .replace(/^\| (.+)$/gm, (m) => {
      const cells = m.split('|').filter(c => c.trim()).map(c => `<td style="padding:4px 8px;border:1px solid #334155">${c.trim()}</td>`)
      return `<tr>${cells.join('')}</tr>`
    })
    .replace(/^- (.+)$/gm, '<li>$1</li>')
    .replace(/\n/g, '<br>')
})

onMounted(async () => {
  try {
    const res = await fetch(`/api/bugs/${route.params.id}/report`)
    report.value = await res.json()
  } catch {}
  loading.value = false
})
</script>

<style scoped>
.report-content { font-size: 14px; line-height: 1.8; color: #e2e8f0; }
.report-content h2 { font-size: 18px; margin: 16px 0 8px; color: #f1f5f9; }
.report-content h3 { font-size: 15px; margin: 12px 0 6px; color: #cbd5e1; }
</style>
