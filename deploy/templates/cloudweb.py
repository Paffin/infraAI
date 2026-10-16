"""One design, four clouds: a web service in three zones of a region, built from what the cloud provider runs.

The regional services (the entry, the load balancer, the queue, the object storage, the secrets, the monitoring) stand outside any zone: the model reads a
node without a zone as "not subject to the failure of a zone", which is what a regional managed service is. The services with a primary and replicas (the
database, the cache) and the application are in the zones. The copies are in a second region, so that they are outside the domain of what they protect.

What differs between the clouds is which product is what (`Spec`); the shape is the same, and so is what the engine has to say about it.
"""
from lib import Arch, backup_props, requirements, store


class Spec:
    def __init__(self, **kw):
        self.__dict__.update(kw)


def build(s):
    a = Arch(
        s.id, s.name, s.description, s.badge, s.order, s.tags,
        requirements(s.rps, 40, 300, 0, s.budget, "site", s.slo),
        s.title, s.about,
    )
    region = a.zone("region", s.region[0], "region")
    zones = {x: a.zone(f"az-{x}", f"{s.zone_label} {x.upper()}", "az", region, geo=s.region[1], costModel="cloud") for x in "abc"}
    region_dr = a.zone("region-dr", s.dr_region[0], "region")
    zone_dr = a.zone("dr-a", f"Копии · {s.zone_label} A", "az", region_dr, geo=s.dr_region[1], costModel="cloud")

    # the entry
    a.node("users", "users", "Пользователи", rpsLabel=f"{s.rps // 1000}k RPS пик")
    entry = ["users"]
    for key, group in (("dns", "gslb"), ("cdn", "cdn"), ("waf", "waf"), ("lb", "lb")):
        spec = getattr(s, key, None)
        if spec:
            kind, name, props = spec
            a.node(key, kind, name, group=group, **{"vcpu": 1, "ramGb": 2, **props})
            entry.append(key)
    a.chain(*entry)

    # the application in every zone
    k8s_kind, k8s_name, k8s_props = s.k8s
    for x, z in zones.items():
        a.node(f"app-{x}", k8s_kind, f"{k8s_name} · приложение ({x.upper()})", z, "app", replicas=18, vcpu=1, ramGb=2, hpaMin=12, hpaMax=48, clusterName="main", **k8s_props)
        a.edge(entry[-1], f"app-{x}", "traffic")

    # the database and the cache: the primary in A, replicas elsewhere
    kind, name, props = s.db
    db = dict(version="16", vcpu=8, ramGb=32, diskGb=500, failover="auto", failoverSeconds=30, connectionPooler=True, **props)
    a.node("db-a", kind, f"{name} · primary", zones["a"], "db", "primary", **store(**db))
    a.node("db-b", kind, f"{name} · реплика (синхронная)", zones["b"], "db", "replica", **store(**db))
    a.node("db-c", kind, f"{name} · реплика", zones["c"], "db", "replica", **store(**db))
    kind, name, props = s.cache
    a.node("cache-a", kind, f"{name} · primary", zones["a"], "cache", "primary", vcpu=2, ramGb=16, **props)
    a.node("cache-b", kind, f"{name} · реплика", zones["b"], "cache", "replica", vcpu=2, ramGb=16, **props)
    for x, d, c in (("a", "db-a", "cache-a"), ("b", "db-b", "cache-b"), ("c", "db-c", "cache-b")):
        a.call(f"app-{x}", d, 0.3, fanoutColdCache=0.8, tls=True)
        a.call(f"app-{x}", c, 1, tls=True)
    a.sync("db-a", "db-b")
    a.async_("db-a", "db-c", lag=1)
    a.async_("cache-a", "cache-b")

    # regional services
    kind, name, props = s.queue
    a.node("queue", kind, name, **store(vcpu=1, ramGb=2, **props))
    kind, name, props = s.files
    a.node("files", kind, name, **store(vcpu=2, ramGb=4, diskGb=4000, **props))
    kind, name, props = s.secrets
    a.node("secrets", kind, name, vcpu=0.5, ramGb=1, **props)
    kind, name, props = s.monitoring
    a.node("mon", kind, name, group="monitoring", **{"vcpu": 2, "ramGb": 8, **props})
    for x in "abc":
        a.call(f"app-{x}", "queue", 0.2, tls=True)
        a.call(f"app-{x}", "files", 0.1, tls=True)
        a.control("mon", f"app-{x}", tls=False)
    a.node("gitops", "gitops", "GitOps · Argo CD", zones["c"], "gitops", replicas=2)
    for x in "abc":
        a.control("gitops", f"app-{x}")

    # the copies: in the second region, locked, encrypted, restored on a schedule
    kind, name, props = s.backup
    a.node("backup", kind, name, zone_dr, **backup_props(restoreSeconds=1800, vcpu=1, ramGb=2, **props))
    a.backup("db-b", "backup", label="PITR + снимки")
    a.backup("files", "backup", every=3600)
    a.backup("secrets", "backup", every=3600, label=s.secrets_copy)
    return a
