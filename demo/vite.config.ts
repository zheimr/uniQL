import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import wasm from 'vite-plugin-wasm';

export default defineConfig({
  base: '/uniQL/',
  plugins: [react(), tailwindcss(), wasm()],
  build: {
    target: 'esnext',
  },
  optimizeDeps: {
    exclude: ['uniql-wasm'],
  },
});
