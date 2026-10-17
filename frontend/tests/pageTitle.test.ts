import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { CONFIG_KINDS, SECTION_IDS, SECTION_SUBS } from '../src/admin/lib/configKinds'
import { setLang } from '../src/i18n'
import { pageTitle } from '../src/lib/pageTitle'

// Real dictionaries, both languages; nothing is stood in for.

const APP = 'Infra Architect'
afterEach(() => setLang('ru'))

/** Every page of the admin panel: 11 sections of one page, and the sub-pages of the catalog (6) and of the placement (3). */
function adminPages(): { section: string; sub?: string }[] {
  return SECTION_IDS.flatMap((section) => {
    if (!SECTION_SUBS[section]) return [{ section }]
    return CONFIG_KINDS.filter((k) => k.section === section).map((k) => ({ section, sub: k.sub }))
  })
}
const admin = (p: { section: string; sub?: string }, app = APP) => pageTitle({ routeName: 'admin', params: { section: p.section, sub: p.sub }, appName: app })

describe('the title of the tab', () => {
  it('names the page, then the service, in Russian', () => {
    setLang('ru')
    expect(pageTitle({ routeName: 'login', params: {}, appName: APP })).toBe('Вход · Infra Architect')
    expect(pageTitle({ routeName: 'projects', params: {}, appName: APP })).toBe('Проекты · Infra Architect')
    expect(admin({ section: 'settings' })).toBe('Администрирование · Настройки · Infra Architect')
    expect(admin({ section: 'catalog', sub: 'edge_type' })).toBe('Администрирование · Каталог · Типы связей · Infra Architect')
  })

  it('and in English', () => {
    setLang('en')
    expect(pageTitle({ routeName: 'login', params: {}, appName: APP })).toBe('Sign in · Infra Architect')
    expect(pageTitle({ routeName: 'projects', params: {}, appName: APP })).toBe('Projects · Infra Architect')
    expect(admin({ section: 'users' })).toBe('Administration · Users · Infra Architect')
    expect(admin({ section: 'placement', sub: 'stack' })).toBe('Administration · Placement · Platforms · Infra Architect')
  })

  it('an open project is named by itself, in any language', () => {
    for (const lang of ['ru', 'en']) {
      setLang(lang)
      expect(pageTitle({ routeName: 'editor', params: { id: 'p1' }, projectName: 'Shop prod', appName: APP })).toBe('Shop prod · Infra Architect')
    }
  })

  it('a project that is not there yet (loading, or the one before it) leaves the name of the service alone', () => {
    expect(pageTitle({ routeName: 'editor', params: { id: 'p1' }, projectName: null, appName: APP })).toBe(APP)
    expect(pageTitle({ routeName: 'editor', params: { id: 'p1' }, projectName: '', appName: APP })).toBe(APP)
    expect(pageTitle({ routeName: 'something-else', params: {}, appName: APP })).toBe(APP)
    expect(pageTitle({ routeName: undefined, params: {}, appName: APP })).toBe(APP)
  })

  it('uses the name of the service from the settings', () => {
    expect(pageTitle({ routeName: 'projects', params: {}, appName: 'Acme Infra' })).toBe('Проекты · Acme Infra')
    expect(pageTitle({ routeName: 'editor', params: {}, projectName: 'X', appName: 'Acme Infra' })).toBe('X · Acme Infra')
    expect(pageTitle({ routeName: 'editor', params: {}, appName: 'Acme Infra' })).toBe('Acme Infra')
  })

  it('/admin, an unknown section and a section that is not the first of its pages are what the page itself shows', () => {
    const settings = admin({ section: 'settings' })
    expect(pageTitle({ routeName: 'admin', params: {}, appName: APP })).toBe(settings)
    expect(pageTitle({ routeName: 'admin', params: { section: 'nonsense' }, appName: APP })).toBe(settings)
    // the sections of one page ignore a stray sub-page; the catalog without one opens its first page
    expect(admin({ section: 'rules', sub: 'junk' })).toBe(admin({ section: 'rules' }))
    expect(pageTitle({ routeName: 'admin', params: { section: 'catalog' }, appName: APP })).toBe(admin({ section: 'catalog', sub: 'kind' }))
    // a label of the navigation that is not a section ('admin.nav.label' exists in the dictionary) is not one either
    expect(pageTitle({ routeName: 'admin', params: { section: 'label' }, appName: APP })).toBe(settings)
  })

  it('every page of the service has a title of its own: login, projects, the 20 pages of the admin panel (and the editor, by the project)', () => {
    const pages = adminPages()
    expect(pages).toHaveLength(20)
    for (const lang of ['ru', 'en']) {
      setLang(lang)
      const titles = [
        pageTitle({ routeName: 'login', params: {}, appName: APP }),
        pageTitle({ routeName: 'projects', params: {}, appName: APP }),
        pageTitle({ routeName: 'editor', params: { id: 'a' }, projectName: 'A', appName: APP }),
        ...pages.map((p) => admin(p)),
      ]
      expect(titles).toHaveLength(23)
      expect(new Set(titles).size, `${lang}: ${titles.join(' | ')}`).toBe(titles.length)
      expect(titles.filter((t) => !t.endsWith(APP))).toEqual([])
    }
  })

  it('is written in one place: the stores do not set document.title themselves any more', () => {
    for (const f of ['src/stores/app.ts', 'src/stores/editor.ts', 'src/views/EditorView.vue', 'src/views/LoginView.vue']) {
      expect(readFileSync(join(__dirname, '..', f), 'utf8'), f).not.toMatch(/document\.title\s*=/)
    }
    // the only writer is the effect in App.vue
    expect(readFileSync(join(__dirname, '..', 'src/App.vue'), 'utf8')).toMatch(/document\.title\s*=\s*pageTitle\(/)
  })
})
