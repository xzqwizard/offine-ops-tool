import { createRouter, createWebHistory } from 'vue-router'
import { useProjectStore } from '@/stores/project'
import { ElMessage } from 'element-plus'
import { toAppError } from '@/api/backend'
import AppLayout from '@/components/layout/AppLayout.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      component: AppLayout,
      children: [
        {
          path: '',
          name: 'workbench',
          component: () => import('@/views/WorkbenchView.vue')
        },
        {
          path: 'projects',
          name: 'projects',
          component: () => import('@/views/ProjectsView.vue')
        },
        {
          path: 'project/:id',
          name: 'project-edit',
          component: () => import('@/views/ProjectEditView.vue')
        },
        {
          path: 'images',
          name: 'images',
          component: () => import('@/views/ImagesView.vue')
        },
        {
          path: 'docker-pkgs',
          name: 'docker-pkgs',
          component: () => import('@/views/DockerPkgsView.vue')
        },
        {
          path: 'settings',
          name: 'settings',
          component: () => import('@/views/SettingsView.vue')
        }
      ]
    }
  ]
})

router.beforeEach(async (to, from) => {
  if (to.fullPath === from.fullPath) return true
  try { await useProjectStore().flushPending(); return true }
  catch (e) { ElMessage.error(`保存失败，已保留当前页面: ${toAppError(e).message}`); return false }
})
export default router
