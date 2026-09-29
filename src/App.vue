<script setup lang="ts">
import { useTheme } from '@/composables/useTheme'

// 只负责主题初始化；布局由路由根组件 AppLayout 承载（避免双层嵌套）
useTheme()
</script>

<template>
  <router-view />
</template>
