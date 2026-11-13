import { defineConfig } from 'astro/config';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  outDir: './dist',
  server: { port: 4321, host: '127.0.0.1' },
  vite: { plugins: [tailwindcss()], clearScreen: false, server: { strictPort: true } },
});
