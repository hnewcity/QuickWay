import { defineConfig } from 'vite';

export default defineConfig({
  build: { target: 'es2022' },
  clearScreen: false,
  // Cargo writes into src-tauri/target; watching it crashes Vite on Windows (EBUSY on the linked .exe).
  server: { watch: { ignored: ['**/src-tauri/**'] } },
});
