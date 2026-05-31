<template>
  <el-container class="app-container">
    <el-aside width="200px" class="app-aside">
      <div class="logo">
        <span class="logo-icon">⚙️</span>
        <span class="logo-text">AgentForge</span>
        <el-tag type="primary" size="small" effect="dark">RS</el-tag>
      </div>
      <el-menu
        :default-active="route.path"
        router
        background-color="#1e293b"
        text-color="#94a3b8"
        active-text-color="#60a5fa"
        class="app-menu"
      >
        <el-menu-item index="/">
          <el-icon><DataBoard /></el-icon>
          <span>仪表盘</span>
        </el-menu-item>
        <el-menu-item index="/bugs/unclosed">
          <el-icon><Warning /></el-icon>
          <span>Bug 明细</span>
        </el-menu-item>
        <el-menu-item index="/agents">
          <el-icon><User /></el-icon>
          <span>智能体</span>
        </el-menu-item>
        <el-menu-item index="/queues">
          <el-icon><List /></el-icon>
          <span>队列</span>
        </el-menu-item>
        <el-menu-item index="/analytics">
          <el-icon><TrendCharts /></el-icon>
          <span>L4 分析</span>
        </el-menu-item>
      </el-menu>
      <div class="nav-footer">
        <el-tag :type="connected ? 'success' : 'danger'" size="small" effect="dark">
          {{ connected ? '🟢 已连接' : '🔴 断开' }}
        </el-tag>
      </div>
    </el-aside>
    <el-main class="app-main">
      <router-view />
    </el-main>
  </el-container>
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { useRoute } from 'vue-router'
import { DataBoard, Warning, User, List, TrendCharts } from '@element-plus/icons-vue'

const route = useRoute()
const connected = ref(false)
let timer = null

onMounted(async () => {
  try {
    const r = await fetch('/api/health')
    const d = await r.json()
    connected.value = d.ok
  } catch { connected.value = false }
  timer = setInterval(async () => {
    try {
      const r = await fetch('/api/health')
      const d = await r.json()
      connected.value = d.ok
    } catch { connected.value = false }
  }, 10000)
})

onUnmounted(() => clearInterval(timer))
</script>

<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #0f172a; color: #e2e8f0; }
.app-container { height: 100vh; }
.app-aside {
  background: #1e293b; border-right: 1px solid #334155;
  display: flex; flex-direction: column; overflow: hidden;
}
.logo {
  display: flex; align-items: center; gap: 8px; padding: 16px 20px;
  border-bottom: 1px solid #334155;
}
.logo-icon { font-size: 22px; }
.logo-text { font-size: 16px; font-weight: 700; color: #f8fafc; flex: 1; }
.app-menu { border-right: none !important; flex: 1; }
.app-main { background: #0f172a; padding: 20px; overflow-y: auto; }
.nav-footer { padding: 12px 20px; border-top: 1px solid #334155; }

/* Element Plus 深色主题覆盖 */
:root {
  --el-bg-color: #1e293b;
  --el-bg-color-overlay: #1e293b;
  --el-text-color-primary: #e2e8f0;
  --el-text-color-regular: #94a3b8;
  --el-border-color: #334155;
  --el-fill-color-blank: #0f172a;
}
.el-table { --el-table-bg-color: #1e293b; --el-table-tr-bg-color: #1e293b; --el-table-header-bg-color: #0f172a; --el-table-row-hover-bg-color: rgba(59,130,246,0.08); --el-table-border-color: #334155; --el-table-text-color: #e2e8f0; }
.el-table .el-table__row--striped td { background: #0f172a !important; }
.el-card { --el-card-bg-color: #1e293b; border-color: #334155; }
.el-input__wrapper { background: #0f172a !important; box-shadow: 0 0 0 1px #334155 inset !important; }
.el-input__inner { color: #e2e8f0 !important; }
.el-input__inner::placeholder { color: #475569 !important; }
.el-tabs { --el-tabs-header-height: 42px; }
.el-tabs__header { background: #1e293b !important; border-color: #334155 !important; border-radius: 8px 8px 0 0; }
.el-tabs__item { color: #64748b !important; }
.el-tabs__item.is-active { color: #60a5fa !important; background: #0f172a; }
.el-tabs__content { background: #0f172a; padding: 16px; border-radius: 0 0 8px 8px; }
.el-badge__content { font-size: 10px; }
.el-statistic__head { color: #64748b !important; }
.el-statistic__content { color: #e2e8f0 !important; }
</style>
