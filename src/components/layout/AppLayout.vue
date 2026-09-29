<script setup lang="ts">
import TitleBar from './TitleBar.vue'
import AppSidebar from './AppSidebar.vue'
import AppStatusBar from './AppStatusBar.vue'
</script>

<template>
  <div class="h-screen flex flex-col bg-surface text-on-surface font-body overflow-hidden">
    <TitleBar />
    <div class="flex flex-1 min-h-0">
      <AppSidebar />
      <main class="flex-1 min-w-0 overflow-y-auto px-8 py-6">
        <router-view />
      </main>
    </div>
    <AppStatusBar />
  </div>
</template>
