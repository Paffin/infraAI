import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import en from '../src/i18n/en'
import enAdmin from '../src/i18n/en.admin'
import ru from '../src/i18n/ru'
import ruAdmin from '../src/i18n/ru.admin'
import enPlacement from '../src/i18n/en.placement'
import ruPlacement from '../src/i18n/ru.placement'
import enCatalog from '../src/i18n/en.catalog'
import ruCatalog from '../src/i18n/ru.catalog'
import { GROUP_ORDER } from '../src/editor/paletteCommands'
import { GROUPS, SHORTCUTS } from '../src/lib/shortcuts'

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f)
    return statSync(p).isDirectory() ? sources(p) : /\.(vue|ts)$/.test(f) && !/\.(admin|placement|catalog)\./.test(f) && !/\/i18n\/(ru|en)\.ts$/.test(p) ? [p] : []
  })
}

const RU = { ...ru, ...ruAdmin, ...ruPlacement, ...ruCatalog }
const EN = { ...en, ...enAdmin, ...enPlacement, ...enCatalog }

describe('translations', () => {
  it('have the same keys in Russian and English', () => {
    expect(Object.keys(EN).filter((k) => !(k in RU)).sort()).toEqual([])
    expect(Object.keys(RU).filter((k) => !(k in EN)).sort()).toEqual([])
  })

  it('cover every literal key used in the sources', () => {
    const used = new Set<string>()
    for (const file of sources(join(process.cwd(), 'src'))) {
      const text = readFileSync(file, 'utf8')
      // literal keys only: `t('tab.' + id)` style prefixes are checked through the dictionaries below
      for (const m of text.matchAll(/\bt\(\s*'([a-zA-Z0-9_.]+)'\s*[,)]/g)) used.add(m[1])
    }
    const missing = [...used].filter((k) => !(k in RU) || !(k in EN)).sort()
    expect(missing).toEqual([])
  })

  it('keeps placeholders consistent between languages', () => {
    const names = (s: string) => [...new Set([...s.matchAll(/\{(\w+)(?:\|[^{}]+)?\}/g)].map((m) => m[1]))].sort().join(',')
    const broken = Object.keys(RU).filter((k) => k in EN && names(RU[k]) !== names(EN[k]))
    expect(broken).toEqual([])
  })

  it('cover the code families the UI builds keys from', () => {
    const families: Record<string, string[]> = {
      'tab.': ['checks', 'requirements', 'capacity', 'cost', 'chat'],
      'cap.state.': ['ok', 'tight', 'over', 'unknown'],
      'sim.result.': ['up', 'degraded', 'down'],
      'checks.statuses.': ['critical', 'risk', 'no_data', 'ok', 'na'],
      'export.': ['json', 'terraform', 'helm', 'adr', 'runbook', 'mermaid', 'svg'],
      'layer.': ['network', 'data', 'dr', 'all'],
      'req.failureTolerance.': ['instance', 'rack', 'site', 'geo'],
      'problem.': ['down', 'rto', 'rpo', 'data_lost', 'capacity'],
      'placement.error.': ['sites_out_of_range', 'unknown_pattern', 'unknown_stack', 'bad_kind', 'unknown_kind', 'unknown_variant', 'too_many_nodes', 'unknown_tolerance', 'unknown_zone_type'],
      'placement.note.': ['witness_sites', 'site_failures_clamped', 'copies_limited', 'single_copy', 'no_authority', 'arbiters_share_locations', 'voters_share_locations', 'cluster_per_domain'],
      'node.state.': ['up', 'partial', 'down', 'stalled', 'fenced', 'promoted', 'isolated'],
      'job.': ['preparing', 'calling_model', 'repairing', 'evaluating', 'refining', 'autofix', 'previewing'],
      'ai.error.': ['llm_not_configured', 'llm_disabled', 'llm_budget', 'llm_auth', 'llm_credits', 'llm_rate_limited', 'llm_http', 'llm_timeout', 'llm_empty', 'llm_truncated', 'invalid_output', 'interrupted', 'ai_rate_limited', 'job_running', 'prompt_too_long'],
      'event.': ['promoted', 'manual_failover', 'primary_lost', 'fenced', 'split_brain', 'quorum_lost', 'quorum_ok', 'no_candidate', 'no_failover', 'writes_blocked', 'async_fallback', 'restore_from_backup', 'data_lost', 'replicated_elsewhere', 'reroute', 'scale_out', 'capacity_overload', 'soft_dependency_lost'],
      'fix.': ['set_version', 'add_backup', 'add_member', 'add_required', 'add_witness', 'group', 'make_sync', 'remove_edge', 'scale', 'set_dcs', 'set_prop', 'set_replicas', 'set_role'],
      'chat.proposal.': ['applied', 'rejected', 'stale'],
    }
    // families that come from data: the shortcut table and the palette sections
    families['kb.'] = SHORTCUTS.map((x) => x.id)
    families['shortcuts.group.'] = [...GROUPS]
    families['palette.group.'] = [...GROUP_ORDER]
    const missing: string[] = []
    for (const [prefix, codes] of Object.entries(families)) for (const c of codes) if (!(prefix + c in RU) || !(prefix + c in EN)) missing.push(prefix + c)
    expect(missing).toEqual([])
  })
})
