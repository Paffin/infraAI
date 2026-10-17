<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ArrowUpCircle, RotateCcw } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ConfigItem } from '@/api/types'
import { has, lt, t } from '@/i18n'
import Modal from '@/components/Modal.vue'
import { useDialogs } from '@/stores/dialogs'
import { useToasts } from '@/stores/toasts'
import { errorMessage, fmtDateTime } from '@/lib/format'
import { useDirtyGuard } from '../lib/dirty'
import { modKey } from '../lib/platform'
import { itemTitle, orderedForEditor, skeleton, validItemId, withId } from '../lib/configKinds'
import { deepEqual, isPlainObject, parseJson, pretty } from '../lib/json'
import { useTabs } from '@/lib/useTabs'
import { useAdminConfig } from '../store'
import JsonEditor from './JsonEditor.vue'
import PromptForm from './PromptForm.vue'

const props = defineProps<{ kind: string; item: ConfigItem | null; items: ConfigItem[] }>()
const emit = defineEmits<{ close: []; saved: []; reset: [item: ConfigItem] }>()

const toasts = useToasts()
const dialogs = useDialogs()
const store = useAdminConfig()

const create = computed(() => !props.item)
const categories = computed(() => [...new Set(props.items.map((i) => (isPlainObject(i.data) && typeof i.data.category === 'string' ? i.data.category : '')).filter(Boolean))])
/** The stacks the existing placement patterns use: a new pattern starts with one that exists. */
const stacks = computed(() => [...new Set(props.items.map((i) => (isPlainObject(i.data) && typeof i.data.stack === 'string' ? i.data.stack : '')).filter(Boolean))])
const blank = () => pretty(skeleton(props.kind, '', { categories: categories.value, stacks: stacks.value }))
const initialText = props.item ? pretty(orderedForEditor(props.kind, props.item.data)) : blank()

const newId = ref('')
const startFrom = ref('')
const text = ref(initialText)
const tab = ref<'form' | 'json'>(props.kind === 'prompt' ? 'form' : 'json')
const serverError = ref<string | null>(null)
const saving = ref(false)
const editor = ref<InstanceType<typeof JsonEditor> | null>(null)

const parsed = computed(() => parseJson(text.value))
const idOk = computed(() => !create.value || validItemId(newId.value.trim()))
const changed = computed(() => {
  if (create.value) return true
  const p = parsed.value
  return !p.ok || !deepEqual(p.value, props.item!.data)
})
const dirty = computed(() => (create.value ? newId.value !== '' || text.value !== initialText : text.value !== initialText && changed.value))
const canSave = computed(() => parsed.value.ok && idOk.value && changed.value && !saving.value)
const parseError = computed(() => (props.item ? store.errors.find((e) => e.kind === props.kind && e.id === props.item!.id) : undefined))
const title = computed(() => (props.item ? itemTitle(props.kind, props.item.id, props.item.data, lt, has(`admin.kind.${props.kind}.title`) ? t(`admin.kind.${props.kind}.title`) : '') : null))
const canReset = computed(() => !!props.item && props.item.hasDefault && (props.item.modified || props.item.updateAvailable))

useDirtyGuard(() => dirty.value && !saving.value)
onMounted(() => {
  if (create.value) document.getElementById('new-id')?.focus()
  else editor.value?.focus()
})

function pickStart() {
  const src = props.items.find((i) => i.id === startFrom.value)
  text.value = src ? pretty(orderedForEditor(props.kind, src.data)) : blank()
  serverError.value = null
}

async function save() {
  const p = parsed.value
  if (!p.ok || !canSave.value) return
  saving.value = true
  serverError.value = null
  try {
    if (create.value) {
      const id = newId.value.trim()
      await api.admin.configCreate(props.kind, id, withId(props.kind, id, p.value))
      toasts.ok(t('admin.items.created', { id }))
    } else {
      await api.admin.configPut(props.kind, props.item!.id, p.value)
      toasts.ok(t('admin.items.saved', { id: props.item!.id }))
    }
    emit('saved')
  } catch (e) {
    serverError.value = errorMessage(e)
    toasts.error(t('admin.items.rejected'))
  } finally {
    saving.value = false
  }
}

async function requestClose() {
  if (dirty.value && !saving.value) {
    const ok = await dialogs.confirm({ title: t('admin.unsaved.title'), text: t('admin.unsaved.text'), confirmLabel: t('admin.unsaved.leave'), danger: true })
    if (!ok) return
  }
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (!e.defaultPrevented && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    save()
  }
}

function switchTab(to: 'form' | 'json') {
  tab.value = to
}
// the tab pattern (lib/useTabs.ts); only a prompt has two views
const { tablist, tab: tabAttrs, panel: panelAttrs } = useTabs({ tabs: computed(() => ['form', 'json']), selected: tab, select: (id) => switchTab(id as 'form' | 'json'), label: computed(() => t('admin.items.tabs')) })
</script>

<template>
  <Modal size="xl" :title="create ? t('admin.items.createTitle') : item!.id" :closable="!saving" @close="requestClose">
    <template #title>
      <template v-if="create">{{ t('admin.items.createTitle') }}</template>
      <template v-else><span class="mono">{{ item!.id }}</span> <span v-if="title && title.title !== item!.id" class="muted editor-sub">{{ title.title }}</span></template>
    </template>

    <div class="col" style="gap: 12px" @keydown="onKeydown">
      <div v-if="parseError" class="banner banner-warn" role="alert"><strong>{{ t('admin.items.parseFailed') }}</strong> {{ parseError.error }}</div>

      <div v-if="item?.updateAvailable" class="banner banner-info upd">
        <ArrowUpCircle :size="16" />
        <div class="grow">
          <strong>{{ t('admin.items.updateTitle') }}</strong>
          <p>{{ t('admin.items.updateText') }}</p>
        </div>
        <button class="btn btn-sm" @click="emit('reset', item!)"><RotateCcw :size="13" />{{ t('admin.items.resetNew') }}</button>
      </div>

      <div v-if="create" class="pform-grid">
        <div class="field">
          <label for="new-id">{{ t('admin.items.id') }}</label>
          <input id="new-id" v-model="newId" class="input mono" :class="{ 'is-invalid': newId && !idOk }" autocomplete="off" spellcheck="false" maxlength="80" />
          <span class="hint" :class="{ bad: newId && !idOk }">{{ t('admin.items.idHint') }}</span>
        </div>
        <div class="field">
          <label for="start-from">{{ t('admin.items.startFrom') }}</label>
          <select id="start-from" v-model="startFrom" class="select" @change="pickStart">
            <option value="">{{ t('admin.items.skeleton') }}</option>
            <option v-for="i in items" :key="i.id" :value="i.id">{{ t('admin.items.copyOf', { id: i.id }) }}</option>
          </select>
          <span class="hint">{{ t('admin.items.idInJson') }}</span>
        </div>
      </div>

      <div v-if="kind === 'prompt'" class="tabs sub-tabs" v-bind="tablist">
        <button class="tab" :class="{ active: tab === 'form' }" v-bind="tabAttrs('form')">{{ t('admin.items.tabForm') }}</button>
        <button class="tab" :class="{ active: tab === 'json' }" v-bind="tabAttrs('json')">{{ t('admin.items.tabJson') }}</button>
      </div>

      <!-- both views of a prompt are in the one panel the tabs control; any other kind has no tabs and no panel -->
      <div v-bind="kind === 'prompt' ? panelAttrs : {}">
        <PromptForm v-if="kind === 'prompt' && tab === 'form'" v-model="text" :prompt-id="item?.id || newId" />
        <JsonEditor v-show="tab === 'json' || kind !== 'prompt'" ref="editor" v-model="text" :result="parsed" height="clamp(260px, 52vh, 640px)" :label="t('admin.items.json')" @save="save" />
      </div>
      <div v-if="serverError && tab === 'form' && kind === 'prompt'" class="banner banner-err" role="alert">{{ serverError }}</div>
      <div v-else-if="serverError" class="banner banner-err" role="alert">
        <strong>{{ t('admin.items.serverSays') }}</strong> {{ serverError }}
      </div>
      <p v-if="item" class="faint meta-line">{{ t('admin.items.updated', { when: fmtDateTime(item.updatedAt), by: item.updatedBy || '—' }) }}</p>
    </div>

    <template #footer>
      <button v-if="canReset && !item?.updateAvailable" class="btn" style="margin-right: auto" :disabled="saving" @click="emit('reset', item!)"><RotateCcw :size="14" />{{ t('admin.items.reset') }}</button>
      <span class="faint kbd-hint"><span class="kbd">{{ modKey }}</span> + <span class="kbd">S</span></span>
      <button class="btn" :disabled="saving" @click="requestClose">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" :disabled="!canSave" @click="save"><span v-if="saving" class="spinner" />{{ create ? t('common.create') : t('common.save') }}</button>
    </template>
  </Modal>
</template>
