<script setup lang="ts">
import { useTheme } from '@/composables/useTheme'

useTheme()
</script>

<template>
  <div class="min-h-screen bg-surface text-on-surface font-body">
    <main class="flex flex-col items-center justify-center h-screen gap-6">
      <h1 class="font-headline text-3xl font-bold tracking-tight">离线部署运维工具</h1>
      <p class="font-mono text-sm text-on-surface-variant">OfflinePreOpsTool · skeleton OK</p>
    </main>
  </div>
</template>
