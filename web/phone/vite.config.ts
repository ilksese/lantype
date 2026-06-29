import { defineConfig } from 'vite'
import preact from '@preact/preset-vite'
import stylex from '@stylexjs/unplugin'
import { viteSingleFile } from 'vite-plugin-singlefile'

export default defineConfig(({ command }) => ({
  plugins: [
    stylex.vite({
      dev: command === 'serve',
      runtimeInjection: true,
      genConditionalClasses: true,
      useCSSLayers: true,
    }),
    preact(),
    viteSingleFile(),
  ],
  build: {
    cssCodeSplit: false,
    assetsInlineLimit: 100_000,
  },
}))