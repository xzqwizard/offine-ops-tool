<script setup lang="ts">
import { useTheme } from '@/composables/useTheme'

const { currentTheme, themeList, setTheme } = useTheme()
</script>

<template>
  <el-dropdown trigger="click" @command="setTheme">
    <button
      title="切换主题"
      class="p-2 rounded-lg text-on-surface-variant hover:bg-surface-container-high transition-colors"
    >
      <span class="material-symbols-outlined text-lg">palette</span>
    </button>
    <template #dropdown>
      <el-dropdown-menu>
        <el-dropdown-item
          v-for="t in themeList"
          :key="t.name"
          :command="t.name"
          :class="{ 'is-active-theme': currentTheme === t.name }"
        >
          <span class="flex items-center gap-2">
            <span class="material-symbols-outlined text-base w-4">{{
              currentTheme === t.name ? 'check' : 'radio_button_unchecked'
            }}</span>
            {{ t.label }}
          </span>
        </el-dropdown-item>
      </el-dropdown-menu>
    </template>
  </el-dropdown>
</template>
