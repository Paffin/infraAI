<script setup lang="ts">
import { computed, ref } from 'vue'
import { PanelLeftClose, Search } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { KindDef, TemplateMeta, Architecture } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { lt, t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'
import { useToasts } from '@/stores/toasts'

const app = useApp()
const editor = useEditor()
const toasts = useToasts()
const ui = useUi()

const q = ref('')
const input = ref<HTMLInputElement | null>(null)
const pending = ref<{ meta: TemplateMeta; arch: Architecture } | null>(null)

const query = computed(() => q.value.trim().toLowerCase())
const matches = (...parts: (string | null | undefined)[]) => !query.value || parts.some((p) => p && p.toLowerCase().includes(query.value))

const templates = computed(() => [...(app.boot?.templates ?? [])].sort((a, b) => a.order - b.order).filter((x) => matches(lt(x.name), lt(x.description), x.id)))
const categories = computed(() => {
  const cat = app.boot?.catalog
  if (!cat) return []
  return [...cat.categories].sort((a, b) => a.order - b.order).map((c) => ({
    id: c.id, name: lt(c.name),
    kinds: cat.kinds.filter((k) => !k.hidden && k.category === c.id && matches(lt(k.name), k.abbr, k.id, lt(k.description))),
  })).filter((c) => c.kinds.length)
})
const zoneTypes = computed(() => (app.boot?.catalog.zoneTypes ?? []).filter((z) => matches(lt(z.name), z.id)))

function dragKind(e: DragEvent, type: 'kind' | 'zone', id: string) {
  e.dataTransfer?.setData('application/x-infra-item', JSON.stringify({ type, id }))
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'
}

function addKind(k: KindDef) {
  if (!editor.readonly) editor.addNode(k.id)
}

async function pickTemplate(meta: TemplateMeta) {
  if (editor.readonly) return
  try {
    const tpl = await api.engine.template(meta.id)
    if (editor.isEmpty) editor.addTemplate(tpl.architecture, 'replace')
    else pending.value = { meta, arch: tpl.architecture }
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

function applyTemplate(mode: 'replace' | 'add') {
  if (pending.value) editor.addTemplate(pending.value.arch, mode)
  pending.value = null
}

defineExpose({ focus: () => { ui.libraryOpen = true; setTimeout(() => input.value?.focus(), 30) } })
</script>

<template>
  <aside v-show="ui.libraryOpen" class="library">
    <div class="search">
      <Search :size="15" />
      <input ref="input" v-model="q" class="input" :placeholder="t('library.search') + ' ( / )'" @keydown.esc="q = ''; input?.blur()" />
      <button class="collapse" :title="t('library.collapse')" :aria-label="t('library.collapse')" @click="ui.libraryOpen = false"><PanelLeftClose :size="16" /></button>
    </div>

    <div class="scroll">
      <section v-if="templates.length">
        <h4>{{ t('library.templates') }}</h4>
        <button v-for="tpl in templates" :key="tpl.id" class="tpl" :disabled="editor.readonly" :title="lt(tpl.description)" @click="pickTemplate(tpl)">
          <span class="grow truncate">{{ lt(tpl.name) }}</span>
          <span v-if="tpl.badge" class="badge">{{ tpl.badge }} ✓</span>
        </button>
      </section>

      <section v-for="c in categories" :key="c.id">
        <h4>{{ c.name }}</h4>
        <div class="grid">
          <button
            v-for="k in c.kinds" :key="k.id" class="kind" :disabled="editor.readonly" draggable="true" :title="lt(k.description) || t('library.hint')"
            @dragstart="dragKind($event, 'kind', k.id)" @click="addKind(k)"
          >
            <span class="badge-abbr" :style="{ background: k.color }">{{ k.abbr }}</span>
            <span class="truncate">{{ lt(k.name) }}</span>
          </button>
        </div>
      </section>

      <section v-if="zoneTypes.length">
        <h4>{{ t('library.zones') }}</h4>
        <div class="grid">
          <button v-for="z in zoneTypes" :key="z.id" class="kind" :disabled="editor.readonly" draggable="true" :title="t('library.hint')" @dragstart="dragKind($event, 'zone', z.id)" @click="editor.addZone(z.id)">
            <span class="zsw" :style="{ borderColor: z.color }" />
            <span class="truncate">{{ lt(z.name) }}</span>
          </button>
        </div>
      </section>

      <div v-if="!templates.length && !categories.length && !zoneTypes.length" class="empty">{{ t('library.nothing') }}</div>
    </div>

    <Modal v-if="pending" :title="t('library.template.apply.title', { name: lt(pending.meta.name) })" @close="pending = null">
      <p style="margin: 0">{{ t('library.template.apply.text') }}</p>
      <template #footer>
        <button class="btn" @click="pending = null">{{ t('common.cancel') }}</button>
        <button class="btn" @click="applyTemplate('add')">{{ t('library.template.apply.add') }}</button>
        <button class="btn btn-primary" @click="applyTemplate('replace')">{{ t('library.template.apply.replace') }}</button>
      </template>
    </Modal>
  </aside>
</template>

<style scoped>
.library { width: var(--sidebar-w); flex: none; background: var(--surface); border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
.search { position: relative; padding: 14px 14px 10px; display: flex; align-items: center; gap: 6px; }
.collapse { flex: none; width: 30px; height: 30px; border: 0; background: transparent; color: var(--muted); border-radius: var(--radius-sm); display: inline-flex; align-items: center; justify-content: center; }
.collapse:hover { background: var(--surface-sunken); color: var(--text); }
.search > svg:first-child { position: absolute; left: 25px; top: 24px; color: var(--faint); }
.search .input { padding-left: 34px; height: 36px; background: var(--surface-2); flex: 1; min-width: 0; }
.scroll { flex: 1; overflow: auto; padding: 0 14px 18px; }
section { margin-top: 14px; }
h4 { font-size: 11.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; margin: 0 0 8px 2px; }
.tpl { width: 100%; display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 12px; margin-bottom: 8px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 13.5px; text-align: left; }
.tpl:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.tpl:disabled { opacity: .55; cursor: not-allowed; }
.badge { font-family: var(--mono); font-size: 11px; color: var(--ok); letter-spacing: .02em; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
.kind { display: flex; align-items: center; gap: 7px; height: 40px; padding: 0 7px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 12.5px; text-align: left; min-width: 0; cursor: grab; }
.kind:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.kind:disabled { opacity: .55; cursor: not-allowed; }
.kind .badge-abbr { flex: none; min-width: 26px; height: 24px; font-size: 9px; }
.zsw { width: 26px; height: 18px; border: 2px solid; border-radius: 5px; flex: none; }
</style>
