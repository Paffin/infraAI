<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import * as api from '@/api/endpoints'
import BrandLogo from '@/components/BrandLogo.vue'
import LangSwitch from '@/components/LangSwitch.vue'
import MotionSwitch from '@/components/MotionSwitch.vue'
import { t } from '@/i18n'
import { passwordChangeError, signInError } from '@/lib/authErrors'
import { useApp } from '@/stores/app'

const app = useApp()
const router = useRouter()
const route = useRoute()

const email = ref('')
const password = ref('')
const current = ref('')
const next = ref('')
const busy = ref(false)
const error = ref('')

const changing = computed(() => !!app.user?.mustChangePassword)
/** Why the page was opened by itself: the session ended (`?expired=`), and whether the unsaved edits were kept in this browser (`kept`) or could not be (`lost`). */
const expired = computed(() => {
  const e = route.query.expired
  return e === 'kept' || e === 'lost' || e === '1' ? e : ''
})

function target(): string {
  const n = route.query.next
  return typeof n === 'string' && n.startsWith('/') && !n.startsWith('//') ? n : '/'
}

async function submit() {
  error.value = ''
  busy.value = true
  try {
    const u = await app.login(email.value.trim(), password.value)
    if (u.mustChangePassword) { current.value = password.value; return }
    router.replace(target())
  } catch (e) {
    error.value = signInError(e)
  } finally {
    busy.value = false
  }
}

async function change() {
  error.value = ''
  busy.value = true
  try {
    await api.auth.changePassword(current.value, next.value)
    await app.loadBootstrap()
    router.replace(target())
  } catch (e) {
    error.value = passwordChangeError(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <main class="login">
    <div class="lang"><MotionSwitch /><LangSwitch /></div>
    <section class="card login-card">
      <h1 class="brand"><BrandLogo :size="34" /><span>{{ app.appName }}</span></h1>
      <p class="sub muted">{{ t('login.subtitle') }}</p>

      <div v-if="expired && !changing" class="banner banner-warn" role="status" data-testid="login-expired">
        <b>{{ t('login.expired') }}</b>
        <template v-if="expired === 'kept'"> {{ t('login.expired.kept') }}</template>
        <template v-else-if="expired === 'lost'"> {{ t('login.expired.lost') }}</template>
      </div>

      <form v-if="!changing" class="col" style="gap: 14px" @submit.prevent="submit">
        <div class="field">
          <label for="email">{{ t('login.email') }}</label>
          <input id="email" v-model="email" class="input" type="email" autocomplete="username" required autofocus />
        </div>
        <div class="field">
          <label for="password">{{ t('login.password') }}</label>
          <input id="password" v-model="password" class="input" type="password" autocomplete="current-password" required />
        </div>
        <div v-if="error" class="banner banner-err" role="alert">{{ error }}</div>
        <button class="btn btn-primary btn-block" :disabled="busy || !email || !password">{{ t('login.submit') }}</button>
      </form>

      <form v-else class="col" style="gap: 14px" @submit.prevent="change">
        <h2>{{ t('login.change.title') }}</h2>
        <p class="muted" style="margin: 0">{{ t('login.change.hint') }}</p>
        <div class="field">
          <label for="cur">{{ t('login.change.current') }}</label>
          <input id="cur" v-model="current" class="input" type="password" autocomplete="current-password" required />
        </div>
        <div class="field">
          <label for="new">{{ t('login.change.new') }}</label>
          <input id="new" v-model="next" class="input" type="password" autocomplete="new-password" required autofocus />
        </div>
        <div v-if="error" class="banner banner-err" role="alert">{{ error }}</div>
        <button class="btn btn-primary btn-block" :disabled="busy || !current || !next">{{ t('login.change.submit') }}</button>
      </form>
    </section>
  </main>
</template>

<style scoped>
.login { min-height: 100%; display: flex; align-items: center; justify-content: center; padding: 24px; background: var(--ink); position: relative; }
.lang { position: absolute; top: 16px; right: 20px; display: flex; gap: 8px; }
.login-card { width: min(400px, 100%); padding: 28px; }
.brand { display: flex; align-items: center; gap: 12px; font-size: 20px; font-weight: 650; letter-spacing: -0.01em; }
.sub { margin: 6px 0 22px; }
.login-card > .banner { margin: -8px 0 16px; }
.login-card > .banner b { display: block; }
h2 { font-size: 17px; }
</style>
