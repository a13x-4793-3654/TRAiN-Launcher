import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // ネットワーク共有(UNCパス)にマップされたドライブ上でビルドすると、
  // Windows/Vite側のドライブレター⇔UNCパス解決の実装上の制約により
  // realpath解決が壊れることがあるため、シンボリックリンク解決を無効化する。
  resolve: {
    preserveSymlinks: true,
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` (Tauriのビルド成果物/gen/を監視しない)
      ignored: ["**/src-tauri/**", "**/gen/**"],
    },
  },
}));
