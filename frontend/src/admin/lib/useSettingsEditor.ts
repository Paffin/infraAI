// Shared state of the settings forms (Settings and OpenRouter pages): loads the registry, keeps one
// draft per setting, tracks changes and saves only the changed keys.

import { computed, onBeforeUnmount, reactive, ref } from 'vue'
import * as api from '@/api/endpoints'
import { ApiError } from '@/api/client'
import type { SettingDef } from '@/api/types'
import { t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useToasts } from '@/stores/toasts'
import { jsonErrorText } from './jsonMessage'
import {
  buildChanges, defaultEdit, dirtyKeys, groupSettings, parseServerSettingError, toEdit,
  type EditValue, type SettingError,
} from './settings'

export function errorText(e: SettingError): string {
  if (e.json) return jsonErrorText(e.json)
  return t(`admin.settings.err.${e.code}`, e.params)
}

export function useSettingsEditor() {
  const toasts = useToasts()
  const app = useApp()
  const defs = ref<SettingDef[]>([])
  const rev = ref(0)
  const loading = ref(false)
  const loadError = ref<string | null>(null)
  const saving = ref(false)
  const drafts = reactive<Record<string, EditValue>>({})
  const errors = reactive<Record<string, string>>({})
  /** result of the last save, announced to screen readers */
  const status = ref<{ kind: 'ok' | 'error'; text: string } | null>(null)
  let statusTimer: ReturnType<typeof setTimeout> | undefined

  const groups = computed(() => groupSettings(defs.value))
  const dirty = computed(() => dirtyKeys(defs.value, drafts))
  const byKey = computed(() => new Map(defs.value.map((d) => [d.key, d])))

  function adopt(list: SettingDef[]) {
    defs.value = list
    for (const k of Object.keys(drafts)) delete drafts[k]
    for (const k of Object.keys(errors)) delete errors[k]
    for (const d of list) drafts[d.key] = toEdit(d)
  }

  async function load() {
    loading.value = true
    try {
      const r = await api.admin.settings()
      adopt(r.settings)
      rev.value = r.rev
      loadError.value = null
    } catch (e) {
      loadError.value = errorMessage(e)
    } finally {
      loading.value = false
    }
  }

  function discard() {
    for (const d of defs.value) drafts[d.key] = toEdit(d)
    for (const k of Object.keys(errors)) delete errors[k]
    status.value = null
  }

  function resetToDefault(key: string) {
    const d = byKey.value.get(key)
    if (!d) return
    drafts[key] = defaultEdit(d)
    delete errors[key]
  }

  function setDraft(key: string, v: EditValue) {
    drafts[key] = v
    delete errors[key]
  }

  /** Save the changed keys (optionally only those in `only`). Returns true on success. */
  async function save(only?: string[]): Promise<boolean> {
    const subset = only ? defs.value.filter((d) => only.includes(d.key)) : defs.value
    const { changes, errors: local } = buildChanges(subset, drafts)
    for (const k of Object.keys(errors)) delete errors[k]
    const localKeys = Object.keys(local)
    if (localKeys.length) {
      for (const k of localKeys) errors[k] = errorText(local[k])
      status.value = { kind: 'error', text: t('admin.settings.invalid', { n: localKeys.length }) }
      return false
    }
    if (!Object.keys(changes).length) return true
    saving.value = true
    try {
      const r = await api.admin.saveSettings(changes)
      adopt(r.settings)
      rev.value = r.rev
      const n = Object.keys(changes).length
      status.value = { kind: 'ok', text: t('admin.settings.saved', { n }) }
      toasts.ok(status.value.text)
      clearTimeout(statusTimer)
      statusTimer = setTimeout(() => { if (status.value?.kind === 'ok') status.value = null }, 5000)
      // name, theme and the AI summary come from the bootstrap: refresh them in the background
      app.loadBootstrap().catch(() => undefined)
      return true
    } catch (e) {
      const msg = errorMessage(e)
      if (e instanceof ApiError && e.code === 'invalid_setting') {
        const p = parseServerSettingError(msg, defs.value.map((d) => d.key))
        if (p.key) errors[p.key] = p.message
        status.value = { kind: 'error', text: p.key ? t('admin.settings.rejected', { key: p.key }) : msg }
      } else {
        status.value = { kind: 'error', text: msg }
      }
      toasts.error(msg)
      return false
    } finally {
      saving.value = false
    }
  }

  onBeforeUnmount(() => clearTimeout(statusTimer))

  return { defs, rev, loading, loadError, saving, drafts, errors, status, groups, dirty, byKey, load, discard, resetToDefault, setDraft, save }
}
