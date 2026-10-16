import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // 보안: TAURI_ 접두어를 넣으면 업데이터 서명 키가 번들에 포함될 수 있음 (GHSA-2rcp-jvr4-r259)
  envPrefix: ["VITE_"],
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // 데스크톱 앱이라 xterm.js를 포함한 단일 번들이 커도 문제없음
    chunkSizeWarningLimit: 1500,
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
