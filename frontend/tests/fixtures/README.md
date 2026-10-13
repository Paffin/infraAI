# Recorded engine reports

Each file is `{ "architecture": …, "report": … }` where `report` is the **real** output of the Rust engine
(`engine::evaluate::evaluate` with the shipped catalog, rules and settings) for `architecture`. The frontend
suites (`needs`, `objectives`, `capacity`, `diagram`) run on these files, so a change in the engine's JSON
shape fails here instead of in the browser.

| file | input | what it shows |
|---|---|---|
| `report-three-az` | template `three-az-region` | healthy design: score 100, capacity plan for 8 services |
| `report-no-requirements` | the same, `requirements` emptied | `no_data` findings that need RTO / RPO / RPS / budget |
| `report-overloaded` | `two-dc-witness`, peak RPS ×6 | services that are tight or overloaded, a failing worst case |
| `report-gaps` | `multi-region-pilot-light`, `lagSeconds` removed from async edges | `no_data` findings that need an edge property |
| `report-no-geo` | `two-dc-witness`, geo tolerance, `geo` removed from the zones | `no_data` findings that need a zone property |
| `report-spof` | `two-dc-witness`, `gslb` and `waf` at one replica | single points of failure |

`plan-spof` is different: `{ architecture, plan }`, where `plan` is the real output of `engine::recommend` ("what to improve first") for the architecture of `report-spof`
(recorded with `FIXTURE_PLAN=1 deploy/record-fixture.sh <architecture.json> plan-spof`). The suites `recommend` (unit) and `recommendations` / `a11y` (browser) run on it.

Re-record with `deploy/record-fixture.sh <template-id | architecture.json> <name>` (it runs
`cargo run -p engine --example report -- … en --json` and wraps the output as `{architecture, report}`). Recording is only
needed when the shape of the report changes; the inputs of the files above are the shipped templates with the changes
listed in the table.
