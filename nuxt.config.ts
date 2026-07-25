import vuetify, { transformAssetUrls } from 'vite-plugin-vuetify'

export default defineNuxtConfig({
  compatibilityDate: '2025-07-15',
  ssr: false,
  devtools: { enabled: true },
  // vite-plugin-vuetify の autoImport が入れるのはコンポーネント個別のスタイル
  // だけ。余白・flex・typography のユーティリティ（ga-4 / mb-8 / pa-7 /
  // d-flex / text-h3 など）と v-card__overlay の基準値は 'vuetify/styles' 側に
  // あり、これが無いと画面の間隔が一切効かず、カードのホバーでオーバーレイが
  // 全開になって白く飛ぶ。main.css は上書き側なので必ず最後に置く。
  css: ['vuetify/styles', '@mdi/font/css/materialdesignicons.css', '~/assets/main.css'],
  // ルートルールやペイロード再検証のための仕組みで、サーバを持たない
  // デスクトップアプリでは使わない。有効なままだと /_nuxt/builds/meta/*.json
  // を取りに行って毎回コンソールにエラーが出る。
  experimental: { appManifest: false },
  build: { transpile: ['vuetify'] },
  modules: ['@pinia/nuxt'],
  plugins: ['~/plugins/vuetify'],
  vite: {
    plugins: [vuetify({ autoImport: true })],
    vue: { template: { transformAssetUrls } }
  }
})
