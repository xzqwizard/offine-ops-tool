<script setup lang="ts">
import pkg from '../../../package.json'
import { ref } from 'vue'
import { useTaskStore } from '@/stores/tasks'
import { ElMessage } from 'element-plus'
import { toAppError } from '@/api/backend'
const tasks = useTaskStore()
const open = ref(false)
const statusText = { running: '运行中', cancelling: '取消中', complete: '完成', failed: '失败', cancelled: '已取消' }
const cancel = (id: string) => tasks.cancel(id).catch(e => ElMessage.error(toAppError(e).message))
</script>

<template>
  <footer
    class="glass-panel h-9 shrink-0 flex justify-between items-center px-4 border-t border-outline-variant select-none"
  >
    <div class="font-mono text-xs text-on-surface-variant flex items-center gap-2">
      <span class="w-2 h-2 bg-success rounded-full pulsing-orb"></span>
      <button @click="open = true">{{ tasks.busy ? `${tasks.active.length} 个任务运行中` : '就绪' }} · 查看任务</button>
    </div>
    <div class="font-mono text-[10px] text-on-surface-variant/50">
      v{{ pkg.version }} · {{ new Date().getFullYear() }}
    </div>
  </footer>
  <el-drawer v-model="open" title="任务记录" size="580px">
    <el-empty v-if="!tasks.records.length" description="暂无任务" />
    <section v-for="task in tasks.records" :key="task.id" class="mb-5 border-b border-outline-variant pb-4">
      <div class="flex items-center justify-between"><strong>{{ task.label }}</strong><span>{{ statusText[task.status] }}</span></div>
      <p class="text-xs text-on-surface-variant mt-1">{{ task.elapsedMs ? `${(task.elapsedMs / 1000).toFixed(1)} 秒` : '正在处理' }}</p>
      <el-progress v-if="task.progress" :percentage="Math.round(task.progress.percent)" />
      <el-button v-if="task.status === 'running'" size="small" class="mt-2" @click="cancel(task.id)">取消任务</el-button>
      <p v-if="task.error" class="text-error text-xs mt-2">{{ task.error }}</p>
      <pre v-if="task.logs.length" class="text-xs mt-2 whitespace-pre-wrap max-h-56 overflow-auto">{{ task.logs.join('\n') }}</pre>
    </section>
  </el-drawer>
</template>
