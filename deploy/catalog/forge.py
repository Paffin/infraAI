"""Shortcuts for writing the large catalog: one call per product, the archetype decides props, behaviour and traits.

A module `kinds_<name>.py` of this folder exports

    KINDS      the list of kinds it adds (built with the functions below)
    FUNCTIONS  the functions it introduces, per category: {"edge": [("ddos", "Защита от DDoS", "DDoS protection", "shield-alert"), ...]}
               (id, name ru, name en, icon from frontend/src/lib/icons.ts); a function that already exists is not declared again
    IAC        optional {function id: reason class} for the honest "no verified template" text of the function (see iac.py)

and build.py puts them after the kinds that were written by hand. `python3 deploy/catalog/lint.py kinds_<name>` checks one module without
writing anything. The rules the engine reads off a kind are in README.md ("Что движок читает у вида").
"""
from arch import *  # noqa: F401,F403 - k, pv, pr, res, db_props, cluster_props, variant, managed, rb, R_PR, R_VOTER, votes_prop, p_enc, p_lb, VERS …

# ---- colours: one family per category, a few functions stand out ---------------------------------------------------------------------
CAT_COLOR = {"edge": "#2d6cdf", "compute": "#2a64e0", "data": "#12756c", "platform": "#7a42d7", "security": "#b42318", "apps": "#b45309", "ai": "#a21caf"}
FN_COLOR = {
    "dns-server": "#3a58c4", "gslb": "#3a58c4", "cdn": "#2d6cdf", "waf": "#9d2235", "firewall": "#9d2235", "ddos": "#9d2235", "l7": "#bf1340", "l4": "#bf1340",
    "network": "#4c596c", "vpn": "#4c596c", "proxy": "#2d6cdf", "network-services": "#4c596c",
    "k8s": "#2a64e0", "vm": "#4c596c", "orchestrator": "#2a64e0", "functions": "#7a42d7", "api-gateway": "#3a58c4", "data-processing": "#7a42d7", "workflow": "#7a42d7",
    "rdbms": "#0e766e", "newsql": "#0e766e", "mq": "#0e766e", "object-store": "#4c596c", "sds": "#4c596c", "storage-array": "#4c596c", "backup": "#4c596c",
    "monitoring": "#7a42d7", "logging": "#7a42d7", "tracing": "#7a42d7", "telemetry": "#7a42d7", "ci": "#6d3fc4", "registry": "#6d3fc4",
    "identity": "#4c596c", "secrets": "#c2410c", "pki": "#4c596c", "policy": "#4c596c", "mesh": "#2a64e0", "cni": "#2a64e0", "iac": "#6d3fc4",
    "siem": "#b42318", "edr": "#9d2235", "ids-ips": "#b42318",
}


def color_for(category, function):
    return FN_COLOR.get(function) or CAT_COLOR[category]


# ---- trait presets: what the engine's rules do with them is in README.md --------------------------------------------------------------
T = {
    "workload": ["workload"],                                   # an application tier: >= 2 instances, spread over the sites
    "app": ["workload", "app"],
    "gateway": ["gateway", "edge"],
    "lb": ["balancer", "edge"],                                 # >= 2 instances, health checks
    "ingress": ["balancer", "ingress", "edge"],
    "proxy": ["proxy", "edge"],
    "waf": ["waf", "edge"],                                     # the path from the users to the application must pass it
    "firewall": ["firewall", "edge"],
    "netdev": ["network-device", "edge"],
    "vpn": ["vpn", "edge"],
    "cdn": ["cdn", "edge"],
    "dns": ["dns", "edge"],
    "platform": ["platform"],
    "monitoring": ["monitoring", "platform"],                   # satisfies "there is monitoring" (HA-OBS-01)
    "telemetry": ["telemetry", "platform"],
    "logging": ["logging", "platform"],
    "secrets": ["secrets", "stateful", "backup-required", "platform"],
    "security": ["security", "platform"],
    "sql": ["datastore", "stateful", "backup-required", "sql"],
    "doc": ["datastore", "stateful", "backup-required", "nosql", "document"],
    "nosql": ["datastore", "stateful", "backup-required", "nosql"],
    "kv": ["datastore", "stateful", "backup-required", "nosql", "kv"],
    "cache": ["datastore", "stateful", "cache", "soft-dependency"],     # losing it degrades the service, it does not stop it
    "mq": ["datastore", "stateful", "mq"],
    "search": ["datastore", "stateful", "backup-required", "search"],
    "olap": ["datastore", "stateful", "backup-required", "analytics"],
    "storage": ["datastore", "stateful", "backup-required", "storage"],
    "objstore": ["datastore", "stateful", "object-store", "backup-required"],
    "backup": ["backup-target"],
}
BACKUP_PROPS = [p for p in json.load(open(f"{HERE}/base.json", encoding="utf-8"))["kinds"] if p["id"] == "backup_storage"][0]["props"]


def backup_props(extra=()):
    """The properties DR-BKP-03..05 read off a backup target (immutability, retention, restore test) plus the sizing."""
    import copy
    return copy.deepcopy([p for p in BACKUP_PROPS if p["key"] not in ("bucket", "endpoint")]) + list(extra)


# ---- archetypes -----------------------------------------------------------------------------------------------------------------------
def svc(id, cat, fn, abbr, ru, en, desc, *, table=None, traits=("platform",), beh="stateless", replicas=2, rep=("Экземпляры", "Instances"), vcpu=2, ram=4, disk=None,
        cap=None, variants=None, extra=(), sub="{replicas}× · {version}", color=None, roles=None, runbook=None, icon=None, versions=None, version=True):
    """A stateless service: any live instance serves. `desc` is (ru, en). `table` is the release table of the version (None: no lifecycle
    data, the version is a free field). Give `cap` (requests per second one instance sustains) to anything that sits on the request path."""
    props = ([pv(table)] if version else []) + [pr(replicas, *rep)] + list(extra) + res(vcpu, ram, disk)
    if versions is None and table and table != id:
        versions = table
    return k(id, cat, fn, abbr, color or color_for(cat, fn), ru, en, beh, list(traits), props, desc, sub=sub if version else "{replicas}×", cap=cap,
             variants=variants, versions=versions, runbook=runbook, roles=roles, icon=icon)


def app(id, cat, fn, abbr, ru, en, desc, *, cap, traits=("workload", "app"), **kw):
    """An application tier (a server, a portal, a business application): the rules want two or more instances on two sites."""
    return svc(id, cat, fn, abbr, ru, en, desc, traits=traits, cap=cap, **kw)


def lbsvc(id, cat, fn, abbr, ru, en, desc, *, cap, table=None, traits=("balancer", "edge"), replicas=2, vcpu=2, ram=4, **kw):
    """A balancer or a proxy on the path of the requests: instances behind a shared address, health checks."""
    return svc(id, cat, fn, abbr, ru, en, desc, table=table, traits=traits, cap=cap, replicas=replicas, vcpu=vcpu, ram=ram, extra=p_lb(), **kw)


def appliance(id, cat, fn, abbr, ru, en, desc, *, cap, traits, table=None, replicas=2, rep=("Узлы кластера", "Cluster nodes"), vcpu=4, ram=8, **kw):
    """A network or security appliance (firewall, router, hardware balancer): an active/standby pair or a stack, one is a single point of failure."""
    return svc(id, cat, fn, abbr, ru, en, desc, table=table, traits=traits, cap=cap, replicas=replicas, rep=rep, vcpu=vcpu, ram=ram, extra=p_lb(), **kw)


def saas(id, cat, fn, abbr, ru, en, desc, *, traits=("managed",), cap=None, table=None, extra=(), sub=None, color=None, variants=None, icon=None, versions=None, runbook=None):
    """A service that exists only as somebody's cloud or SaaS: the provider runs it (the trait `managed`: the engine reads its availability
    off the provider's SLA, not off instances). No instances to size, so the trait `provider-ha` too: the rules that ask for two instances
    (HA-APP-01, HA-LB-01) and for no single point of failure (HA-SPOF-01) leave it alone; the SLA is what its availability stands on.
    Give `cap` when it sits on the request path of the design."""
    props = ([pv(table)] if table else []) + list(extra)
    sized = any(p.get("key") == "replicas" for p in props)  # a kind that does ask for an instance count is checked like any other
    traits = list(dict.fromkeys(list(traits) + ["managed"] + ([] if sized else ["provider-ha"])))
    if versions is None and table and table != id:
        versions = table
    return k(id, cat, fn, abbr, color or color_for(cat, fn), ru, en, "stateless", traits, props, desc, sub=sub, cap=cap, variants=variants, versions=versions, icon=icon, runbook=runbook)


def sqldb(id, fn, abbr, ru, en, desc, *, table=None, cap=4000, vcpu=8, ram=32, disk=500, dcs=False, failover="auto", secs=30, votes=True, pooler=None, enc=True,
          traits=None, variants=None, runbook=None, extra=(), roles=None, versions=None, cat="data", sub="{version} · {vcpu} vCPU", color=None, icon=None):
    """A database with one writer and replicas (primary_replica): the writer is promoted on failure. `dcs=True` when a consensus store decides it."""
    if versions is None and table and table != id:
        versions = table
    return k(id, cat, fn, abbr, color or color_for(cat, fn), ru, en, "primary_replica", list(traits or T["sql"]),
             db_props(table, vcpu, ram, disk, failover=failover, secs=secs, dcs=dcs, votes=votes, pooler=pooler, enc=enc, extra=extra), desc, sub=sub, cap=cap,
             roles=roles or R_PR(), variants=variants, versions=versions, runbook=runbook, icon=icon)


def qcluster(id, fn, abbr, ru, en, desc, *, table=None, traits, cap, replicas=3, rf=3, min_insync=2, vcpu=8, ram=32, disk=500, rf_ru="Replication factor", rf_en="Replication factor",
             mi_ru="Реплик подтверждают запись", mi_en="Replicas acknowledging a write", rep_ru="Узлы", rep_en="Nodes", enc=True, variants=None, runbook=None, extra=(), versions=None,
             cat="data", sub="{replicas}× · RF{replicationFactor}", color=None, icon=None):
    """A cluster that needs a majority (or minInsync) of its instances alive on one side of a partition (quorum_log): Cassandra, Kafka, Raft-based stores."""
    if versions is None and table and table != id:
        versions = table
    return k(id, cat, fn, abbr, color or color_for(cat, fn), ru, en, "quorum_log", list(traits),
             cluster_props(table, replicas, rf, min_insync, vcpu, ram, disk, rf_ru=rf_ru, rf_en=rf_en, mi_ru=mi_ru, mi_en=mi_en, rep_ru=rep_ru, rep_en=rep_en, enc=enc, extra=extra),
             desc, sub=sub, cap=cap, variants=variants, versions=versions, runbook=runbook, icon=icon)


def voters(id, fn, abbr, ru, en, desc, *, table=None, traits, cat="platform", vcpu=2, ram=4, disk=50, variants=None, versions=None, color=None, icon=None, sub="{replicas}× · {version}", secs=5):
    """A consensus group (consensus): a majority of voting members decides; three or five of them, on three sites to survive the loss of one."""
    props = [pv(table), votes_prop(), num("electionSeconds", "Время выбора лидера", "Leader election time", unit_="s", min=0, default=secs, group="failover")] + res(vcpu, ram, disk)
    if versions is None and table and table != id:
        versions = table
    return k(id, cat, fn, abbr, color or color_for(cat, fn), ru, en, "consensus", list(traits), props, desc, sub=sub, roles=R_VOTER, variants=variants, versions=versions, icon=icon)


def vself(id, ru, en, d_ru, d_en, default=False, **kw):
    """A variant for a way of running the product on your own servers."""
    return variant(id, ru, en, d_ru, d_en, default=default, **kw)


def vcloud(id, ru, en, d_ru=None, d_en=None, **kw):
    """A variant for the same product run by a provider (adds the trait `managed`)."""
    return managed(id, ru, en, desc_ru=d_ru, desc_en=d_en, **kw)
