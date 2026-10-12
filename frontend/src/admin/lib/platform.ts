/** The modifier key of the save shortcut as the user knows it. */
export const modKey: string = (() => {
  try {
    return /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent) ? '⌘' : 'Ctrl'
  } catch {
    return 'Ctrl'
  }
})()
