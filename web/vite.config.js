import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 18081,
    proxy: {
      '/api': 'http://127.0.0.1:18081'
    }
  },
  build: {
    outDir: '../static',
    emptyOutDir: true
  }
})
