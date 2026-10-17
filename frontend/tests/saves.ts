import { watch } from 'vue'

/**
 * How many requests the save path has made, read off the store itself: a request goes out each time the queue enters `saving`. Nothing is wrapped or replaced
 * (the tests used to count by putting a spy around `fetch`, which is a double of the platform all the same). `stop()` ends the counting; do it when the test is over.
 */
export function countSaves(editor: { saveState: string }): { readonly count: number; stop: () => void } {
  let count = 0
  const stop = watch(() => editor.saveState, (s) => { if (s === 'saving') count++ }, { flush: 'sync' })
  return { get count() { return count }, stop }
}
