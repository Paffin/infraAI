import { createRouter, createWebHistory } from 'vue-router'
import { useApp } from '@/stores/app'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', name: 'login', component: () => import('@/views/LoginView.vue'), meta: { public: true } },
    { path: '/', name: 'projects', component: () => import('@/views/ProjectsView.vue') },
    { path: '/p/:id', name: 'editor', component: () => import('@/views/EditorView.vue'), props: true },
    { path: '/admin/:section?/:sub?', name: 'admin', component: () => import('@/admin/AdminView.vue'), meta: { admin: true } },
    { path: '/:rest(.*)*', redirect: '/' },
  ],
})

router.beforeEach(async (to) => {
  const app = useApp()
  if (!app.ready) await app.init()
  if (to.meta.public) {
    return app.user && !app.user.mustChangePassword ? { name: 'projects' } : true
  }
  if (!app.user) return { name: 'login', query: to.fullPath !== '/' ? { next: to.fullPath } : {} }
  if (app.user.mustChangePassword) return { name: 'login' }
  if (to.meta.admin && !app.isAdmin) return { name: 'projects' }
  return true
})
