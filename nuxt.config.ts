import vuetify, { transformAssetUrls } from 'vite-plugin-vuetify'

// GitHub Pages はリポジトリ名のサブパスで配信されるが、Tauri は
// `tauri://localhost` の直下から読む。**同じ出力先（.output/public）を
// 両方で使う**ので、固定値を書くとどちらかが 404 で白画面になる。
// 既定は `/` のまま（= デスクトップは今までどおり）、Pages 用ビルドだけ
// `NUXT_APP_BASE_URL=/photo-curator/` を与える。
// `process` の型は入れていない（@types/node を足さない）ので globalThis 経由で読む。
const environment = (globalThis as { process?: { env?: Record<string, string | undefined> } })
  .process?.env ?? {}
const baseURL = environment.NUXT_APP_BASE_URL ?? '/'

export default defineNuxtConfig({
  compatibilityDate: '2025-07-15',
  ssr: false,
  app: {
    baseURL,
    head: {
      // iPad でホーム画面に追加したときに、アプリとして開くための指定。
      meta: [
        { name: 'viewport', content: 'width=device-width, initial-scale=1, viewport-fit=cover' },
        { name: 'theme-color', content: '#101114' },
        { name: 'apple-mobile-web-app-capable', content: 'yes' },
        { name: 'apple-mobile-web-app-status-bar-style', content: 'black-translucent' },
        { name: 'apple-mobile-web-app-title', content: 'Curator' }
      ],
      link: [
        { rel: 'manifest', href: `${baseURL}manifest.webmanifest` },
        { rel: 'apple-touch-icon', sizes: '180x180', href: `${baseURL}icon-180.png` },
        { rel: 'icon', type: 'image/png', sizes: '192x192', href: `${baseURL}icon-192.png` }
      ]
    }
  },
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
