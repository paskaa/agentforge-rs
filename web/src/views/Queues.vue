<template>
  <div>
    <h1 style="margin-bottom:8px;font-size:22px">📋 队列状态</h1>
    <div style="color:#64748b;font-size:14px;margin-bottom:20px">
      共 <el-tag type="primary" size="small">{{ totalItems }}</el-tag> 个待处理任务
    </div>

    <el-row :gutter="16">
      <el-col :span="12" v-for="q in queues" :key="q.agent">
        <el-card shadow="hover" body-style="padding:0" style="margin-bottom:16px">
          <div style="display:flex;align-items:center;gap:10px;padding:14px 16px;border-bottom:1px solid #334155">
            <span style="font-size:20px">{{ agentIcon(q.agent) }}</span>
            <span style="font-weight:600;font-size:14px">{{ agentName(q.agent) }}</span>
            <el-badge :value="q.queue_len" :type="q.queue_len > 0 ? 'primary' : 'info'" />
            <el-tag v-if="q.processing" type="success" size="small" effect="dark" style="margin-left:4px">🔄 处理中</el-tag>
            <router-link :to="'/agent/' + q.agent" style="margin-left:auto;text-decoration:none">
              <el-button size="small" type="primary" link>详情 →</el-button>
            </router-link>
          </div>
          <div v-if="q.items.length > 0" style="padding:0">
            <div v-for="(item, i) in q.items" :key="i"
              style="padding:10px 16px;border-bottom:1px solid rgba(51,65,85,0.3);font-size:13px">
              <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px">
                <a :href="zentaoBugUrl(item.bug_id)" target="_blank"
                  style="color:#60a5fa;font-family:monospace;font-weight:500;text-decoration:none">
                  #{{ item.bug_id }}
                  <el-icon style="font-size:10px;margin-left:2px"><Link /></el-icon>
                </a>
                <el-tag :type="item.source === 'processing' ? 'success' : item.source === 'web_ui' ? 'warning' : 'info'" size="small" effect="plain">
                  {{ item.source === 'processing' ? '🔄 处理中' : item.source === 'web_ui' ? '📥 入列' : '📋 ' + item.source }}
                </el-tag>
                <span v-if="item.queued_at && item.queued_at !== '正在处理'" style="color:#475569;font-size:11px;margin-left:auto">
                  {{ item.queued_at.substring(11, 19) }}
                </span>
                <span v-else-if="item.queued_at === '正在处理'" style="color:#22c55e;font-size:11px;margin-left:auto">
                  ⏳ {{ item.queued_at }}
                </span>
              </div>
              <!-- 流水线节点进度 -->
              <PipelineProgress :bugId="item.bug_id" />
            </div>
          </div>
          <el-empty v-else description="队列为空" :image-size="40" />
        </el-card>
      </el-col>
    </el-row>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { Link } from '@element-plus/icons-vue'
import PipelineProgress from '../components/PipelineProgress.vue'

const queues = ref([])
let pollTimer = null

const totalItems = computed(() => queues.value.reduce((s, q) => s + q.queue_len, 0))

const agentIcons = { guanyu:'⚔️', zhaoyun:'🐉', xunyu:'📚', zhangfei:'🔥', huatuo:'💊', chenlin:'📝', liubei:'👑', zhugeliang:'🪶' }
const agentNames = { guanyu:'关羽', zhaoyun:'赵云', xunyu:'荀彧', zhangfei:'张飞', huatuo:'华佗', chenlin:'陈琳', liubei:'刘备', zhugeliang:'诸葛亮' }

function agentIcon(id) { return agentIcons[id] || '🤖' }
function agentName(id) { return agentNames[id] || id }
function zentaoBugUrl(bugId) {
  const id = String(bugId || '').replace('Bug#', '')
  return `https://zentao.gentronhealth.com/index.php?m=bug&f=view&bugID=${id}`
}

async function fetchQueues() {
  try {
    const r = await fetch('/api/queues')
    queues.value = await r.json()
  } catch {}
}

onMounted(() => { fetchQueues(); pollTimer = setInterval(fetchQueues, 5000) })
onUnmounted(() => clearInterval(pollTimer))
</script>
