<script setup lang="ts">
import { computed } from 'vue'
import { Boxes, Copy, Trash2, Unlink, X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { useDialogs } from '@/stores/dialogs'
import { useEditor } from '@/stores/editor'

const MAX_NAMES = 4

const editor = useEditor()
const dialogs = useDialogs()

const ids = computed(() => editor.multi)
const nodes = computed(() => ids.value.map((id) => editor.nodeById.get(id)).filter((n): n is NonNullable<typeof n> => !!n))
const names = computed(() => nodes.value.slice(0, MAX_NAMES).map((n) => n.name).join(', ') + (nodes.value.length > MAX_NAMES ? ` +${nodes.value.length - MAX_NAMES}` : ''))
const dis = computed(() => editor.readonly)
/** The group all selected nodes share (`null` when they differ or have none). */
const commonGroup = computed(() => {
  const groups = new Set(nodes.value.map((n) => n.group ?? ''))
  return groups.size === 1 ? [...groups][0] || null : null
})
const anyGrouped = computed(() => nodes.value.some((n) => n.group))
const zones = computed(() => editor.arch.zones.map((z) => ({ id: z.id, name: z.name })))

async function group() {
  const v = await dialogs.prompt({ title: t('bulk.group.title'), label: t('bulk.group.label'), value: commonGroup.value ?? '' })
  if (v !== null && v.trim()) editor.groupNodes(ids.value, v)
}

function toZone(e: Event) {
  const el = e.target as HTMLSelectElement
  const v = el.value
  el.value = '__pick'
  if (v === '__pick') return
  editor.moveNodesToZone(ids.value, v === '__none' ? null : v)
}
</script>

<template>
  <div class="bulk card" role="region" :aria-label="t('bulk.title', { n: ids.length })">
    <header>
      <span class="dot" />
      <b class="grow truncate">{{ t('bulk.title', { n: ids.length }) }}</b>
      <button class="btn btn-ghost btn-icon" :aria-label="t('bulk.clear')" :title="t('bulk.clear')" @click="editor.setMulti([])"><X :size="16" /></button>
    </header>
    <div class="body">
      <p class="names truncate" :title="nodes.map((n) => n.name).join(', ')">{{ names }}</p>
      <p class="hint">{{ t('bulk.hint') }}</p>
      <template v-if="!dis">
        <button class="act" @click="group"><Boxes :size="15" />{{ t('bulk.group') }}</button>
        <button v-if="anyGrouped" class="act" @click="editor.groupNodes(ids, '')"><Unlink :size="15" />{{ t('bulk.ungroup') }}</button>
        <label class="act sel">
          <span>{{ t('bulk.zone') }}</span>
          <select class="select input-sm" :aria-label="t('bulk.zone')" @change="toZone">
            <option value="__pick">{{ t('bulk.zone.pick') }}</option>
            <option v-for="z in zones" :key="z.id" :value="z.id">{{ z.name }}</option>
            <option value="__none">{{ t('bulk.zone.none') }}</option>
          </select>
        </label>
      </template>
    </div>
    <footer v-if="!dis">
      <button class="btn btn-sm" @click="editor.duplicateNodes(ids)"><Copy :size="14" />{{ t('bulk.duplicate') }}</button>
      <span class="spacer" />
      <button class="btn btn-sm btn-ghost del" @click="editor.removeNodes(ids)"><Trash2 :size="14" />{{ t('bulk.delete') }}</button>
    </footer>
  </div>
</template>

<style scoped>
.bulk { position: absolute; left: 18px; top: 62px; width: 308px; z-index: 55; box-shadow: var(--shadow-2); display: flex; flex-direction: column; }
header { display: flex; align-items: center; gap: 8px; padding: 10px 8px 6px 16px; font-size: 14px; }
.dot { width: 9px; height: 9px; border-radius: 50%; background: var(--accent); flex: none; box-shadow: 0 0 0 3px var(--accent-soft); }
.body { padding: 0 16px 10px; display: flex; flex-direction: column; gap: 6px; }
.names { margin: 0; font-size: 12.5px; color: var(--text-2); }
.hint { margin: 0 0 4px; font-size: 12px; color: var(--faint); }
.act { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 34px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); font-size: 13px; text-align: left; }
.act:hover { background: var(--surface-2); border-color: #b8c2d4; }
.act svg { color: var(--muted); flex: none; }
.act.sel { cursor: default; justify-content: space-between; }
.act.sel:hover { background: var(--surface); border-color: var(--line-strong); }
.act.sel span { white-space: nowrap; }
.act.sel select { width: 150px; }
footer { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-top: 1px solid var(--line); }
.del { color: var(--crit); }
@container (max-width: 1260px) { .bulk { top: 104px; } }
</style>
