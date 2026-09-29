import { createRouter, createWebHistory } from 'vue-router'
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

export default router
