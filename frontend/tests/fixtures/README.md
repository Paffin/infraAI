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

## Recorded simulations

`sim-*.json` is `{ architecture, scenario, result }`, where `result` is the **real** output of `engine::sim::simulate_timeline` (the story of a failure: the frames, the
events, and where the traffic goes in normal operation (`trafficBefore`), at every moment and once settled). The suites `playback` and `traffic` (unit) and the browser specs run on the same shapes.

| file | input | what it shows |
|---|---|---|
| `sim-two-dc-zone` | `two-dc-witness`, the data centre `dc1` fails | a stateful failover with several frames: requests sent into the dead data centre are lost, the replica is promoted, the traffic flows to `dc2` |
| `sim-three-az-app` | `three-az-region`, the node `app-a` fails | the load balancer keeps sending to the dead member until the health check notices, then the traffic goes on by *detours* (routes no drawn link leads along) |
| `sim-three-az-link` | `three-az-region`, the link between `az-a` and `az-b` is cut | a partition: the cut links lose their traffic, the nodes behind them are reached by detours |
| `sim-two-dc-sites` | `two-dc-witness`, both data centres (`dc1`, `dc2`) fail | the service is down, a person restores it from the backup (7200 s), the Kafka clusters lose their data: the tiles of the failure panel (`simpanel`) |
| `sim-two-dc-combo` | `two-dc-witness`, `dc1` fails **and** the node `pg-replica` with it (`combo`) | two failures at once, each survived alone and fatal together: the label, the written account and the link of a combined scenario |

Record one with `deploy/record-sim.sh <template-id | architecture.json> '<scenario json>' <name>` (it runs `cargo run -p engine --example simulate`).
