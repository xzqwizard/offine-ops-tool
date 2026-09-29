<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import pkg from '../../../package.json'
import ThemeSwitcher from './ThemeSwitcher.vue'

// 非 Tauri 环境（纯浏览器开发/预览）下禁用窗口 API，避免抛错中断渲染
const inTauri = '__TAURI_INTERNALS__' in window
const appWindow = inTauri ? getCurrentWindow() : null

const isMaximized = ref(false)
let unlisten: (() => void) | null = null
let disposed = false

async function refreshMaxState() {
  if (!appWindow) return
  try {
    isMaximized.value = await appWindow.isMaximized()
  } catch {
    /* ignore */
  }
}

onMounted(async () => {
  await refreshMaxState()
  if (!appWindow) return
  try {
    const fn = await appWindow.onResized(refreshMaxState)
    if (disposed) fn()
    else unlisten = fn
  } catch {
    /* ignore */
  }
})

onUnmounted(() => {
  disposed = true
  unlisten?.()
})

async function toggleMaximize() {
  if (!appWindow) return
  try {
    await appWindow.toggleMaximize()
    await refreshMaxState()
  } catch {
    /* ignore */
  }
}
</script>

<template>
  <header
    class="glass-panel h-10 shrink-0 flex justify-between items-center pl-4 pr-2 border-b border-outline-variant select-none"
    data-tauri-drag-region
  >
    <div class="flex items-center gap-3" data-tauri-drag-region>
      <img src="/app-icon.png" alt="logo" class="w-5 h-5 rounded" data-tauri-drag-region />
      <span class="text-sm font-bold font-headline tracking-tight" data-tauri-drag-region
        >离线部署运维工具</span
      >
      <div class="h-3.5 w-px bg-outline-variant" data-tauri-drag-region></div>
      <span class="font-headline font-medium text-[11px] tracking-tight text-primary opacity-70">
        V{{ pkg.version }}
      </span>
    </div>
    <div class="flex items-center gap-1">
      <ThemeSwitcher />
      <template v-if="appWindow">
        <button
          title="最小化"
          class="p-2 rounded-lg text-on-surface-variant hover:bg-surface-container-high transition-colors"
          @click="appWindow.minimize()"
        >
          <span class="material-symbols-outlined text-lg">remove</span>
        </button>
        <button
          title="最大化/还原"
          class="p-2 rounded-lg text-on-surface-variant hover:bg-surface-container-high transition-colors"
          @click="toggleMaximize"
        >
          <span class="material-symbols-outlined text-lg">{{
            isMaximized ? 'filter_none' : 'crop_square'
          }}</span>
        </button>
      </template>
      <button
        title="关闭"
        class="p-2 rounded-lg text-on-surface-variant hover:bg-error-container/50 hover:text-error transition-colors"
        @click="appWindow?.close()"
      >
        <span class="material-symbols-outlined text-lg">close</span>
      </button>
    </div>
  </header>
</template>
