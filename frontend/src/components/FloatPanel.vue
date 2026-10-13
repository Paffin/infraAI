<script setup lang="ts">
// A window over the canvas: dragged by its header anywhere (the arrow keys nudge it, Home or a double click on the grip put it back), folded to its header,
// put back where it came from ("dock"), closed. Where it was left is remembered per browser (lib/useFloat.ts); what the user put there is theirs:
// the diagram does not move out of its way (`data-overlay="float"`).
import { computed, ref } from 'vue'
import { ChevronDown, ChevronUp, GripVertical, PanelRightOpen, X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { useFloat } from '@/lib/useFloat'

const props = withDefaults(defineProps<{
  title: string
  /** where its position is kept */
  storageKey: string
  width: number
  folded?: boolean
  /** the colour of the dot before the title: the state of what the window is about */
  tone?: 'ok' | 'risk' | 'crit' | ''
  dockLabel?: string
}>(), { folded: false, tone: '', dockLabel: '' })
const emit = defineEmits<{ (e: 'update:folded', v: boolean): void; (e: 'dock'): void; (e: 'close'): void }>()

const root = ref<HTMLElement | null>(null)
const fl = useFloat({ key: props.storageKey, area: () => root.value?.parentElement ?? null, el: () => root.value, margin: 12 })
const style = computed(() => ({ width: props.width + 'px', ...fl.style.value }))

/** The header is the handle (the buttons in it keep working as buttons). */
function headDown(e: PointerEvent) {
  if ((e.target as HTMLElement).closest('button, a, input')) return
  fl.onPointerDown(e)
}
</script>

<template>
  <section ref="root" class="float" :class="{ folded, dragging: fl.dragging.value, moved: fl.floating.value }" :style="style" data-overlay="float" role="dialog" :aria-label="title" data-testid="float-panel">
    <header @pointerdown="headDown" @dblclick="emit('update:folded', !folded)">
      <button class="grip" :title="t('float.move.hint')" :aria-label="t('float.move')" data-testid="float-grip" @keydown="fl.onKeyDown" @dblclick.stop="fl.reset()"><GripVertical :size="14" /></button>
      <i v-if="tone" class="dot" :class="tone" aria-hidden="true" />
      <h2 class="grow truncate" :title="title">{{ title }}</h2>
      <slot name="actions" />
      <button class="ib" :title="folded ? t('float.unfold') : t('float.fold')" :aria-label="folded ? t('float.unfold') : t('float.fold')" :aria-expanded="!folded" data-testid="float-fold" @click="emit('update:folded', !folded)">
        <component :is="folded ? ChevronDown : ChevronUp" :size="15" />
      </button>
      <button class="ib" :title="dockLabel || t('float.dock')" :aria-label="dockLabel || t('float.dock')" data-testid="float-dock" @click="emit('dock')"><PanelRightOpen :size="15" /></button>
      <button class="ib" :title="t('float.close')" :aria-label="t('float.close')" data-testid="float-close" @click="emit('close')"><X :size="15" /></button>
    </header>
    <div v-show="!folded" class="body"><slot /></div>
  </section>
</template>

<style scoped>
.float { position: absolute; top: calc(var(--bars-top, 56px) + 46px); right: 16px; z-index: 62; display: flex; flex-direction: column; max-width: calc(100% - 24px); max-height: calc(100% - var(--bars-top, 56px) - 46px - var(--bars-bottom, 110px));
  background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); box-shadow: var(--shadow-3); overflow: hidden; animation: pop-in var(--dur-base) var(--ease-spring) both; }
.float.dragging { box-shadow: var(--shadow-3), 0 0 0 2px var(--accent); user-select: none; }
header { display: flex; align-items: center; gap: 6px; padding: 5px 6px 5px 4px; cursor: grab; border-bottom: 1px solid var(--line); flex: none; touch-action: none; user-select: none; }
.dragging header { cursor: grabbing; }
.folded header { border-bottom-color: transparent; }
h2 { margin: 0; font-size: 14px; font-weight: 650; }
.grip { width: 20px; height: 28px; padding: 0; border: 0; background: transparent; color: var(--faint); border-radius: var(--radius-sm); display: inline-flex; align-items: center; justify-content: center; cursor: inherit; }
.grip:hover, .grip:focus-visible { color: var(--text-2); background: var(--surface-sunken); }
.dot { width: 9px; height: 9px; border-radius: 50%; flex: none; background: var(--ok-dot); box-shadow: 0 0 0 3px var(--ok-bg); }
.dot.risk { background: var(--risk-dot); box-shadow: 0 0 0 3px var(--risk-bg); }
.dot.crit { background: var(--crit); box-shadow: 0 0 0 3px var(--crit-bg); }
.ib { width: 28px; height: 28px; border: 0; background: transparent; color: var(--muted); border-radius: var(--radius-sm); display: inline-flex; align-items: center; justify-content: center; flex: none; transition: background-color var(--dur-fast), color var(--dur-fast); }
.ib:hover { background: var(--surface-sunken); color: var(--text); }
.body { display: flex; flex-direction: column; min-height: 0; flex: 1; overflow: hidden; }
</style>
