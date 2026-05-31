<template>
  <div class="app">
    <nav class="sidebar">
      <div class="logo">
        <span class="logo-icon">⚙️</span>
        <span class="logo-text">AgentForge</span>
        <span class="logo-badge">RS</span>
      </div>
      <router-link to="/" class="nav-item" active-class="active">
        <span>📊</span> 仪表盘
      </router-link>
      <router-link to="/analytics" class="nav-item" active-class="active">
        <span>📈</span> L4 分析
      </router-link>
      <router-link to="/agents" class="nav-item" active-class="active">
        <span>🤖</span> 智能体
      </router-link>
      <div class="nav-footer">
        <div class="status-dot" :class="connected ? 'online' : 'offline'"></div>
        <span>{{ connected ? '已连接' : '断开' }}</span>
      </div>
    </nav>
    <main class="content">
      <router-view />
    </main>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue'

const connected = ref(false)
let timer = null

onMounted(async () => {
  try {
    const r = await fetch('/api/health')
    connected.value = r.ok
  } catch { connected.value = false }
  timer = setInterval(async () => {
    try {
      const r = await fetch('/api/health')
      connected.value = r.ok
    } catch { connected.value = false }
  }, 10000)
})

onUnmounted(() => clearInterval(timer))
</script>

<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #0f172a; color: #e2e8f0; }
.app { display: flex; height: 100vh; }

.sidebar {
  width: 220px; background: #1e293b; border-right: 1px solid #334155;
  display: flex; flex-direction: column; padding: 16px 0;
}
.logo { display: flex; align-items: center; gap: 8px; padding: 0 20px 20px; border-bottom: 1px solid #334155; }
.logo-icon { font-size: 24px; }
.logo-text { font-size: 18px; font-weight: 700; color: #f8fafc; }
.logo-badge { font-size: 11px; background: #3b82f6; color: white; padding: 2px 6px; border-radius: 4px; font-weight: 600; }

.nav-item {
  display: flex; align-items: center; gap: 10px; padding: 12px 20px;
  color: #94a3b8; text-decoration: none; transition: all 0.2s; font-size: 14px;
}
.nav-item:hover { background: #334155; color: #e2e8f0; }
.nav-item.active { background: #1e3a5f; color: #60a5fa; border-right: 3px solid #3b82f6; }

.nav-footer {
  margin-top: auto; padding: 16px 20px; border-top: 1px solid #334155;
  display: flex; align-items: center; gap: 8px; font-size: 12px; color: #64748b;
}
.status-dot { width: 8px; height: 8px; border-radius: 50%; }
.status-dot.online { background: #22c55e; box-shadow: 0 0 6px #22c55e; }
.status-dot.offline { background: #ef4444; }

.content { flex: 1; overflow-y: auto; padding: 24px; }
</style>
