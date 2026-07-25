import { createVuetify } from 'vuetify'

export default defineNuxtPlugin((nuxtApp) => {
  nuxtApp.vueApp.use(createVuetify({
    theme: {
      defaultTheme: 'photoCurator',
      themes: {
        photoCurator: {
          dark: true,
          colors: {
            background: '#101114',
            surface: '#191b20',
            'surface-variant': '#24272d',
            // surface-variant を暗い色へ上書きしても on-surface-variant は
            // 自動で追随せず既定の黒のままになる。color="surface-variant" を
            // 指定した箇所が暗い背景に黒文字（コントラスト 1.4:1）となって
            // 読めなくなるため、前景色も明示する。
            'on-surface-variant': '#e6e8ec',
            primary: '#d6ff73',
            secondary: '#a7c8ff',
            error: '#ffb4ab'
          }
        }
      }
    }
  }))
})
