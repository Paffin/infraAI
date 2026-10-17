<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { lang, t } from '@/i18n'
import { useEditor } from '@/stores/editor'
import { useUnsaved } from '@/stores/unsaved'

// What the person has to know about their edits, in a strip of its own between the prompt bar and the work area: the save keeps failing (no connection, a server
// that does not answer, a refusal), the project was changed by somebody else, or edits that never reached the server wait in this browser. It is part of the
// page's flow, not a card laid over the diagram: it cannot sit on the toolbar, and it does not compete for the places the other notices of the diagram have.

const editor = useEditor()
const unsaved = useUnsaved()
const router = useRouter()

const offer = computed(() => unsaved.offer)
const when = computed(() => (offer.value ? new Date(offer.value.backup.savedAt).toLocaleString(lang.value, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }) : ''))
const conflict = computed(() => editor.saveState === 'conflict')
/** Why the saves fail, while they do: `offline` (no answer), `server` (5xx), `rejected` (a refusal). Nothing is said while all goes well. */
const failing = computed(() => (conflict.value ? null : editor.saveFailure))

/** The edits become a project of their own, and the editor moves there. */
async function openAsNew(arch: typeof editor.arch, offered: typeof offer.value) {
  const id = await unsaved.openAsNew(arch, offered)
  if (id) await router.push({ name: 'editor', params: { id } })
}
</script>

<template>
  <div class="sbar" data-testid="save-status">
    <!-- a live region has to be in the page before its text changes, so it is always drawn -->
    <div role="status" class="sb-live" data-testid="save-live">
      <div v-if="failing === 'offline' || failing === 'server'" class="sb banner banner-warn" :data-failure="failing">
        <span class="sb-main">{{ t(failing === 'offline' ? 'editor.save.offline' : 'editor.save.server') }}</span>
        <span class="sb-actions"><button class="btn btn-sm" @click="editor.flushSave()">{{ t('editor.save.retry') }}</button></span>
      </div>
    </div>

    <div v-if="failing === 'rejected'" class="sb banner banner-err" role="alert" data-failure="rejected">
      <span class="sb-main"><b>{{ t('editor.save.error') }}:</b> {{ editor.saveError }}</span>
      <span class="sb-actions"><button class="btn btn-sm" @click="editor.flushSave()">{{ t('editor.save.retry') }}</button></span>
    </div>

    <div v-if="conflict" class="conflict sb banner banner-err" role="alert">
      <span class="sb-main"><b>{{ t('editor.conflict.title') }}.</b> {{ t('editor.conflict.text') }}</span>
      <span class="sb-actions">
        <button class="btn btn-primary btn-sm" @click="editor.reloadFromServer()">{{ t('editor.conflict.reload') }}</button>
        <button class="btn btn-sm" data-testid="conflict-new" @click="openAsNew(editor.arch, null)">{{ t('editor.conflict.copy') }}</button>
      </span>
    </div>

    <section v-if="offer" class="sb banner banner-info" :aria-label="t('unsaved.region')" data-testid="unsaved-offer">
      <div class="sb-main">
        <b>{{ t('unsaved.title', { time: when }) }}.</b> {{ t('unsaved.text') }}
        <ul v-if="offer.lines.length" class="sb-lines">
          <li v-for="(l, i) in offer.lines" :key="i"><span class="sign" aria-hidden="true">{{ l.sign }}</span> {{ l.text }}</li>
          <li v-if="offer.more" class="more">{{ t('unsaved.more', { n: offer.more }) }}</li>
        </ul>
        <p v-if="offer.moved" class="sb-moved">{{ t('unsaved.moved') }}</p>
      </div>
      <span class="sb-actions">
        <button class="btn btn-primary btn-sm" data-testid="unsaved-restore" @click="unsaved.restore()">{{ t('unsaved.restore') }}</button>
        <button v-if="offer.diff" class="btn btn-sm" data-testid="unsaved-show" :aria-pressed="unsaved.showing" @click="unsaved.toggleView()">{{ t(unsaved.showing ? 'unsaved.hide' : 'unsaved.show') }}</button>
        <button v-if="offer.moved" class="btn btn-sm" data-testid="unsaved-new" @click="openAsNew(offer.backup.architecture, offer)">{{ t('unsaved.new') }}</button>
        <button class="btn btn-sm" data-testid="unsaved-discard" @click="unsaved.discard()">{{ t('unsaved.discard') }}</button>
      </span>
    </section>
  </div>
</template>

<style scoped>
.sbar { flex: none; }
.sb { display: flex; align-items: flex-start; justify-content: space-between; gap: 6px 16px; flex-wrap: wrap; border-radius: 0; border-width: 0 0 1px; padding: 7px 20px; font-size: 13px; line-height: 1.4; }
.sb-main { flex: 1 1 360px; min-width: 0; }
.sb-actions { display: inline-flex; gap: 6px; flex-wrap: wrap; align-items: center; flex: none; }
.sb-lines { margin: 3px 0 0; padding: 0; list-style: none; font-size: 12.5px; }
.sb-lines li { overflow-wrap: anywhere; }
.sb-lines .sign { display: inline-block; width: 1.2em; font-family: var(--mono); font-weight: 700; }
.sb-lines .more { opacity: .8; }
.sb-moved { margin: 4px 0 0; font-weight: 600; }
</style>
