<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RefreshCw, RotateCcw } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ConfigItem } from '@/api/types'
import { has, t } from '@/i18n'
import { errorMessage, fmtDateTime } from '@/lib/format'
import { useDialogs } from '@/stores/dialogs'
import { useToasts } from '@/stores/toasts'
import { useDirtyGuard } from '../lib/dirty'
import { modKey } from '../lib/platform'
import { orderedForEditor } from '../lib/configKinds'
import { anchorOffsets, deepEqual, isPlainObject, parseJson, pretty } from '../lib/json'
import { useAdminConfig } from '../store'
import JsonEditor from '../components/JsonEditor.vue'
import StateBlock from '../components/StateBlock.vue'

const props = defineProps<{ kind: string }>()

const toasts = useToasts()
const dialogs = useDialogs()
const store = useAdminConfig()

const item = ref<ConfigItem | null>(null)
const text = ref('')
const original = ref('')
const loading = ref(false)
const error = ref<string | null>(null)
const serverError = ref<string | null>(null)
const saving = ref(false)
const editor = ref<InstanceType<typeof JsonEditor> | null>(null)

const parsed = computed(() => parseJson(text.value))
const changed = computed(() => {
  const p = parsed.value
  const o = parseJson(original.value)
  return !p.ok || !o.ok || !deepEqual(p.value, o.value)
})
const dirty = computed(() => text.value !== original.value && changed.value)
const canSave = computed(() => parsed.value.ok && changed.value && !saving.value)
useDirtyGuard(() => dirty.value)

/** "Jump to" anchors: the sections of the engine settings, or the property keys of the common properties. */
const anchors = computed(() => {
  const p = parsed.value
  if (!p.ok) return []
  const offsets = anchorOffsets(text.value, 1)
  if (isPlainObject(p.value)) return Object.keys(p.value).map((k) => ({ id: k, label: k, offset: offsets.get(k) }))
  if (Array.isArray(p.value)) {
    return p.value.map((v, i) => ({ id: String(i), label: isPlainObject(v) && typeof v.key === 'string' ? v.key : `#${i + 1}`, offset: offsets.get(String(i)) }))
  }
  return []
})

async function load() {
  loading.value = true
  try {
    const r = await api.admin.configList(props.kind)
    item.value = r.items[0] ?? null
    original.value = item.value ? pretty(orderedForEditor(props.kind, item.value.data)) : ''
    text.value = original.value
    error.value = item.value ? null : t('admin.items.empty')
    serverError.value = null
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

async function save() {
  const p = parsed.value
  if (!item.value || !p.ok || !canSave.value) return
  saving.value = true
  serverError.value = null
  try {
    await api.admin.configPut(props.kind, item.value.id, p.value)
    toasts.ok(t('admin.items.saved', { id: item.value.id }))
    await Promise.all([load(), store.load()])
  } catch (e) {
    serverError.value = errorMessage(e)
    toasts.error(t('admin.items.rejected'))
  } finally {
    saving.value = false
  }
}

async function reset() {
  if (!item.value) return
  const upd = item.value.updateAvailable
  const ok = await dialogs.confirm({
    title: upd ? t('admin.items.resetNewTitle', { id: item.value.id }) : t('admin.items.resetTitle', { id: item.value.id }),
    text: upd ? t('admin.items.resetNewText') : t('admin.items.resetText'),
    confirmLabel: upd ? t('admin.items.resetNew') : t('admin.items.reset'),
    danger: !upd,
  })
  if (!ok) return
  try {
    await api.admin.configReset(props.kind, item.value.id)
    toasts.ok(t('admin.items.resetDone', { id: item.value.id }))
    await Promise.all([load(), store.load()])
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

function discard() {
  text.value = original.value
  serverError.value = null
}

function anchorTitle(id: string): string {
  const k = `admin.engine.sec.${id}`
  return props.kind === 'engine' && has(k) ? t(k) : ''
}

const titleKey = computed(() => `admin.kind.${props.kind}.title`)
onMounted(load)
watch(() => props.kind, () => { item.value = null; load() })
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ has(titleKey) ? t(titleKey) : kind }}</h1>
        <p class="muted">{{ t(`admin.kind.${kind}.desc`) }}</p>
      </div>
      <button class="btn btn-icon" :title="t('common.refresh')" :aria-label="t('common.refresh')" :disabled="loading || dirty" @click="load"><RefreshCw :size="15" /></button>
      <button v-if="item?.hasDefault && (item.modified || item.updateAvailable)" class="btn" @click="reset"><RotateCcw :size="14" />{{ item.updateAvailable ? t('admin.items.resetNew') : t('admin.items.reset') }}</button>
    </header>

    <StateBlock :loading="loading && !item" :error="error" @retry="load">
      <template v-if="item">
        <div v-if="item.updateAvailable" class="banner banner-info"><strong>{{ t('admin.items.updateTitle') }}</strong> {{ t('admin.items.updateText') }}</div>

        <div class="card card-pad">
          <div v-if="anchors.length" class="anchors" role="navigation" :aria-label="t('admin.engine.jump')">
            <span class="label">{{ t('admin.engine.jump') }}</span>
            <button v-for="a in anchors" :key="a.id" type="button" class="chip mono" :title="anchorTitle(a.id)" :disabled="a.offset === undefined" @click="a.offset !== undefined && editor?.jumpTo(a.offset)">{{ a.label }}</button>
          </div>
          <JsonEditor ref="editor" v-model="text" :result="parsed" :error="serverError" height="clamp(320px, 62vh, 760px)" :label="t('admin.items.json')" @save="save" />
          <div class="row editor-actions">
            <span class="faint grow">{{ t('admin.items.updated', { when: fmtDateTime(item.updatedAt), by: item.updatedBy || '—' }) }}</span>
            <span class="faint kbd-hint"><span class="kbd">{{ modKey }}</span> + <span class="kbd">S</span></span>
            <button class="btn" :disabled="!dirty || saving" @click="discard">{{ t('admin.settings.discard') }}</button>
            <button class="btn btn-primary" :disabled="!canSave" @click="save"><span v-if="saving" class="spinner" />{{ t('common.save') }}</button>
          </div>
        </div>
      </template>
    </StateBlock>
  </section>
</template>
