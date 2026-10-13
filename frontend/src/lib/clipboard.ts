/**
 * Puts text on the clipboard. The async API works where the page is a secure context (https, localhost); an intranet tool served over plain
 * http has none, so there a hidden field is selected and copied (`execCommand`), and whatever had the focus gets it back.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (window.isSecureContext && navigator.clipboard) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    // refused (permission, an unfocused document): the field below may still be allowed
  }
  const before = document.activeElement as HTMLElement | null
  const field = document.createElement('textarea')
  field.value = text
  field.setAttribute('readonly', '')
  field.setAttribute('aria-hidden', 'true')
  field.style.cssText = 'position:fixed;top:0;left:0;width:1px;height:1px;opacity:0;pointer-events:none'
  document.body.appendChild(field)
  field.select()
  try {
    return document.execCommand('copy')
  } catch {
    return false
  } finally {
    field.remove()
    before?.focus?.()
  }
}
