import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
import * as ElementPlusIconsVue from '@element-plus/icons-vue'
import App from './App.vue'
import Dashboard from './views/Dashboard.vue'
import Analytics from './views/Analytics.vue'
import Agents from './views/Agents.vue'
import AgentDetail from './views/AgentDetail.vue'
import Queues from './views/Queues.vue'
import BugList from './views/BugList.vue'
import Reports from './views/Reports.vue'
import ReportDetail from './views/ReportDetail.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: Dashboard },
    { path: '/analytics', component: Analytics },
    { path: '/agents', component: Agents },
    { path: '/queues', component: Queues },
    { path: '/agent/:id', component: AgentDetail, props: true },
    { path: '/bugs/:filter', component: BugList, props: true },
    { path: '/reports', component: Reports },
    { path: '/report/:id', component: ReportDetail, props: true },
  ]
})

const app = createApp(App)
app.use(router)
app.use(ElementPlus, { locale: { el: { pagination: { goto: '前往', pagesize: '条/页', total: '共 {total} 条' } } } })
for (const [key, component] of Object.entries(ElementPlusIconsVue)) {
  app.component(key, component)
}
app.mount('#app')
