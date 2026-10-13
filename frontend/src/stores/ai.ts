import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { ApiError } from '@/api/client'
import * as api from '@/api/endpoints'
import type { AiJob, ChatMessage, Proposal } from '@/api/types'
import { lang, has, t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { useApp } from './app'
import { useEditor } from './editor'
import { useToasts } from './toasts'

export type AiKind = 'generate' | 'patch' | 'fix' | 'chat'

export function aiErrorText(code: string | null | undefined, message: string | null | undefined): string {
  const key = 'ai.error.' + (code ?? '')
  if (code && has(key)) return t(key, { message: message ?? '' })
  return t('ai.error.default', { message: message ?? code ?? '' })
}

export const useAi = defineStore('ai', () => {
  const editor = useEditor()
  const app = useApp()
  const toasts = useToasts()

  const job = ref<AiJob | null>(null)
  const submitting = ref(false)
  const error = ref<{ code: string; message: string } | null>(null)
  const proposal = shallowRef<Proposal | null>(null)
  const proposalBusy = ref(false)
  const proposalStale = ref(false)
  const messages = ref<ChatMessage[]>([])
  const chatLoaded = ref(false)

  const busy = computed(() => submitting.value || (!!job.value && (job.value.status === 'queued' || job.value.status === 'running')))
  const enabled = computed(() => !!app.boot?.ai.enabled && app.canEdit !== undefined)
  const step = computed(() => job.value?.steps[job.value.steps.length - 1] ?? null)

  let pollTimer: ReturnType<typeof setTimeout> | undefined
  let pollGen = 0

  function reset() {
    pollGen++
    clearTimeout(pollTimer)
    job.value = null
    submitting.value = false
    error.value = null
    proposal.value = null
    proposalStale.value = false
    messages.value = []
    chatLoaded.value = false
  }

  // ----------------------------------------------------------------------------------------- jobs
  async function submit(kind: AiKind, prompt: string, findingId?: string): Promise<boolean> {
    if (busy.value || !editor.projectId) return false
    error.value = null
    submitting.value = true
    try {
      await editor.flushSave()
      const r = await api.ai.createJob(editor.projectId, { kind, prompt: prompt || undefined, findingId, lang: lang.value })
      track(r.jobId)
      return true
    } catch (e) {
      const err = e instanceof ApiError ? e : null
      error.value = { code: err?.code ?? 'error', message: errorMessage(e) }
      return false
    } finally {
      submitting.value = false
    }
  }

  function track(jobId: string) {
    const gen = ++pollGen
    clearTimeout(pollTimer)
    const tick = async () => {
      if (gen !== pollGen) return
      try {
        const j = await api.ai.job(jobId)
        if (gen !== pollGen) return
        job.value = j
        if (j.status === 'queued' || j.status === 'running') { pollTimer = setTimeout(tick, app.editorCfg.jobPollMs); return }
        await finish(j)
      } catch (e) {
        if (gen !== pollGen) return
        if (e instanceof ApiError && e.status === 0) { pollTimer = setTimeout(tick, app.editorCfg.jobPollMs * 2.5); return } // transient: back off
        error.value = { code: 'error', message: errorMessage(e) }
        job.value = null
      }
    }
    void tick()
  }

  async function finish(j: AiJob) {
    if (j.status === 'failed') {
      error.value = { code: j.errorCode ?? 'error', message: j.error ?? '' }
    } else if (j.status === 'succeeded') {
      const r = j.result ?? {}
      if (typeof r.proposalId === 'string') await openProposal(r.proposalId)
      else if (j.kind !== 'chat' && r.noChanges) toasts.info(t('proposal.nochanges'))
    }
    await loadChat()
  }

  async function cancel() {
    const j = job.value
    if (!j) return
    try { await api.ai.cancel(j.id) } catch { /* the job may have finished meanwhile */ }
  }

  function dismissError() {
    error.value = null
  }

  // ------------------------------------------------------------------------------------ proposals
  async function openProposal(id: string) {
    try {
      const p = await api.ai.proposal(id)
      proposalStale.value = false
      if (p.status !== 'pending') { proposal.value = p; editor.setPreview(null); return }
      let arch = p.architecture ?? null
      let preview = p.preview
      if (p.mode === 'ops') {
        const r = await api.engine.apply(editor.arch, p.ops ?? [], { preview: true, projectId: editor.projectId, lang: lang.value })
        arch = r.architecture
        if (r.diff && r.before && r.after) preview = { diff: r.diff, before: r.before, after: r.after, introduced: r.introduced ?? [], resolved: r.resolved ?? [] }
      }
      proposal.value = { ...p, preview }
      if (arch) editor.setPreview({ kind: 'proposal', proposalId: p.id, base: editor.arch, arch, diff: preview.diff })
    } catch (e) {
      proposalStale.value = e instanceof ApiError && (e.code === 'ops_invalid' || e.status === 400)
      toasts.error(proposalStale.value ? t('proposal.stale') : errorMessage(e))
      editor.setPreview(null)
    }
  }

  function closeProposal() {
    proposal.value = null
    editor.setPreview(null)
  }

  async function applyProposal() {
    const p = proposal.value
    if (!p || proposalBusy.value) return
    proposalBusy.value = true
    try {
      await editor.flushSave()
      // ops are rebased onto the current draft by the server; a full replacement must match the revision
      const r = await api.ai.apply(p.id, p.mode === 'replace' ? editor.rev : undefined)
      editor.setPreview(null)
      proposal.value = null
      editor.replaceArchitecture(r.architecture, { keepRev: r.rev })
      toasts.ok(t('proposal.applied', { n: r.version }))
      await loadChat()
    } catch (e) {
      if (e instanceof ApiError && (e.code === 'proposal_stale' || e.code === 'conflict' || e.code === 'proposal_closed')) {
        proposalStale.value = true
        editor.setPreview(null)
        toasts.error(t('proposal.stale'))
        await loadChat()
      } else toasts.error(errorMessage(e))
    } finally {
      proposalBusy.value = false
    }
  }

  async function rejectProposal() {
    const p = proposal.value
    if (!p || proposalBusy.value) return
    proposalBusy.value = true
    try { await api.ai.reject(p.id) } catch { /* already closed */ }
    proposalBusy.value = false
    closeProposal()
    await loadChat()
  }

  // ------------------------------------------------------------------------------------------ chat
  async function loadChat() {
    if (!editor.projectId) return
    try {
      const r = await api.ai.chat(editor.projectId)
      messages.value = r.messages
      chatLoaded.value = true
      if (r.runningJobId && !busy.value) track(r.runningJobId)
    } catch { /* chat is secondary */ }
  }

  async function clearChat() {
    if (!editor.projectId) return
    await api.ai.clearChat(editor.projectId)
    messages.value = []
  }

  /** Re-open a proposal referenced from the chat. */
  async function openFromChat(m: ChatMessage) {
    if (m.proposalId && m.proposalStatus === 'pending') await openProposal(m.proposalId)
  }

  return {
    job, submitting, error, proposal, proposalBusy, proposalStale, messages, chatLoaded, busy, enabled, step,
    reset, submit, cancel, dismissError, openProposal, closeProposal, applyProposal, rejectProposal, loadChat, clearChat, openFromChat,
  }
})
