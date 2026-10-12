import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 在开发时通过 TAURI_DEV_HOST 暴露给移动端调试；桌面端为空
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [vue()],
  // 让 Tauri CLI 的输出不被 Vite 清屏覆盖
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // src-tauri 由 cargo 自己监听，避免 Vite 重复触发
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // WebView2 基于 Chromium，可放心用较新的 target
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
    rollupOptions: {
      input: {
        // 主界面
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        // 启动动画单独入口：只为一段动画没必要把整个主界面应用加载起来，
        // 而且动画窗在主窗口之前就要出现，越轻越好。
        splash: fileURLToPath(new URL("./splash.html", import.meta.url)),
      },
    },
  },
});
