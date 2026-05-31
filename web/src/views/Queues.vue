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
            <router-link :to="'/agent/' + q.agent" style="margin-left:auto;text-decoration:none">
              <el-button size="small" type="primary" link>查看活动 →</el-button>
            </router-link>
          </div>
          <div v-if="q.items.length > 0" style="padding:0">
            <div v-for="(item, i) in q.items" :key="i" style="display:flex;gap:12px;padding:8px 16px;border-bottom:1px solid rgba(51,65,85,0.3);font-size:13px">
              <span style="color:#60a5fa;font-family:monospace;font-weight:500;min-width:50px">#{{ item.bug_id }}</span>
              <span style="color:#64748b">{{ item.source || 'pipeline' }}</span>
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

const queues = ref([])
let pollTimer = null

const totalItems = computed(() => queues.value.reduce((s, q) => s + q.queue_len, 0))

const agentIcons = { guanyu:'⚔️', zhaoyun:'🐉', xunyu:'📚', zhangfei:'🔥', huatuo:'💊', chenlin:'📝', liubei:'👑', zhugeliang:'🪶' }
const agentNames = { guanyu:'关羽', zhaoyun:'赵云', xunyu:'荀彧', zhangfei:'张飞', huatuo:'华佗', chenlin:'陈琳', liubei:'刘备', zhugeliang:'诸葛亮' }

function agentIcon(id) { return agentIcons[id] || '🤖' }
function agentName(id) { return agentNames[id] || id }

async function fetchQueues() {
  try {
    const r = await fetch('/api/queues')
    queues.value = await r.json()
  } catch {}
}

onMounted(() => { fetchQueues(); pollTimer = setInterval(fetchQueues, 5000) })
onUnmounted(() => clearInterval(pollTimer))
</script>
