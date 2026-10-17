# HTTP API

Base path `/api`. JSON in, JSON out. Authentication is a session cookie (`ia_session`, HttpOnly,
SameSite=Lax) obtained from `POST /api/auth/login`. All errors share one shape:

```json
{ "error": { "code": "conflict", "message": "the project was changed by someone else", "details": { "currentRev": 7 } } }
```

State-changing requests carrying an `Origin` header must come from the same origin (or the
configured `app.publicUrl`); otherwise `403 forbidden`.

### Errors

Every failure under `/api` is `application/json` in the shape above, whatever went wrong: a request the server cannot read (broken JSON, a wrong type, a missing field, a wrong
`Content-Type`, a bad identifier in the path or the query, a body past `limits.maxBodyMb`, a method the path does not take) is answered by the same extractors and the same table as one it
refuses on the merits, never by a bare text. `code` is the stable part (the UI translates it, scripts branch on it), `message` is an English sentence for scripts and the log, `details` is optional
and carries the numbers and names the sentence needs. The interface puts the refusals it knows in the language of the person from `code` and `details` and does not show them the English `message`: a text past its
limit (`field_too_long`, `invalid_name`), too many changes (`ops_too_many`), a request that took too long (`request_timeout`), a body that is too large (`payload_too_large`), a busy server (`rate_limited` with
`details.scope`) and a setting value that was not accepted (`invalid_setting`). A code it does not know is shown by its `message`.

| Code | Status | Meaning |
|---|---|---|
| `invalid_json` | 400 | the body is not JSON (`details` none; the message says where the parser stopped) |
| `invalid_body` | 422 | the body is JSON but not what the endpoint takes: `details.field` is the path of the field (`ops[0].op`, `architecture`), or the missing field's name |
| `unsupported_media_type` | 415 | a body without `Content-Type: application/json` |
| `payload_too_large` | 413 | the body is past `limits.maxBodyMb` |
| `invalid_path` | 400 | an identifier in the path is not of the right form (a project id that is not a UUID, a version that is not a number) |
| `invalid_query` | 400 | a query parameter is not of the right form (`download=abc`) |
| `method_not_allowed` | 405 | the path exists but not for this method; the `Allow` header lists the ones it has |
| `not_found` | 404 | no such endpoint, project, version, job, proposal or item |
| `unauthorized` | 401 | no valid session, or the sign-in was wrong (one answer for a wrong password, an address with no account and a disabled account) |
| `forbidden` | 403 | the role may not do this, or the request came from another origin |
| `password_change_required` | 403 | a temporary password is still in force: only `/auth/me`, `/auth/password` and `/auth/logout` answer |
| `wrong_password`, `weak_password` | 400 | changing a password: the current one is wrong / the new one is too short or too simple |
| `locked` | 429 | repeated failures: the account is locked for this address (or for everybody, see `auth.lockoutScope`); also the same for an address that has no account, so the answer tells nothing about who has one. `Retry-After` says when to come back. Also changing a password with a wrong current one too many times |
| `rate_limited` | 429 | too many sign-in attempts from this address in a minute (`auth.loginsPerMinutePerIp`, counted in the database, so the same on every replica); `Retry-After`. Also a heavy request refused because too many are running (see Heavy requests): `details {scope: user\|replica, limit}`, `Retry-After` is `limits.heavyRetryAfterSeconds` |
| `request_timeout` | 504 | the request took longer than `limits.requestTimeoutSeconds` (60; 0 = no deadline) and was stopped, whatever it was waiting for: the database, a lock, a client sending its body slowly. Not on the routes that wait for another host (`/admin/versions/sync`, `/admin/llm/test`, `/admin/llm/models`). The setting is read on every request |
| `ai_rate_limited` | 429 | the hourly limit of AI requests of the user is spent |
| `conflict` | 409 | the project was changed by someone else (`details.currentRev`); also other revision clashes |
| `duplicate` | 409 | an item with this id already exists |
| `project_limit` | 409 | `limits.maxProjectsPerUser` is reached |
| `job_running` | 409 | the project already has a running AI job |
| `llm_disabled`, `llm_not_configured` | 409 | AI is switched off by the administrator / has no API key |
| `proposal_stale`, `proposal_closed` | 409 | the proposal's base moved on / it was already applied or rejected |
| `last_admin`, `self_lockout` | 409 | the last active administrator / your own account cannot be demoted or disabled |
| `not_deletable` | 409 | a built-in configuration item cannot be deleted (reset it instead) |
| `invalid_architecture` | 400 | the design is refused by validation; `details.issues` lists the errors (see Validation below) |
| `ops_invalid` | 400 | the operations cannot be applied, or what they make is refused; `details.issues` |
| `limits_exceeded` | 400 | the design is larger than `limits.maxNodes` / `maxEdges` / `maxZones` (`/evaluate`, `/diff`); `details` has the numbers |
| `ops_too_many` | 400 | `/ops/apply`: more than `limits.maxOpsPerRequest` operations (`details {max, actual}`) |
| `unknown_scenario_reference` | 400 | `/simulate`: the scenario names a zone, node, unit or geo label the design does not have; `details.refs` lists every one |
| `invalid_scenario` | 400 | `/simulate`: the scenario cannot be a failure at all (nothing named, a count out of range, a link from a site to itself, nested too deep); `details.reason` |
| `placement_failed` | 400 | the placement cannot be generated; `details.code` |
| `invalid_variants` | 400 | `/placement/compare`: no variants, or more than `limits.placementCompareMax` |
| `invalid_name` | 400 | a project name must be 1 to `limits.maxNameChars` (200) characters; `details {field, max, actual}` |
| `field_too_long` | 400 | a text past its bound, counted in characters: a description, a version message, the reason for an accepted risk (`limits.maxTextChars`, 20 000), a user's name (`limits.maxNameChars`), a finding's fingerprint (`limits.maxIdChars`); `details {field, max, actual}`. The bounds are settings and are read on every request |
| `invalid_cursor` | 400 | `GET /projects`: the `cursor` is not one that the list gave out |
| `invalid_ref`, `invalid_version`, `unknown_format` | 400 | a version reference that is not `draft` or a number / an export format that does not exist |
| `reason_too_short`, `unknown_finding`, `not_acceptable` | 400 | accepting a risk: the reason is too short / the finding does not exist / only critical and risk findings can be accepted |
| `bad_kind`, `empty_prompt`, `missing_finding`, `prompt_too_long` | 400 | an AI request that cannot be made: unknown kind / empty / `fix` without `findingId` / longer than `llm.maxPromptChars` (`details {field, max, actual}`; a `findingId` past `limits.maxIdChars` is `field_too_long`) |
| `invalid_setting`, `invalid_config`, `invalid_kind`, `invalid_id` | 400 | the administrator's input: a setting value (`details {key, reason, reasonCode, params}`: `reason` is the English sentence, `reasonCode` is why, in a code the interface says in its own words - `not_http`, `has_credentials`, `has_query`, `not_origin`, `not_single_line`, `line_too_long`, `address_chars`, `address_too_long`, `bad_address`, `no_host`, `empty`, `below_min`, `above_max`, `not_an_option`, `wrong_type`, `unknown_setting`, `cannot_store`, `invalid` for a refusal that has no code of its own - and `params` are the numbers and names it needs: `min`, `max`, `options`, `expected`, and `value` for the entry of a list that is wrong), a configuration item that does not parse or whose ranges allow no value (a `min` above the `max`, a default outside the range: the message names the property), the wrong kind, a malformed id |
| `invalid_email`, `invalid_role` | 400 | a user: not an email (empty, past 200 bytes, a control character) / not one of admin, editor, viewer |
| `llm_unreachable` | 503 | the model catalog of OpenRouter cannot be reached |
| `internal` | 500 | a bug or a failure of the database; the message never says more, the log does |

Roles: `viewer` (read) < `editor` (write projects, use AI, accept risks) < `admin` (settings, catalog,
users, audit). `chat` AI requests are available to every role.

Non-API endpoints: `GET /healthz` (liveness), `GET /readyz` (database + not draining),
`GET /metrics` (Prometheus; `Authorization: Bearer $METRICS_TOKEN` when the token is set, compared in constant time; `401` without it, `404` when `observability.metricsEnabled` is off). With `METRICS_PORT` set the metrics are served only by a second listener on that port
(nothing else is: any other path there is `404`), and this port answers `404` in the shape of the errors above ([DEPLOYMENT.md](DEPLOYMENT.md)). The `method` label of the request series is `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS` or `other`.

## Auth

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/auth/login` | `{email, password}` | `{user}` + cookie. `400 invalid_email` (empty, past 200 bytes, or with a control character such as NUL: nothing is counted or stored), `401 unauthorized`, `429 locked` / `rate_limited` (both with `Retry-After`) |
| POST | `/auth/logout` | – | `{ok}` |
| GET | `/auth/me` | – | `{user}` |
| POST | `/auth/password` | `{current, new}` | `{ok}` (re-issues the session). `400 wrong_password`, `weak_password`; `429 locked` after `auth.maxFailedAttempts` wrong current passwords in a row |
| GET | `/public/config` | – | `{appName, defaultLanguage, languages, theme}` (no auth) |

`user = {id, email, name, role, mustChangePassword}`. While `mustChangePassword` is true (temporary password issued by
the first start or by an administrator) every endpoint except `/auth/me`, `/auth/password` and `/auth/logout`
answers `403 password_change_required`.

**What a sign-in tells, and what it costs.** A wrong password, an address with no account and a disabled account (with the right or the wrong password) get the same `401 unauthorized` with the same body, and cost the
same work: one Argon2 check, against a hash of the same cost when there is no account. After `auth.maxFailedAttempts` failures from one address for one e-mail address the next attempt from that address is `429 locked`
(with `Retry-After`) *whether or not there is such an account*; it is refused before the password is looked at. The failures of one address lock only that address for that account; failures from many addresses add up to
`auth.maxFailedAttemptsPerAccount` and then lock the account for everybody (`auth.lockoutScope = account` makes the lock the account's from the first); a correct password is never a failure, so the sign-in that happens to be the one reaching that ceiling is given back together with the lock it closed and signs in. Every attempt from an address, successful ones too, counts towards
`auth.loginsPerMinutePerIp` (`429 rate_limited`). The counters are in PostgreSQL, so they are the same on every replica; a password reset by an administrator clears them. Behind a reverse proxy the address is read from
`X-Forwarded-For` only with `TRUST_PROXY=true`, counting `TRUST_PROXY_HOPS` entries from the right ([CONFIGURATION.md](CONFIGURATION.md)). The reason of a failure (no account, disabled, wrong password) is in the audit log
(`auth.login_failed`, `details.reason`) and in `auth.locked` when a lock was closed; the metrics carry only the outcome (`infra_auth_logins_total{outcome="ok|failed|locked|throttled"}`).

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
and a version with `props.variant` / `props.version`; an unknown variant is reported by validation as a warning (`unknown_variant`), and the node is judged as if it ran the kind's default one. See [CATALOG.md](CATALOG.md).

`GET /versions` → `{today, tables: VersionSet[]}`, `VersionSet = {id, name?, source, asOf, cycles:[{cycle, releaseDate?, support?, eol?, latest?, latestReleaseDate?, lts}]}`
(`support`/`eol` are `"YYYY-MM-DD"`, `true` or `false`). Supports `If-None-Match` → `304`. The editor judges versions from it with the same rules as the engine's `VER-01`.

## Projects

| Method | Path | Notes |
|---|---|---|
| GET | `/projects?limit=&cursor=` | `{projects:[{id,name,description,owner,updatedAt,createdAt,headVersion,draftRev,nodeCount,summary,thumb}], next}`; `summary` = `ReportSummary` of the latest version; `thumb` = what a thumbnail needs, see below; `description` is only the first `limits.listDescriptionChars` (280) characters, the whole one is in `GET /projects/{id}` (so the search of the project cards, which reads the list, covers the name and that beginning of the description, not what comes after it). Paged, see below |
| POST | `/projects` | `{name, description?, templateId? \| architecture? \| placement?, lang?}` → `{id}` (editor); `placement` is a `PlacementRequest` (see Placement below), `400 placement_failed` if it cannot be generated; `400 invalid_name` / `field_too_long` for a name / description past `limits.maxNameChars` / `limits.maxTextChars` |
| GET | `/projects/{id}` | `{project, architecture, rev, waivers}` |
| PATCH | `/projects/{id}` | `{name?, description?}` (the same bounds as on create) |
| DELETE | `/projects/{id}` | soft delete (owner or admin) |
| PUT | `/projects/{id}/draft` | `{architecture, expectedRev}` → `{rev}`; `409 conflict` with `details.currentRev` |
| GET | `/projects/{id}/versions` | `{versions:[{n,message,author,summary,createdAt}]}` |
| POST | `/projects/{id}/versions` | `{message, lang?}` → `{n}` (snapshot of the draft + report summary); `400 field_too_long` for a message past `limits.maxTextChars` |
| GET | `/projects/{id}/versions/{n}` | `{n,message,author,architecture,summary,createdAt}` |
| POST | `/projects/{id}/versions/{n}/restore` | `{expectedRev}` → `{rev, snapshot}`. The draft being replaced is saved first as the version «Before restore of vN» (`snapshot` = its number) unless it already is the newest version (`snapshot: null`): nothing that was not in a version is lost. `409 conflict` with `details.currentRev` when the revision is not `expectedRev`. Audited as `project.restore` |
| GET | `/projects/{id}/diff?from=draft\|N&to=draft\|N&ui=0` | `ArchDiff` |
| POST | `/projects/{id}/waivers` | `{fingerprint, reason, expiresInDays?, lang?}` → `{id}` (accept a risk); `400 field_too_long` for a reason past `limits.maxTextChars` or a fingerprint past `limits.maxIdChars` |
| DELETE | `/projects/{id}/waivers/{wid}` | revoke |
| GET | `/projects/{id}/export/{format}?lang=&version=&download=1` | JSON `{format, files:[{path,content}], report}`; `report` (only for `kubernetes`/`helm`/`terraform`, see [IAC.md](IAC.md)) = `{clusters, generated, skipped:[{unit,name,kind,nodes,code,message,props}], notes, prerequisites, secrets}`; with `download=1` a file or a zip |

**Paging.** The list is ordered by the time of the last change, newest first, and by id among projects changed at the same instant, so the order is total. `limit` (1 to `limits.listPageMax`, 500 by default,
which is also what a request without `limit` gets; a value outside the range is clamped, not refused) and `cursor` (the `next` of the previous page; opaque, and `400 invalid_cursor` for anything this list did not give out) walk it: `next` is `null` on the last page. A client
that sends neither gets what it always got, the first 500, plus `next`: whoever stops at the first page sees what they saw before, and the 501st project is reachable by following `next`. A cursor is a position, not a snapshot: the page after it is the next rows in the order
at the time of the request (a project changed meanwhile moves to the front, one deleted meanwhile is gone, the cursor stays valid even if the row it names is deleted). Thumbnails come with the first page only. `GET /projects/{id}/versions` is not paged (a project keeps at most `limits.maxVersionsPerProject` versions) and neither is `GET /admin/users`.

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
| POST | `/diff` | `{from, to, ui?}` → `ArchDiff` (design vs. an imported snapshot, any two architectures; canvas positions are ignored unless `ui: true`; nothing is stored). Each side is held to `limits.maxNodes` / `maxEdges` / `maxZones` (`400 limits_exceeded`) and no further: a snapshot of an environment may carry a kind the catalog does not know |
| POST | `/ops/apply` | `{architecture, ops, preview?, projectId?, lang?}` → `{architecture, cascadedEdges, renamed}`; with `preview: true` also `{diff, before, after, introduced, resolved}`. At most `limits.maxOpsPerRequest` operations (`400 ops_too_many`, `details {max, actual}`) |
| GET | `/templates/{id}` | full `Template` (`{id,name,description,badge,architecture}`) |

### Heavy requests

What costs the processors of the replica for seconds is counted while it **runs**, not by the minute: a person may have `limits.heavyConcurrentPerUser` (3) of these in flight and a replica `limits.heavyConcurrentPerCore` (4) per processor; past either
the answer is `429 rate_limited` with `Retry-After` (`limits.heavyRetryAfterSeconds`), before the body is read and before the engine starts. The count is in the memory of the replica asked (the processors are its own), and a request that the client aborted
still counts until its calculation has ended: a blocking task is not stopped by a closed connection. The editor treats the answer as "later" for what it asks for by itself after every edit - the verdict (`/evaluate`), the failure being watched (`/simulate`) and the plan of improvements (`/recommendations`): it shows no error,
keeps what is on the screen marked as coming and asks again after the `Retry-After`. For a request the person made (an export, a version, a placement, an accepted risk) it says in the person's language that the server is busy and for how long; a script should wait the `Retry-After` and send the request again.
The routes that are counted (`heavy::HEAVY_ROUTES`, the one list the gate and its test use):

| Method | Route |
|---|---|
| POST | `/evaluate`, `/recommendations`, `/simulate`, `/diff`, `/ops/apply` |
| POST | `/placement/preview`, `/placement/compare` |
| GET | `/projects/{id}/export/{format}` |
| POST | `/projects`, `/projects/{id}/versions`, `/projects/{id}/versions/{n}/restore`, `/projects/{id}/waivers`, `/proposals/{id}/apply` |

The work the AI pipeline does (its own evaluations) and the demo project at start-up are not requests of a person and are not counted here (`jobs.maxConcurrent` bounds the jobs). `limits.maxBodyMb` is read on every request as well.

`scenario` is one of
`{"type":"none"}`, `{"type":"instance","nodeId":"…","count":1}`, `{"type":"node","nodeId":"…"}`,
`{"type":"unit","unit":"<group>"}`, `{"type":"zone","zoneId":"…"}`,
`{"type":"link","a":"<zoneId>","b":"<zoneId>"}`, `{"type":"isolate","zoneIds":["…","…"]}` (sites cut off from the rest but alive),
`{"type":"sites","zoneIds":["…","…"]}`, `{"type":"geo","geo":"<label>"}`,
`{"type":"combo","items":[<scenario>, …]}` (several failures at the same time: what each item takes away is taken away together; one item equals that item, items nest).

A scenario has to name what the design has. A failure of a zone, node, cluster or disaster domain that is not there takes nothing away, and the simulator would answer "up": for a scenario that comes from a typo,
a link made on another design or a tab that still shows a node deleted since, that is a wrong answer, not a service that survived. `/simulate` refuses it with `400 unknown_scenario_reference` (the message names what is
missing, `details.refs` lists all of it: `[{kind: zone|node|unit|geo, id}]`) or, when it cannot be a failure at all, `400 invalid_scenario` with `details.reason`: `no_zones` (`isolate` / `sites` of nothing), `empty_combo`,
`too_many_items` and `too_deep` (a combination past `scenarios.maxComboItems` / `maxComboDepth` of the engine settings, 100 and 4), `link_to_itself`, `count_out_of_range` (an instance count from 1 to `limits.maxReplicas`). A `geo` label is
one the engine gives: the `geo` property of the innermost zone around a node, else the id of the nearest disaster-level zone. Every scenario the engine lists in `Report.scenarios` is accepted.

### Validation: what is refused and what is only said

One validator (`engine::model::validate`) judges every design, and it says the same things everywhere: `Issue = {level: error|warning, code, message, subject?}`. **Structure** (ids, references, kinds,
limits) is always an error. **Numbers** are held to the range the catalog gives their property (`min` / `max`, a kind that restates a property inherits what it leaves out from the common one; a property
the catalog does not describe is left alone) and to the ceilings of the engine settings (`limits` of `engine`: `maxReplicas` 100000 instances of a node, `maxNumber` 10¹² for any number without a maximum of its own, `maxRpsPeak`,
`maxGrowthPercent`), and the requirements to their domains (an availability above 0 and at most 100, times and money not negative, `rpsPeak` from 0, `growthPercent` above −100).
A number as text (`"3,5"`) is a number; `null` and an empty text are "not set".

| Code | Level | Said when |
|---|---|---|
| `bad_id` | error | an id is empty, longer than 80, or has a character other than letters, digits, `_ - . :` |
| `duplicate_id` | error | two zones, nodes or links share an id (operations rename a colliding id instead: `renamed`) |
| `dangling_edge` | error | a link names a node that does not exist |
| `dangling_parent` | error | a zone's parent does not exist |
| `dangling_zone` | error | a node's zone does not exist |
| `self_loop` | error | a link joins a node to itself |
| `zone_cycle` | error | a zone is its own ancestor |
| `unknown_kind`, `unknown_edge_type`, `unknown_zone_type` | error | the catalog has no such product, link type or zone type |
| `unknown_unit_ref` | error | a property that names another service (`dcs`) names one that is not there |
| `unknown_variant`, `unknown_role` | warning | the product has no such way to deploy / role: the default one is assumed |
| `limit_nodes`, `limit_edges`, `limit_zones` | error | more than `limits.maxNodes` / `maxEdges` / `maxZones` |
| `prop_out_of_range` | error, warning when only looked at | a number is outside the range of its property (`replicas = 10¹²`: allowed 1 to 100000); the message names the node, the property and the range |
| `prop_type` | error | a property that is a number or a flag holds something else (`"abc"`, a list, `NaN`) |
| `requirement_out_of_range` | error, warning when only looked at | a requirement is outside its domain (`availabilitySlo = 150`) |

Where each is a refusal. What is **written** is refused with `400 invalid_architecture` (`details.issues` lists them): `PUT /projects/{id}/draft`, `POST /projects` (an `architecture`, a `templateId` and a generated `placement` alike),
the result of `/ops/apply` (`400 ops_invalid`) and of an AI proposal, and the model's answer (a node it adds has the number out of range dropped, never clamped; any other number out of range goes back to it for repair, naming the node, the property and the range).
What is **looked at** is not refused for a number: `/evaluate`, `/simulate`, `/recommendations`, an export and the report of a version answer `200` and say it as a warning in `issues`, so a project stored out of range still opens, evaluates, exports and
versions instead of answering `500`, and it can be saved again once the number is put right. The engine holds the instances of a node to a whole number from 1 to the most the node may have (`limits.maxReplicas`, a lower `max` of the product); every other number it
reads as it is, so a figure computed from one that is out of range (a load from `rpsPeak = 10³⁰⁸`) may come out without a value (`null`) in the report: the warning says why. The numbers in a message are written with an exponent from 10¹⁵ up.
The base of `/ops/apply` is read the same way, but what the operations make of it is written: an operation that leaves a number out of range where it was is refused (`400 ops_invalid`, with `prop_out_of_range` in `details.issues`), and one that puts it right
is applied. A fix proposed for some other finding therefore cannot be previewed on such a design until the number is put right.
Structure that cannot be judged stays `400 invalid_architecture` on `/simulate`, `/recommendations` and `/ops/apply`; `/evaluate` returns it in `issues` (and `400 limits_exceeded` past the size limits).
`infra-architect check` ([CLI.md](CLI.md)) refuses a file the way a write does: exit code 2 and the reason.

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
| POST | `/placement/preview` | `PlacementRequest` → `{architecture, sites:[{id,name,role,geo,racks}], siteFailures, notes:[{code,level,vars}], issues, report, nodes, edges}`; `report` = `{score, coverage, counts, objectives, cost, scenarioStats, worstCapacity, findings}`; `issues` = what validation says of the design (a requirement outside its domain is a warning there and a refusal when the project is created). A load no node can carry is sized for the most a node may have (`limits.maxReplicas`) and the note `replicas_capped` says so |
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
| POST | `/admin/llm/test` | `{ping?: true}` → key info, planner/fast model checks, warnings, optional 1-token ping. The key is `ok` only when the provider's answer was read: an answer that is past `outbound.maxAnswerKb` or is not JSON gives `ok: false` and says so in `message`, an error status with no readable body gives `ok: false` with the status |
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
| POST | `/admin/versions/sync` | `{ids?:[…]}` (all tables with an `http…` source when empty) → `{results:[{id,ok,cycles,asOf,error}], ok, failed}`. Fetches only addresses under `versions.sourceBases`, and holds every redirect it is sent on to them too (at most 5; an answer that redirects elsewhere is a failure of that table, `error` names the address); an answer past `outbound.maxAnswerKb` is cut off while it is read, with or without a `Content-Length`; a bad answer never empties a table; audited as `versions.sync` |
| GET | `/admin/audit?limit=&before=&action=&actor=` | `action` is a literal prefix (`config.` lists all configuration changes), `actor` a case-insensitive substring; `before` is the id cursor |
| GET | `/admin/system` | version, db, migrations, config rev, jobs, env flags (never secrets), and `posture`: `[{id, level: "warn"\|"info", message: {ru, en}, fix: {ru, en}}]`, what the configuration says about the installation's security, judged only in production (cookie without `Secure`, `/metrics` on the application's port with no token, an active administrator with the default address, a trusted proxy and how many hops, AI on without a monthly budget, a short audit retention, no public URL). `[]` means that none of these was found, not that the installation is secure |

Config `kind` values: `catalog.kind catalog.edge_type catalog.zone_type catalog.category catalog.common_props catalog.versions
rule template iac prompt engine placement.pattern placement.stack placement.settings`. Prompt templates use `{{name}}` placeholders (double braces, exact spelling).
