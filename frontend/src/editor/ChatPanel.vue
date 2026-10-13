<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { Send, Trash2 } from 'lucide-vue-next'
import { t } from '@/i18n'
import { aiErrorText, useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const ai = useAi()
const app = useApp()
const editor = useEditor()
const ui = useUi()

const text = ref('')
const mode = ref<'ask' | 'edit'>('ask')
const list = ref<HTMLElement | null>(null)
const canEdit = computed(() => app.canEdit && !editor.readonly)
const enabled = computed(() => ui.aiOn)

function parseSystem(c: string): string {
  const i = c.indexOf('|')
  return i > 0 ? aiErrorText(c.slice(0, i), c.slice(i + 1)) : c
}

async function send() {
  const v = text.value.trim()
  if (!v || ai.busy || !enabled.value) return
  const kind = mode.value === 'edit' && canEdit.value ? 'patch' : 'chat'
  if (await ai.submit(kind, v)) text.value = ''
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() }
}

watch(() => [ai.messages.length, ai.busy], async () => {
  await nextTick()
  list.value?.scrollTo({ top: list.value.scrollHeight })
}, { immediate: true })
</script>

<template>
  <div class="chat">
    <div class="head">
      <h2 class="grow">{{ t('chat.title') }}</h2>
      <button v-if="ai.messages.length" class="btn btn-ghost btn-sm" :title="t('chat.clear')" @click="ai.clearChat()"><Trash2 :size="14" /></button>
    </div>
    <div ref="list" class="msgs">
      <div v-if="!ai.messages.length && !ai.busy" class="empty">{{ enabled ? t('chat.empty') : t('chat.disabled') }}</div>
      <template v-for="m in ai.messages" :key="m.id">
        <div v-if="m.role === 'system'" class="banner banner-err sys">{{ parseSystem(m.content) }}</div>
        <div v-else class="msg" :class="m.role">
          <div class="bubble">{{ m.content }}</div>
          <div v-if="m.proposalId" class="prop">
            <span class="tag" :class="m.proposalStatus === 'applied' ? 'tag-ok' : m.proposalStatus === 'pending' ? 'tag-accent' : 'tag-nodata'">{{ m.proposalStatus === 'pending' || !m.proposalStatus ? t('chat.proposal') : t('chat.proposal.' + m.proposalStatus) }}</span>
            <button v-if="m.proposalStatus === 'pending'" class="btn btn-sm" @click="ai.openFromChat(m)">{{ t('chat.proposal.open') }}</button>
          </div>
        </div>
      </template>
      <div v-if="ai.busy" class="thinking"><span class="spinner" /> {{ t('chat.thinking') }}</div>
    </div>
    <div class="composer">
      <div v-if="canEdit" class="seg" role="group">
        <button :class="{ on: mode === 'ask' }" @click="mode = 'ask'">{{ t('chat.mode.ask') }}</button>
        <button :class="{ on: mode === 'edit' }" @click="mode = 'edit'">{{ t('chat.mode.edit') }}</button>
      </div>
      <div class="inputrow">
        <textarea v-model="text" class="textarea" rows="2" :disabled="!enabled" :placeholder="t('chat.placeholder')" @keydown="onKey" />
        <button class="btn btn-primary btn-icon send" :disabled="!enabled || ai.busy || !text.trim()" :aria-label="t('chat.send')" @click="send"><Send :size="15" /></button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.chat { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.head { display: flex; align-items: center; padding: 16px 20px 8px; }
.head h2 { font-size: 16px; }
.msgs { flex: 1; overflow: auto; padding: 8px 20px 12px; display: flex; flex-direction: column; gap: 12px; background: var(--surface-2); border-top: 1px solid var(--line); }
.msg { display: flex; flex-direction: column; gap: 6px; max-width: 92%; }
.msg.user { align-self: flex-end; align-items: flex-end; }
.bubble { white-space: pre-wrap; overflow-wrap: anywhere; padding: 9px 12px; border-radius: var(--radius-lg); font-size: 13.5px; line-height: 1.5; background: var(--surface); border: 1px solid var(--line-strong); }
.msg.user .bubble { background: var(--accent); border-color: var(--accent); color: #fff; }
.prop { display: flex; align-items: center; gap: 8px; }
.sys { font-size: 12.5px; }
.thinking { display: flex; gap: 8px; align-items: center; color: var(--muted); font-size: 13px; }
.composer { border-top: 1px solid var(--line); padding: 10px 14px 14px; display: flex; flex-direction: column; gap: 8px; }
.seg { display: inline-flex; align-self: flex-start; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); overflow: hidden; }
.seg button { border: 0; background: var(--surface); padding: 0 12px; height: 26px; font-size: 12.5px; color: var(--muted); }
.seg button.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.inputrow { display: flex; gap: 8px; align-items: flex-end; }
.inputrow .textarea { resize: none; }
.send { height: 38px; width: 38px; flex: none; }
</style>
