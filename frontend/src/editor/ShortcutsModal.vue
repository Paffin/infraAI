<script setup lang="ts">
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import { chord, GROUPS, SHORTCUTS } from '@/lib/shortcuts'
import { useUi } from '@/stores/ui'

const ui = useUi()
</script>

<template>
  <Modal :title="t('shortcuts.title')" size="lg" @close="ui.modal = null">
    <div class="cols">
      <section v-for="g in GROUPS" :key="g">
        <h4>{{ t('shortcuts.group.' + g) }}</h4>
        <dl>
          <div v-for="s in SHORTCUTS.filter((x) => x.group === g)" :key="s.id" class="line">
            <dt>{{ t('kb.' + s.id) }}</dt>
            <dd>
              <template v-for="(keys, i) in s.keys" :key="i">
                <span v-if="i" class="or">{{ t('shortcuts.or') }}</span>
                <span class="chord"><kbd v-for="k in keys" :key="k" class="kbd">{{ chord([k]) }}</kbd></span>
              </template>
            </dd>
          </div>
        </dl>
      </section>
    </div>
    <p class="tip faint">{{ t('shortcuts.tip') }}</p>
  </Modal>
</template>

<style scoped>
.cols { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 36px; }
h4 { margin: 4px 0 8px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
dl { margin: 0 0 14px; display: flex; flex-direction: column; }
.line { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 7px 0; border-bottom: 1px solid var(--line); font-size: 13.5px; }
dt { color: var(--text-2); }
dd { margin: 0; display: inline-flex; align-items: center; gap: 6px; flex: none; }
.chord { display: inline-flex; gap: 4px; }
.or { color: var(--faint); font-size: 12px; }
.tip { margin: 6px 0 0; font-size: 12.5px; }
@media (max-width: 760px) { .cols { grid-template-columns: 1fr; } }
</style>
