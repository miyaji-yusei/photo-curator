import vuetify, { transformAssetUrls } from 'vite-plugin-vuetify'

export default defineNuxtConfig({
  compatibilityDate: '2025-07-15',
  ssr: false,
  devtools: { enabled: true },
  css: ['@mdi/font/css/materialdesignicons.css', '~/assets/main.css'],
  build: { transpile: ['vuetify'] },
  modules: ['@pinia/nuxt'],
  plugins: ['~/plugins/vuetify'],
  vite: {
    plugins: [vuetify({ autoImport: true })],
    vue: { template: { transformAssetUrls } }
  }
})
