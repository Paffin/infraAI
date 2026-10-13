import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { ApiError } from '@/api/client'
import * as api from '@/api/endpoints'
import type { Bootstrap, Rule, User } from '@/api/types'
import { setLang, supportedLangs } from '@/i18n'
import { indexCatalog, type CatalogIndex } from '@/lib/catalog'
import { indexVersions, type VersionIndex } from '@/lib/lifecycle'
import { editorSettings } from '@/lib/uiConfig'
import { useMotion } from './motion'

interface PublicConfig { appName: string; defaultLanguage: string; languages: string[]; theme: Record<string, string>; motion?: string; motionScale?: number }

const themeKeys = new Set<string>()

/** Apply `ui.theme` (CSS variable overrides) from the admin settings. Unknown keys are ignored. */
export function applyTheme(theme: Record<string, string> | null | undefined) {
  const root = document.documentElement
  for (const k of themeKeys) root.style.removeProperty(k)
  themeKeys.clear()
  if (!theme || typeof theme !== 'object') return
  for (const [k, v] of Object.entries(theme)) {
    if (!/^--[a-z0-9-]+$/i.test(k) || typeof v !== 'string' || v.length > 200) continue
    root.style.setProperty(k, v)
    themeKeys.add(k)
  }
}

export const useApp = defineStore('app', () => {
  const user = ref<User | null>(null)
  const publicConfig = ref<PublicConfig | null>(null)
  const boot = shallowRef<Bootstrap | null>(null)
  const index = shallowRef<CatalogIndex | null>(null)
  const rules = shallowRef<Map<string, Rule>>(new Map())
  /** Release tables (support dates of every product); loaded after the bootstrap so that the first paint does not wait for them. */
  const versions = shallowRef<VersionIndex | null>(null)
  const ready = ref(false)
  const bootError = ref<string | null>(null)

  const isAdmin = computed(() => user.value?.role === 'admin')
  const canEdit = computed(() => user.value?.role === 'admin' || user.value?.role === 'editor')
  /** Timings and thresholds of the editor (`editor.*` settings). */
  const editorCfg = computed(() => editorSettings(boot.value?.editor))
  const appName = computed(() => boot.value?.app.name ?? publicConfig.value?.appName ?? 'Infra Architect')
  const languages = computed(() => {
    const l = boot.value?.app.languages ?? publicConfig.value?.languages ?? supportedLangs
    const ok = l.filter((x) => supportedLangs.includes(x))
    return ok.length ? ok : supportedLangs
  })

  function applyBootstrap(b: Bootstrap) {
    boot.value = b
    index.value = indexCatalog(b.catalog)
    rules.value = new Map(b.rules.map((r) => [r.id, r]))
    user.value = b.user
    applyTheme(b.app.theme)
    useMotion().configure(b.app.motion, b.app.motionScale)
    document.title = b.app.name
  }

  /** Public settings (login page): name, languages, theme. Never throws. */
  async function loadPublic() {
    try {
      const c = await api.auth.publicConfig()
      publicConfig.value = c
      applyTheme(c.theme)
      useMotion().configure(c.motion, c.motionScale)
      document.title = c.appName
      try { if (!localStorage.getItem('ia_lang') && supportedLangs.includes(c.defaultLanguage)) setLang(c.defaultLanguage) } catch { /* ignore */ }
    } catch { /* the login page works without it */ }
  }

  /** Resolve the session: returns true when signed in. */
  async function init(): Promise<boolean> {
    try {
      await loadBootstrap()
      ready.value = true
      return true
    } catch (e) {
      ready.value = true
      if (e instanceof ApiError && e.status === 401) {
        user.value = null
        boot.value = null
        await loadPublic()
        return false
      }
      bootError.value = e instanceof Error ? e.message : String(e)
      return false
    }
  }

  async function loadBootstrap() {
    const b = await api.bootstrap()
    applyBootstrap(b)
    bootError.value = null
    void loadVersions()
  }

  /** The editor works without them (no chips, no hints); a failure is not worth an error message. */
  async function loadVersions() {
    try {
      const v = await api.versions()
      versions.value = indexVersions(v.tables, v.today, boot.value?.engine.lifecycle)
    } catch { /* lifecycle hints are optional */ }
  }

  async function login(email: string, password: string) {
    const r = await api.auth.login(email, password)
    user.value = r.user
    if (!r.user.mustChangePassword) await loadBootstrap()
    return r.user
  }

  async function logout() {
    try { await api.auth.logout() } catch { /* already gone */ }
    user.value = null
    boot.value = null
    index.value = null
    versions.value = null
    await loadPublic()
  }

  return { user, publicConfig, boot, index, rules, versions, ready, bootError, isAdmin, canEdit, editorCfg, appName, languages, init, loadPublic, loadBootstrap, loadVersions, login, logout }
})
