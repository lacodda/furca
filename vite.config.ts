import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath } from 'node:url'
import { version } from './package.json'

// Tauri serves the frontend from a fixed port and expects a static build in dist/.
export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    // The splash in index.html prints the version before the bundle - and
    // `define` below - has loaded, so the number is written into the page
    // itself at build time.
    {
      name: 'furca:splash-version',
      transformIndexHtml: (html) => html.replace('__APP_VERSION__', version),
    },
  ],
  define: {
    __APP_VERSION__: JSON.stringify(version),
  },
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
})
