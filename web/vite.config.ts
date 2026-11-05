import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// 开发模式：前端 5173 端口，/api 请求转发给 Rust 后端（默认 8787）
export default defineConfig({
  plugins: [react()],
  test: { environment: "jsdom" },
  server: {
    port: 5173,
    proxy: {
      "/api": { target: "http://127.0.0.1:8787", changeOrigin: true },
    },
  },
});
