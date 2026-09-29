<template>
  <div class="max-w-4xl mx-auto">
    <section class="mb-8">
      <p class="font-mono text-sm text-on-surface-variant flex items-center gap-2 mb-2">
        <span class="w-2.5 h-2.5 bg-success rounded-full pulsing-orb"></span>
        离线交付工作台
      </p>
      <h1 class="font-headline text-3xl font-bold tracking-tight">离线部署运维工具</h1>
      <p class="text-on-surface-variant text-sm mt-2">
        面向政务内网环境：定义服务器与中间件方案 → 跨架构拉取镜像 → 生成每台服务器自包含离线包
      </p>
    </section>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <RouterLink
        :to="{ name: 'projects' }"
        class="bg-surface-container-low p-6 rounded-xl border border-outline-variant hover:border-primary transition-colors block"
      >
        <div class="flex items-center gap-3 mb-3">
          <span class="material-symbols-outlined text-primary">folder_open</span>
          <h2 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
            方案管理
          </h2>
        </div>
        <p class="text-sm text-on-surface-variant">新建部署方案，管理服务器清单、中间件编排与端口矩阵</p>
      </RouterLink>

      <div
        class="bg-surface-container-low p-6 rounded-xl border border-outline-variant opacity-50"
      >
        <div class="flex items-center gap-3 mb-3">
          <span class="material-symbols-outlined text-primary">rocket_launch</span>
          <h2 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
            构建中心
          </h2>
        </div>
        <p class="text-sm text-on-surface-variant">镜像拉取与离线包构建（随方案编辑页提供）</p>
      </div>
    </div>
  </div>
</template>
