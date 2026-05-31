import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import App from './App.vue'
import Dashboard from './views/Dashboard.vue'
import Analytics from './views/Analytics.vue'
import Agents from './views/Agents.vue'
import AgentDetail from './views/AgentDetail.vue'
import Queues from './views/Queues.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: Dashboard },
    { path: '/analytics', component: Analytics },
    { path: '/agents', component: Agents },
    { path: '/queues', component: Queues },
    { path: '/agent/:id', component: AgentDetail, props: true },
  ]
})

createApp(App).use(router).mount('#app')
