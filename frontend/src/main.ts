import { createApp } from 'vue'
import { createPinia } from 'pinia'
import '@fontsource-variable/inter/wght.css'
import '@fontsource/jetbrains-mono/latin-400.css'
import '@fontsource/jetbrains-mono/latin-500.css'
import '@fontsource/jetbrains-mono/cyrillic-400.css'
import './styles/tokens.css'
import './styles/base.css'
import './styles/ui.css'
import './styles/motion.css'
import App from './App.vue'
import { router } from './router'
import { lang } from './i18n'
import { unauthorizedEvent } from './api/client'
import { useApp } from './stores/app'
import { useMotion } from './stores/motion'

const app = createApp(App)
const pinia = createPinia()
app.use(pinia).use(router)
useMotion(pinia) // the user's own motion choice applies from the first paint, even before the settings arrive
document.documentElement.lang = lang.value

// A 401 anywhere (expired session, revoked user) sends the user back to the login page.
window.addEventListener(unauthorizedEvent, () => {
  const a = useApp()
  if (a.user) {
    a.user = null
    router.push({ name: 'login', query: { next: router.currentRoute.value.fullPath } })
  }
})

app.mount('#app')
