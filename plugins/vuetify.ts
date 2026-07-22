import { createVuetify } from 'vuetify'

export default defineNuxtPlugin((nuxtApp) => {
  const vuetify = createVuetify({
    theme: {
      defaultTheme: 'photoCurator',
      themes: {
        photoCurator: {
          dark: true,
          colors: {
            background: '#111315',
            surface: '#1B1E21',
            primary: '#D6FF73'
          }
        }
      }
    }
  })

  nuxtApp.vueApp.use(vuetify)
})
