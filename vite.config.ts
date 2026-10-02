import { defineConfig } from 'vite'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

// Tauri 期望固定端口；clearScreen 保留 Rust 编译错误输出
export default defineConfig({
  plugins: [vue(), tailwindcss(), Components({ dts: false, resolvers: [ElementPlusResolver({ importStyle: 'css' })] })],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true
  },
  build: {
    target: 'es2021',
    sourcemap: false,
    rollupOptions: { output: { manualChunks(id) {
      if (!id.includes('node_modules')) return
      if (id.includes('lodash')) return 'utility'
      if (id.includes('dayjs')) return 'date'
      if (id.includes('@vue-flow')) return 'topology'
    } } }
  }
})
