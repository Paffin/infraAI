# HTTP API

Base path `/api`. JSON in, JSON out. Authentication is a session cookie (`ia_session`, HttpOnly,
SameSite=Lax) obtained from `POST /api/auth/login`. All errors share one shape:

```json
{ "error": { "code": "conflict", "message": "the project was changed by someone else", "details": { "currentRev": 7 } } }
```

State-changing requests carrying an `Origin` header must come from the same origin (or the
configured `app.publicUrl`); otherwise `403 forbidden`.

Roles: `viewer` (read) < `editor` (write projects, use AI, accept risks) < `admin` (settings, catalog,
users, audit). `chat` AI requests are available to every role.

Non-API endpoints: `GET /healthz` (liveness), `GET /readyz` (database + not draining),
`GET /metrics` (Prometheus; optional `Authorization: Bearer $METRICS_TOKEN`).

## Auth

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/auth/login` | `{email, password}` | `{user}` + cookie. `401 unauthorized`, `429 locked` / `rate_limited` |
| POST | `/auth/logout` | – | `{ok}` |
| GET | `/auth/me` | – | `{user}` |
| POST | `/auth/password` | `{current, new}` | `{ok}` (re-issues the session) |
| GET | `/public/config` | – | `{appName, defaultLanguage, languages, theme}` (no auth) |

`user = {id, email, name, role, mustChangePassword}`. While `mustChangePassword` is true (temporary password issued by
the first start or by an administrator) every endpoint except `/auth/me`, `/auth/password` and `/auth/logout`
answers `403 password_change_required`.

## Bootstrap

`GET /bootstrap` → everything the editor needs (supports `If-None-Match` → `304`):

```
{ user, rev, version, engineVersion,
  about: {license, repository, homepage},   // из манифеста сервера (Cargo.toml): окно «О программе» берёт лицензию и ссылки оттуда
  app: {name, defaultLanguage, languages, theme, quickPrompts},   // quickPrompts: [{label: LText, prompt: LText}] (setting ui.quickPrompts)
  catalog: Catalog,            // kinds (products; each with function, variants, versions), edgeTypes, zoneTypes, categories (with functions), commonProps
  rules: Rule[],               // texts for rendering findings
  templates: [{id, name, description, badge, order, nodeCount, requirements}],
  exportFormats: [{id, group}],   // id: json|kubernetes|helm|terraform|adr|runbook|mermaid; group: iac|docs|data
  ai: {enabled, configured, maxPromptChars, plannerModel, fastModel},
  editor: {…},                 // timings and thresholds of the web UI (settings group editor.*)
  limits: {maxNodes, maxEdges, maxZones, waiverMinReasonChars, waiverMaxDays},
  today: "YYYY-MM-DD",         // the date (UTC) the lifecycle verdicts are given for
  engine: {currency, symbol, lifecycle: {warnDays, matureDays, preferLts, staleDays},
           minCapacityDuringFailure, headroomPercent, defaultFailureTolerance, toleranceLevels, maxSiteFailures,
           availability: {enabled, presets, warnPercent, daysPerYear},
           recommendations: {enabled, show}} }
```

`LText` values (names, texts) are a string or `{ru, en, …}`; resolve with a fallback to `en`, then `ru`.

### Products, variants and versions

A catalog **kind is a product**. `kind.function` names the job it does (`rdbms`, `cache`, …; functions are declared in `categories[].functions`), `kind.variants[]` are the ways to deploy it
(`{id, name, description, default?, behavior?, roles?, addTraits?, removeTraits?, defaults?, capacity?, cost?, versions?}`), `kind.versions` is the id of its release table (`""` = none). A node picks a variant
and a version with `props.variant` / `props.version`; an unknown variant is rejected by validation (`unknown_variant`). See [CATALOG.md](CATALOG.md).

`GET /versions` → `{today, tables: VersionSet[]}`, `VersionSet = {id, name?, source, asOf, cycles:[{cycle, releaseDate?, support?, eol?, latest?, latestReleaseDate?, lts}]}`
(`support`/`eol` are `"YYYY-MM-DD"`, `true` or `false`). Supports `If-None-Match` → `304`. The editor judges versions from it with the same rules as the engine's `VER-01`.

## Projects

| Method | Path | Notes |
|---|---|---|
| GET | `/projects` | `{projects:[{id,name,description,owner,updatedAt,createdAt,headVersion,draftRev,nodeCount,summary,thumb}]}`; `summary` = `ReportSummary` of the latest version; `thumb` = what a thumbnail needs, see below |
| POST | `/projects` | `{name, description?, templateId? \| architecture? \| placement?, lang?}` → `{id}` (editor); `placement` is a `PlacementRequest` (see Placement below), `400 placement_failed` if it cannot be generated |
| GET | `/projects/{id}` | `{project, architecture, rev, waivers}` |
| PATCH | `/projects/{id}` | `{name?, description?}` |
| DELETE | `/projects/{id}` | soft delete (owner or admin) |
| PUT | `/projects/{id}/draft` | `{architecture, expectedRev}` → `{rev}`; `409 conflict` with `details.currentRev` |
| GET | `/projects/{id}/versions` | `{versions:[{n,message,author,summary,createdAt}]}` |
| POST | `/projects/{id}/versions` | `{message, lang?}` → `{n}` (snapshot of the draft + report summary) |
| GET | `/projects/{id}/versions/{n}` | `{n,message,author,architecture,summary,createdAt}` |
| POST | `/projects/{id}/versions/{n}/restore` | `{expectedRev}` → `{rev, snapshot}`. The draft being replaced is saved first as the version «Before restore of vN» (`snapshot` = its number) unless it already is the newest version (`snapshot: null`): nothing that was not in a version is lost. `409 conflict` with `details.currentRev` when the revision is not `expectedRev`. Audited as `project.restore` |
| GET | `/projects/{id}/diff?from=draft\|N&to=draft\|N&ui=0` | `ArchDiff` |
| POST | `/projects/{id}/waivers` | `{fingerprint, reason, expiresInDays?, lang?}` → `{id}` (accept a risk) |
| DELETE | `/projects/{id}/waivers/{wid}` | revoke |
| GET | `/projects/{id}/export/{format}?lang=&version=&download=1` | JSON `{format, files:[{path,content}], report}`; `report` (only for `kubernetes`/`helm`/`terraform`, see [IAC.md](IAC.md)) = `{clusters, generated, skipped:[{unit,name,kind,nodes,code,message,props}], notes, prerequisites, secrets}`; with `download=1` a file or a zip |

`thumb` is the working copy boiled down to a layout: `{zones:[{i,x,y,w,h,t,p}], nodes:[{i,x,y,k,z}], edges:[[source,target,type]]}`
(`i` id, `t` zone type, `p` parent zone, `k` kind, `z` zone). It carries no names or properties, so the list does not grow
with the document, and it is sent only for the `limits.listThumbnails` (60 by default) most recently changed projects: for the
rest `thumb` is `null` and the client loads the project (`GET /projects/{id}`) when its card comes into view. Coordinates are
`null` until the editor has laid the project out (a project made from a template or
through the API is laid out the first time somebody opens it); the web UI then lays the thumbnail out itself with the same
algorithm.

Saving the draft is *not* a version. Versions are explicit snapshots (and are created automatically
around applied AI proposals and before a restore). The history keeps the newest `limits.maxVersionsPerProject`
versions, whichever way they were made.

**A deleted project is gone.** Once `DELETE /projects/{id}` has answered, every endpoint that names the project
(`/projects/{id}/…`), its jobs (`/ai/jobs/{id}…`), its proposals (`/proposals/{id}…`) and its accepted risks answers
`404 not_found` (`project not found`, `job not found`, `proposal not found`), and so do the engine calls that take
its `projectId` (`/evaluate`, `/recommendations`, `/ops/apply` with `preview`): the project's accepted risks
cannot be read, so nothing is evaluated with them. A project id that never existed answers the same, also for the lists
(`/versions`, `/chat`) that used to answer `200` with nothing in them.

## Engine (stateless)

| Method | Path | Body → Response |
|---|---|---|
| POST | `/evaluate` | `{architecture, projectId?, lang?}` → `Report` (accepted risks of `projectId` are applied) |
| POST | `/recommendations` | `{architecture, projectId?, lang?}` → `Plan`: что улучшить в первую очередь — исправления находок, каждое проверенное на копии схемы, с измеренным эффектом ([RECOMMENDATIONS.md](RECOMMENDATIONS.md)); при выключенной настройке пустой план |
| POST | `/simulate` | `{architecture, scenario}` → `SimResult` |
| POST | `/diff` | `{from, to, ui?}` → `ArchDiff` (design vs. an imported snapshot, any two architectures; canvas positions are ignored unless `ui: true`; nothing is stored) |
| POST | `/ops/apply` | `{architecture, ops, preview?, projectId?, lang?}` → `{architecture, cascadedEdges, renamed}`; with `preview: true` also `{diff, before, after, introduced, resolved}` |
| GET | `/templates/{id}` | full `Template` (`{id,name,description,badge,architecture}`) |

`scenario` is one of
`{"type":"none"}`, `{"type":"instance","nodeId":"…","count":1}`, `{"type":"node","nodeId":"…"}`,
`{"type":"unit","unit":"<group>"}`, `{"type":"zone","zoneId":"…"}`,
`{"type":"link","a":"<zoneId>","b":"<zoneId>"}`, `{"type":"isolate","zoneIds":["…","…"]}` (sites cut off from the rest but alive),
`{"type":"sites","zoneIds":["…","…"]}`, `{"type":"geo","geo":"<label>"}`,
`{"type":"combo","items":[<scenario>, …]}` (several failures at the same time: what each item takes away is taken away together; one item equals that item, items nest).

### Report

```
{ engineVersion, issues: Issue[],
  counts: {critical, risk, noData, ok, na, accepted},          // per rule
  rules: [{ruleId, category, status, findings, accepted}],
  findings: [{ id, ruleId, category, status, subjects:{nodes,edges,zones}, vars, fixes:[{id,kind,vars,ops}],
               needs?:[{target: requirements|node|edge|zone, id?, prop}],
               scenario?, accepted?:{reason,by,at} }],         // id = stable fingerprint (waivers)
  objectives: [{id: rto|rpo|site_loss|sites_loss|spof|capacity|availability, status, required, actual, assumed}],
  scenarios: [ScenarioSummary], baselineLoads: [UnitLoad],
  capacity: [CapacityRow],                                     // empty without a declared peak load
  score: {value, penalty, coverage, breakdown:[{ruleId,status,penalty}]},
  cost: {currency, symbol, total, nodes:[{nodeId,amount,missing,model}], byCategory, byZone, byKind, incomplete, budget},
  availability?: Availability,                                // absent when the estimate is off or there is nothing to serve
  units: {<groupKey>: <display name>} }
```

`status` ∈ `critical | risk | no_data | ok | na`. Render a finding's text with the rule's `message`
(or `noDataMessage` / `requirementMessage` when `status = no_data`; `requirementMessage` when the
fingerprint ends with `:req`) and `finding.vars`; fix button labels come from `rule.fixLabels[fix.kind]`
rendered with `{...finding.vars, ...fix.vars}`. A fix is a list of patch operations (`ops`).

`needs` (only on `no_data` findings) names the values that would turn the question into an answer: `target`
`requirements` + `prop` = a field of the requirements (`rtoSeconds`, `rpoSeconds`, `rpsPeak`, `budgetMonthly`…), or
`node` / `edge` / `zone` + `id` + `prop` = a property of that element (`rpsPerReplica`, `lagSeconds`, `geo`…). The UI
turns each into a button that opens exactly that field; every `prop` of an element is a property the catalog defines for
it (a test of the frontend checks this against the shipped catalog).

`availability` is the expected availability ([AVAILABILITY.md](AVAILABILITY.md)): `{estimate (0…1), serviceDown, downtimeSecondsPerYear, target|null (0…1),
budgetSecondsPerYear|null, causes:[{cause: instance|unit|provider|rack|site|link|isolation|geo, secondsPerYear, share}], units:[{unit, name, secondsPerYear, share}],
top:[{scenario, cause, failed, perYear, downtimeSeconds, secondsPerYear, basis: repair|recovery|interruption|capacity}], scenarios, truncated, basis}`;
`basis` is the engine settings `reliability` the number rests on (rates, repair times, provider SLAs). The objective `availability` has `required` = the target and `actual` = the estimate (0…1).
A requirement `availabilitySlo` is a percentage (`99.95`); a value up to 1 is read as a fraction.

`capacity[]` is the capacity plan of each logical service (unit): `{unit, name, kind, nodes, instances,
perReplicaRps, loadRps, capacityRps|null, targetRps, instancesNeeded|null, worst:{scenario, failed, loadRps,
capacityRps}|null}`. `capacityRps = null` means there is no capacity model (set vCPU or RPS per instance);
`targetRps` is the capacity that keeps the configured headroom at peak plus growth; `worst` is the failure that leaves
the service the least capacity relative to its load. Load and the capacity it needs scale linearly with `rpsPeak`,
so a client can answer "what if traffic doubles" by multiplying (the editor's *Capacity* tab does).

### SimResult

```
{ scenario, state: up|degraded|down, recovery:{kind: none|auto|manual|never, seconds},
  rpoSeconds|null, rpoAssumed, rpoItems, dataLost:[unit], splitBrain:[unit], capacityRatio|null, bottleneck,
  unitLoads, downUnits:[name], events:[{code,unit?,node?,vars,at?}], failed:[names],
  nodeStates:{<nodeId>: up|partial|down|stalled|fenced|promoted|isolated},
  edgeStates:{<edgeId>: ok|cut|dead},
  timeline?: [{at, state, capacityRatio, nodes:{<nodeId>: state}, edges:{<edgeId>: state}, traffic?: Traffic}],
  traffic?: Traffic, trafficBefore?: Traffic }

Traffic = { flows:{<edgeId>: share}, lost?:{<edgeId>: share}, missed?:{<edgeId>: share}, detours?:[{from:<nodeId>, to:<nodeId>, flow, lost?}], served: 0..1 }
```

`interruptions: [{unit, kind: failover|manual_failover|restore|reroute|scale_out, seconds, share}]` – what interrupts the service while the recovery runs and what share of the requests it touches (the input of the availability estimate).

`events[].code` values: `promoted manual_failover primary_lost fenced split_brain quorum_lost quorum_ok
no_candidate no_failover writes_blocked async_fallback restore_from_backup data_lost replicated_elsewhere
reroute scale_out capacity_overload soft_dependency_lost`.

`events[].at` (seconds after the failure) and `timeline: [{at, state, capacityRatio, nodes, edges}]` tell how the picture changes while the recovery runs (only when some step takes time; the first
frame holds every node and edge, the next ones only what changed, the last equals the settled `nodeStates` / `edgeStates`). See [SIMULATION.md](SIMULATION.md).

`traffic` / `trafficBefore` / `timeline[].traffic` (only from `POST /simulate`) say where the load goes: normal operation, once everything has settled, and in every frame (a whole picture, not a difference). Every figure is relative to the
load entering an entry point (1 = all of it; multiply by `requirements.rpsPeak` for requests per second). `flows` — what each dependency link carries; `lost` — sent into a link and dropped there, the requests behind it fail; `missed` — the same
for an optional dependency (a cache), the requests survive it; `detours` — routes no drawn link leads along (a member whose links are gone sends to whoever serves); `served` — the share of the requests that complete. Links that carry nothing are absent.
The links add up to the unit loads of `unitLoads` (the same load, laid on the links).

### Patch operations

```
{"op":"add_zone","zone":Zone}            {"op":"update_zone","id":"…","set":{"name":"…","props.geo":"…"}}   {"op":"remove_zone","id":"…","cascade":false}
{"op":"add_node","node":Node}            {"op":"update_node","id":"…","set":{"zoneId":"…","props.replicas":3}}  {"op":"remove_node","id":"…"}
{"op":"add_edge","edge":Edge}            {"op":"update_edge","id":"…","set":{"type":"sync","props.latencyMs":1.2}}  {"op":"remove_edge","id":"…"}
{"op":"set_requirements","set":{"rpsPeak":5000}}
```

`set` keys: `name kind zoneId group role ui props.<key>` (nodes), `name type parentId role props.<key>`
(zones), `type label source target props.<key>` (edges). `null` clears. Operations are atomic: either all
apply and the result validates, or nothing changes (`400 ops_invalid`).

## Placement

The wizard that builds a complete design for any number of sites ([PLACEMENT.md](PLACEMENT.md)). Nothing is stored; a project is created with `POST /projects {placement}`.

| Method | Path | Body → Response |
|---|---|---|
| GET | `/placement/options` | `{patterns, defaultPattern, stacks:[{id,name,layers:[{id,name,kind,alternatives,function,scope,optional,default,dcs,products:[{kind,variants:[{id,default,managed}]}]}]}], siteTypes, maxSiteFailures, maxNodes}` |
| POST | `/placement/preview` | `PlacementRequest` → `{architecture, sites:[{id,name,role,geo,racks}], siteFailures, notes:[{code,level,vars}], report, nodes, edges}`; `report` = `{score, coverage, counts, objectives, cost, scenarioStats, worstCapacity, findings}` |
| POST | `/placement/compare` | `{base, variants:[…]}` → `{variants:[{ok:true, pattern, sites, siteFailures, nodes, zones, workingSites, arbiterSites, notes, report} \| {ok:false, pattern, sites, error:{code,message}}]}`; `400 invalid_variants` beyond `limits.placementCompareMax` |

`PlacementRequest = {pattern, sites?, regions?, racks?, siteFailures?, stack?, layers?, kinds?, variants?, requirements?, siteType?, lang?, name?, maxNodes?}`: `kinds` maps a layer id to a product of the layer's function,
`variants` maps a layer id to a variant of that product. Errors: `400 placement_failed` with `details.code` (`unknown_pattern`, `sites_out_of_range`, `unknown_stack`, `unknown_kind`, `bad_kind`, `unknown_variant`, `too_many_nodes`, …).

## AI

Requires an OpenRouter key (`llm.apiKey` or `OPENROUTER_API_KEY`) and `llm.enabled`.

| Method | Path | Notes |
|---|---|---|
| POST | `/projects/{id}/ai/jobs` | `{kind: generate\|patch\|fix\|chat, prompt?, findingId?, lang?}` → `{jobId}`. `409 llm_not_configured/llm_disabled/job_running`, `429 ai_rate_limited`, `400 prompt_too_long` |
| GET | `/ai/jobs/{id}` | `{id,kind,status: queued\|running\|succeeded\|failed\|cancelled, steps:[{code,detail,at}], result, error, errorCode}` – poll every ~1 s |
| POST | `/ai/jobs/{id}/cancel` | |
| GET | `/projects/{id}/chat?after=<messageId>` | `{messages:[{id,role,content,proposalId,proposalStatus,createdAt}], runningJobId}`; a `system` message is `"<errorCode>|<text>"` |
| DELETE | `/projects/{id}/chat` | clear |
| GET | `/proposals/{id}` | `{id,status,mode: ops\|replace, ops?, architecture?, rationale, preview:{diff,before,after,introduced,resolved}, baseRev}` |
| POST | `/proposals/{id}/apply` | `{expectedRev?}` → `{rev, architecture, version}`; `409 proposal_stale/conflict/proposal_closed`. With `expectedRev` (the revision the user reviewed) a draft that moved before or while the apply ran is `409 conflict` with `details.currentRev`; without it the operations are applied to the draft that is there. The evaluation holds no transaction, so many applies at once never exhaust the connection pool |
| POST | `/proposals/{id}/reject` | |

Step codes: `preparing calling_model model_answer(debug) repairing evaluating refining autofix previewing`.
Result: `{proposalId, rationale, assumptions[], autoFixes[]}` for `generate/patch/fix`; `{answer}` for `chat`;
`{noChanges:true}` if the model proposed nothing. Error codes: `llm_not_configured llm_disabled llm_budget
llm_auth llm_credits llm_rate_limited llm_http llm_timeout llm_empty llm_truncated invalid_output interrupted`.

The model never changes the project by itself: a proposal is a preview (diff, before/after score,
introduced/resolved findings) that the user applies explicitly. Applying snapshots the previous state as a
version first.

## Admin (role `admin`)

| Method | Path | Notes |
|---|---|---|
| GET/PUT | `/admin/settings` | GET → `{settings:[SettingDef & {value, modified}\|{secretSet, secretSource}], rev}`; PUT `{changes:{key:value}}`. Secrets are write-only; an empty secret clears it. A number equal to its default is stored as "not modified". An invalid value → `400 invalid_setting` with `details:{key, reason}` (first failing key) |
| GET | `/admin/llm/models?q=&structured=1&refresh=1&limit=` | cached OpenRouter catalog `{models:[{id,name,contextLength,promptPerMillion,completionPerMillion,structuredOutputs,responseFormat,supportedParameters}], total}`; `503 llm_unreachable` when OpenRouter is down |
| POST | `/admin/llm/test` | `{ping?: true}` → key info, planner/fast model checks, warnings, optional 1-token ping |
| GET | `/admin/llm/usage?days=` | spend per day / model / user, month spend vs. budget |
| GET | `/admin/config` | per-kind counters + items that failed to parse |
| GET | `/admin/config/{kind}` | `{items:[{id,data,origin,modified,updateAvailable,hasDefault,updatedAt,updatedBy}]}` (jsonb: object keys come back sorted by length, not in the original order) |
| POST | `/admin/config/{kind}` | `{id, data}` create a custom item. `data.id` must equal `id`. Not available for the single-document kinds `catalog.common_props` (id `common`), `engine` (id `engine`) and `placement.settings` (id `placement`): `400 invalid_kind` |
| PUT | `/admin/config/{kind}/{id}` | `{data}` (validated against the engine types; `data.id == id`). The audit event lists the changed top-level keys |
| DELETE | `/admin/config/{kind}/{id}` | custom items only: `404` for an unknown id, `409 not_deletable` for a built-in one |
| POST | `/admin/config/{kind}/{id}/reset` | back to the built-in default (also dismisses `updateAvailable`) |
| GET | `/admin/config-export` · POST `/admin/config-import` | `{settings, items, dryRun?}` backup/migration of everything changed; secrets are never exported. An import is all or nothing: items, settings and the configuration revision are written in one transaction, so a value that is refused (`400 invalid_setting`, `details.key`) leaves the instance as it was. Every item is held to what creating it by hand would be: a well-formed id (`invalid_id` rules) and no second document in a single-document kind; a problem is reported as `ok: false` with `problems` and nothing is written. A new item goes after the existing ones of its kind |
| GET/POST | `/admin/users` | create returns `temporaryPassword` if none was given (the user must change it on the first sign-in) |
| PATCH | `/admin/users/{id}` | `{name?, role?, disabled?}`. The last active admin is protected (`409 last_admin`) and nobody can demote or disable their own account (`409 self_lockout`). `lockedUntil` is `null` once the lock expired |
| POST | `/admin/users/{id}/reset-password` | `{password?}` (the user must change it again) |
| GET | `/admin/versions/status` | `{today, tables, staleDays, stale:[id], oldestDays, autoSyncDays, sourceBases, last:{at, by: auto\|manual, ok, failed:[{id,error}]}\|null}` – how fresh the release tables are |
| POST | `/admin/versions/sync` | `{ids?:[…]}` (all tables with an `http…` source when empty) → `{results:[{id,ok,cycles,asOf,error}], ok, failed}`. Fetches only addresses under `versions.sourceBases`; a bad answer never empties a table; audited as `versions.sync` |
| GET | `/admin/audit?limit=&before=&action=&actor=` | `action` is a literal prefix (`config.` lists all configuration changes), `actor` a case-insensitive substring; `before` is the id cursor |
| GET | `/admin/system` | version, db, migrations, config rev, jobs, env flags (never secrets) |

Config `kind` values: `catalog.kind catalog.edge_type catalog.zone_type catalog.category catalog.common_props catalog.versions
rule template iac prompt engine placement.pattern placement.stack placement.settings`. Prompt templates use `{{name}}` placeholders (double braces, exact spelling).
