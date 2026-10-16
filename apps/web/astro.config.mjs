import { defineConfig } from 'astro/config';
import node from '@astrojs/node';

export default defineConfig({
  output: 'server',
  session: false,
  adapter: node({ mode: 'standalone' }),
  server: { port: 4321 },
  build: { inlineStylesheets: 'never' },
  vite: {
    build: { assetsInlineLimit: 0 },
    server: {
      proxy: { '/api': process.env.API_BASE_URL || 'http://127.0.0.1:3001' },
    },
  },
});
