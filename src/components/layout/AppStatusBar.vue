<script setup lang="ts">
import pkg from '../../../package.json'
</script>

<template>
  <footer
    class="glass-panel h-9 shrink-0 flex justify-between items-center px-4 border-t border-outline-variant select-none"
  >
    <div class="font-mono text-xs text-on-surface-variant flex items-center gap-2">
      <span class="w-2 h-2 bg-success rounded-full pulsing-orb"></span>
      <span>就绪</span>
    </div>
    <div class="font-mono text-[10px] text-on-surface-variant/50">
      v{{ pkg.version }} · {{ new Date().getFullYear() }}
    </div>
  </footer>
</template>
