import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import type { Bootstrap } from '../src/api/types'
import { aiChrome } from '../src/lib/aiChrome'
import { useApp } from '../src/stores/app'
import { useUi } from '../src/stores/ui'

describe('the AI controls of the editor', () => {
  it('a server without AI has none of them, whoever looks and whatever mode was chosen', () => {
    for (const canEdit of [true, false]) {
      for (const mode of ['ai', 'manual'] as const) {
        expect(aiChrome({ serverEnabled: false, canEdit, mode }), `canEdit ${canEdit}, ${mode}`).toEqual({ promptBar: false, modeSwitch: false, chatTab: false, on: false })
      }
    }
  })

  it('a server with AI gives an editor the bar, the switch and the chat', () => {
    expect(aiChrome({ serverEnabled: true, canEdit: true, mode: 'ai' })).toEqual({ promptBar: true, modeSwitch: true, chatTab: true, on: true })
  })

  it('manual mode, the user\'s own choice, takes nothing from the screen: the way back to AI stays, the bar still shows a running job', () => {
    expect(aiChrome({ serverEnabled: true, canEdit: true, mode: 'manual' })).toEqual({ promptBar: true, modeSwitch: true, chatTab: true, on: false })
  })

  it('a viewer has no prompt bar (nothing to ask for), but may read and ask in the chat, and keeps the switch', () => {
    expect(aiChrome({ serverEnabled: true, canEdit: false, mode: 'ai' })).toEqual({ promptBar: false, modeSwitch: true, chatTab: true, on: true })
  })
})

/** The shape the server sends, with only what these stores read filled in. */
function bootstrap(ai: Partial<Bootstrap['ai']>, role: string): Bootstrap {
  return {
    ai: { enabled: false, configured: false, maxPromptChars: 4000, plannerModel: '', fastModel: '', ...ai },
    user: { id: 'u1', email: 'a@b.c', name: 'A', role },
    app: { name: 'Infra Architect', languages: ['ru', 'en'], theme: {} },
    catalog: { kinds: [], categories: [], edgeTypes: [], zoneTypes: [], commonProps: [] },
    rules: [],
  } as unknown as Bootstrap
}

describe('the AI controls of the editor, on the stores', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    localStorage.clear()
  })

  it('follow the bootstrap of the server: without a key there is nothing, with one there is everything', () => {
    const app = useApp()
    const ui = useUi()
    app.boot = bootstrap({ enabled: false, configured: false }, 'editor')
    app.user = app.boot.user
    expect(ui.chrome).toEqual({ promptBar: false, modeSwitch: false, chatTab: false, on: false })

    // positive control: the same user on a server that has a key
    app.boot = bootstrap({ enabled: true, configured: true }, 'editor')
    expect(ui.chrome).toEqual({ promptBar: true, modeSwitch: true, chatTab: true, on: true })
    expect(ui.aiOn).toBe(true)

    // the user chooses manual mode: the switch stays, so does the bar; the features are off
    ui.aiMode = 'manual'
    expect(ui.chrome).toEqual({ promptBar: true, modeSwitch: true, chatTab: true, on: false })
    expect(ui.aiOn).toBe(false)
  })

  it('a viewer of a server with AI has no bar', () => {
    const app = useApp()
    const ui = useUi()
    app.boot = bootstrap({ enabled: true, configured: true }, 'viewer')
    app.user = app.boot.user
    expect(ui.chrome.promptBar).toBe(false)
    expect(ui.chrome.chatTab).toBe(true)
  })
})
