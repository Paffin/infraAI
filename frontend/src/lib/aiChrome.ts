/**
 * Which AI controls the editor shows. A server with AI off (no key, or switched off by the administrator) has nothing to drive them: a prompt bar that
 * cannot be typed into, a switch between "AI" and "manual" with nothing to switch, a chat tab that cannot answer. They cost 67 px of the canvas and some
 * attention, and each says "unavailable". So they are not drawn at all, and the user who can fix it (the administrator) gets one hint, once.
 *
 * Only the SERVER decides what exists. The user's own choice of "manual" mode does not remove anything: the way back to "AI" has to stay on the screen,
 * and the prompt bar still shows a running job (its stages, "Cancel", an error).
 */
export interface AiChromeInput {
  /** `ai.enabled` of the bootstrap: the server has a key and AI is not switched off. */
  serverEnabled: boolean
  /** The role may change the diagram (administrator, editor): the prompt bar is for them; a viewer has nothing to ask for in it. */
  canEdit: boolean
  /** The user's choice. */
  mode: 'ai' | 'manual'
}

export interface AiChrome {
  /** The prompt bar over the canvas. */
  promptBar: boolean
  /** The "AI + manual edit / manual only" switch of the header. */
  modeSwitch: boolean
  /** The AI chat tab of the right panel, and the palette commands that open it. */
  chatTab: boolean
  /** The AI features work now: the server has them and the user did not turn them off. */
  on: boolean
}

export function aiChrome(i: AiChromeInput): AiChrome {
  return {
    promptBar: i.serverEnabled && i.canEdit,
    modeSwitch: i.serverEnabled,
    chatTab: i.serverEnabled,
    on: i.serverEnabled && i.mode === 'ai',
  }
}
