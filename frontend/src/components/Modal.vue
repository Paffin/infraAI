<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref } from 'vue'
import { X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { parseCssTime } from '@/lib/motion'
import { useFocusScope } from '@/lib/useFocusScope'

const props = withDefaults(defineProps<{ title?: string; size?: 'md' | 'lg' | 'xl'; closable?: boolean }>(), { size: 'md', closable: true })
const emit = defineEmits<{ close: [] }>()

/** A close the dialog starts itself (Esc, the cross, a click outside) plays the exit animation first. */
const closing = ref(false)
const backdrop = ref<HTMLElement | null>(null)
const dialog = ref<HTMLElement | null>(null)
let timer: ReturnType<typeof setTimeout> | undefined
async function close() {
  if (closing.value) return
  closing.value = true
  await nextTick()
  // how long the exit takes is whatever the stylesheet says (tokens, mode, scale): read it, do not repeat it
  const ms = backdrop.value ? parseCssTime(getComputedStyle(backdrop.value).animationDuration) : 0
  timer = setTimeout(() => {
    emit('close')
    // the parent may refuse ("discard the changes?" answered "no"): the dialog stays, and stays usable: it was faded out for nothing, and a second Esc must close it again
    closing.value = false
  }, ms)
}
onBeforeUnmount(() => clearTimeout(timer))

// The dialog takes the keyboard (focus goes to its field, or to the first control of its body, or of its footer), holds it (the page behind is inert, Tab goes
// round) and gives it back to the control that opened it. Escape belongs to the top dialog only: lib/useFocusScope.ts.
useFocusScope(backdrop, {
  onEscape: () => { if (props.closable) void close() },
  container: () => dialog.value,
  regions: ['.modal-body', '.modal-foot'],
})
</script>

<template>
  <Teleport to="body">
    <div ref="backdrop" class="modal-backdrop" :class="{ closing }" @mousedown.self="closable && close()">
      <div ref="dialog" class="modal" tabindex="-1" :class="size === 'lg' ? 'modal-lg' : size === 'xl' ? 'modal-xl' : ''" role="dialog" aria-modal="true" :aria-label="title">
        <div class="modal-head">
          <h2 class="grow truncate"><slot name="title">{{ title }}</slot></h2>
          <slot name="head" />
          <button v-if="closable" class="btn btn-ghost btn-icon" :aria-label="t('common.close')" @click="close"><X :size="18" /></button>
        </div>
        <div class="modal-body"><slot /></div>
        <div v-if="$slots.footer" class="modal-foot"><slot name="footer" /></div>
      </div>
    </div>
  </Teleport>
</template>
