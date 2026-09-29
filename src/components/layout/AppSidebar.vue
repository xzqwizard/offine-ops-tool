<script setup lang="ts">
import { useRoute } from 'vue-router'
import { computed } from 'vue'

const route = useRoute()

interface NavItem {
  name: string
  label: string
  icon: string
  ready: boolean
}

const navItems: NavItem[] = [
  { name: 'workbench', label: '工作台', icon: 'space_dashboard', ready: true },
  { name: 'projects', label: '方案管理', icon: 'folder_open', ready: true },
  { name: 'images', label: '镜像库', icon: 'dataset', ready: false },
  { name: 'docker-pkgs', label: 'Docker 安装包库', icon: 'archive', ready: false },
  { name: 'settings', label: '设置', icon: 'settings', ready: false }
]

const activeName = computed(() => route.name?.toString() ?? '')
</script>

<template>
  <nav class="w-52 shrink-0 border-r border-outline-variant flex flex-col py-4 select-none">
    <RouterLink
      v-for="item in navItems"
      :key="item.name"
      :to="{ name: item.name }"
      class="mx-3 mb-1 px-3 py-2 rounded-lg flex items-center gap-3 transition-colors"
      :class="[
        activeName === item.name
          ? 'bg-surface-container-high text-primary'
          : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface',
        !item.ready && 'opacity-40'
      ]"
    >
      <span class="material-symbols-outlined text-xl">{{ item.icon }}</span>
      <span class="text-sm font-medium flex-1">{{ item.label }}</span>
      <span
        v-if="!item.ready"
        class="text-[10px] font-mono px-1.5 py-0.5 rounded bg-surface-container-highest"
        >待开发</span
      >
    </RouterLink>

    <div class="mt-auto px-4">
      <div class="text-[10px] font-mono text-on-surface-variant/50 leading-relaxed">
        OfflinePreOpsTool
        <br />
        政务离线交付 · 内网部署
      </div>
    </div>
  </nav>
</template>
