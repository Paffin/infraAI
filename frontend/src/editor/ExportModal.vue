<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Copy, Download } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ExportFile } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { lang, t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useToasts } from '@/stores/toasts'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const toasts = useToasts()

const formats = computed(() => app.boot?.exportFormats ?? [])
const format = ref(formats.value.includes('terraform') ? 'terraform' : formats.value[0] ?? 'json')
const files = ref<ExportFile[]>([])
const current = ref(0)
const loading = ref(false)
const error = ref('')
const copied = ref(false)

async function load() {
  loading.value = true
  error.value = ''
  try {
    await editor.flushSave()
    const r = await api.projects.export(editor.projectId, format.value, lang.value)
    files.value = r.files
    current.value = 0
  } catch (e) {
    files.value = []
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

async function copy() {
  const f = files.value[current.value]
  if (!f) return
  try {
    await navigator.clipboard.writeText(f.content)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    toasts.error(t('error.generic', { message: 'clipboard' }))
  }
}

const href = computed(() => api.projects.exportUrl(editor.projectId, format.value, lang.value))
const content = computed(() => files.value[current.value]?.content ?? '')
watch(format, load, { immediate: true })
</script>

<template>
  <Modal :title="t('export.title')" size="xl" @close="ui.modal = null">
    <div class="exp">
      <nav class="formats">
        <button v-for="f in formats" :key="f" :class="{ on: format === f }" @click="format = f">{{ t('export.' + f) }}</button>
        <p v-if="format === 'terraform' || format === 'helm'" class="muted note">{{ t('export.scaffold') }}</p>
      </nav>
      <section class="files">
        <div v-if="loading" class="empty"><span class="spinner" /></div>
        <div v-else-if="error" class="banner banner-err">{{ error }}</div>
        <template v-else>
          <div class="bar row">
            <div class="tabs2 grow">
              <button v-for="(f, i) in files" :key="f.path" :class="{ on: i === current }" @click="current = i">{{ f.path }}</button>
            </div>
            <button class="btn btn-sm" @click="copy"><Copy :size="13" />{{ copied ? t('export.copied') : t('export.copy') }}</button>
            <a class="btn btn-sm btn-primary" :href="href" download><Download :size="13" />{{ t('export.download') }}</a>
          </div>
          <pre class="code mono">{{ content }}</pre>
        </template>
      </section>
    </div>
  </Modal>
</template>

<style scoped>
.exp { display: grid; grid-template-columns: 250px minmax(0, 1fr); gap: 20px; min-height: 420px; }
.formats { display: flex; flex-direction: column; gap: 6px; }
.formats button { text-align: left; padding: 9px 12px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); font-size: 13.5px; }
.formats button:hover { background: var(--surface-2); }
.formats button.on { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.note { font-size: 12px; margin: 8px 0 0; }
.files { min-width: 0; display: flex; flex-direction: column; gap: 10px; }
.tabs2 { display: flex; gap: 4px; overflow-x: auto; }
.tabs2 button { border: 1px solid transparent; background: transparent; padding: 4px 10px; border-radius: 6px; font-family: var(--mono); font-size: 12px; white-space: nowrap; color: var(--muted); }
.tabs2 button.on { background: var(--surface-sunken); color: var(--text); border-color: var(--line); }
.code { margin: 0; padding: 14px 16px; background: var(--ink); color: var(--on-dark); border-radius: var(--radius); font-size: 12px; line-height: 1.55; overflow: auto; max-height: 56vh; white-space: pre; }
@media (max-width: 900px) { .exp { grid-template-columns: 1fr; } }
</style>
