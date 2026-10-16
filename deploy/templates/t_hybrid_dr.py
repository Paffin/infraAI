from lib import Arch, backup_props, requirements, store

# One data center of your own, a standby in the public cloud: the data flows to the cloud all the time, the compute there is a pilot light (two copies of the application that grow
# to the whole fleet when the data center is lost). A person decides to switch (the database is promoted by hand), so the recovery is counted in hours, not seconds, and costs a fraction
# of a second data center.
a = Arch(
    "hybrid-dr", ("Гибрид: свой ЦОД и резерв в облаке", "Hybrid: your own data center and a standby in the cloud"),
    ("Основной ЦОД и «пилотная лампа» в облаке: данные реплицируются постоянно, приложение в облаке растёт до полной мощности при потере ЦОД. Восстановление — часы, цена — доля второго ЦОД.",
     "A main data center and a pilot light in the cloud: the data is replicated all the time, the application there grows to full size when the data center is lost. Recovery takes hours, the price is a fraction of a second data center."),
    "DR", 19, ["dr", "hybrid", "on-prem", "cloud", "pilot-light", "cost"],
    requirements(2500, 30, 14400, 60, 1800000, "site", 99.9),
    "Гибрид: свой ЦОД и резерв в облаке",
    "Основной ЦОД держит всю нагрузку; облако — «пилотная лампа»: данные реплицируются в него постоянно (база асинхронно, объекты, секреты), приложение запущено в двух экземплярах и растёт до полного размера за минуты. "
    "База в облаке повышается вручную по решению дежурного, поэтому восстановление считается часами. Копии лежат в облаке в неизменяемом хранилище.",
)

dc = a.zone("dc", "Основной ЦОД", "dc", role="active", tier="III", geo="moscow", costModel="onprem")
cloud = a.zone("cloud", "Облако · резервная площадка", "cloud", role="standby", geo="cloud-north", costModel="cloud")
arbiter = a.zone("arbiter", "Облако · арбитр", "cloud", role="witness", geo="cloud-south", costModel="cloud")

a.node("users", "users", "Пользователи", rpsLabel="2.5k RPS пик")
a.node("gslb", "gslb", "GSLB", group="gslb", replicas=3, healthCheckSeconds=10, failThreshold=3, dnsTtlSeconds=60)
a.node("waf", "waf", "WAF", group="waf", replicas=3)
a.chain("users", "gslb", "waf")
a.node("lb-dc", "l7", "Ingress", dc, "ingress", replicas=2)
a.node("lb-cloud", "l7", "Ingress (резерв)", cloud, "ingress", replicas=2)
a.node("app-dc", "kubernetes", "Kubernetes · приложение", dc, "app", replicas=40, vcpu=1, ramGb=2, hpaMin=30, hpaMax=80, clusterName="main")
a.node("app-cloud", "kubernetes", "Kubernetes · приложение (пилотная лампа)", cloud, "app", replicas=2, failoverReplicas=60, scaleUpSeconds=600, vcpu=1, ramGb=2, hpaMin=2, hpaMax=80, clusterName="dr")
a.edge("waf", "lb-dc")
a.edge("waf", "lb-cloud")
a.edge("lb-dc", "app-dc")
a.edge("lb-cloud", "app-cloud")

# data: PostgreSQL with a synchronous replica in the data center and an asynchronous one in the cloud that is promoted by hand
pg = dict(version="16", vcpu=16, ramGb=64, diskGb=1000, connectionPooler=True)
a.node("pg-1", "postgresql", "PostgreSQL primary", dc, "pg", "primary", **store(failover="auto", failoverSeconds=30, dcs="etcd", **pg))
a.node("pg-2", "postgresql", "PostgreSQL sync replica", dc, "pg", "replica", **store(failover="auto", failoverSeconds=30, dcs="etcd", **pg))
a.node("pg-dr", "postgresql", "PostgreSQL replica в облаке (повышается вручную)", cloud, "pg", "replica", **store(failover="manual", dcs="etcd", **pg))
a.node("etcd-1", "etcd", "etcd", dc, "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
a.node("etcd-2", "etcd", "etcd", cloud, "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
a.node("etcd-w", "etcd", "etcd (арбитр)", arbiter, "etcd", "witness", vcpu=2, ramGb=4, diskGb=50)
a.quorum("etcd-1", "etcd-2", "etcd-w", ms=14, paths=2)
a.node("redis-1", "redis", "Redis (master)", dc, "redis", "primary", mode="sentinel", vcpu=4, ramGb=32)
a.node("redis-2", "redis", "Redis (реплика)", dc, "redis", "replica", mode="sentinel", vcpu=4, ramGb=32)
a.node("redis-3", "redis", "Redis (в облаке)", cloud, "redis", "replica", mode="sentinel", vcpu=2, ramGb=16)
a.call("app-dc", "pg-1", 0.3, fanoutColdCache=0.8, tls=True)
a.call("app-cloud", "pg-dr", 0.3, fanoutColdCache=0.8, tls=True)
a.call("app-dc", "redis-1", 1)
a.call("app-cloud", "redis-3", 1, tls=True)
a.sync("pg-1", "pg-2")
a.async_("pg-1", "pg-dr", lag=5, ms=14, mbps=2000, paths=2, label="в облако · async")
a.async_("redis-1", "redis-2")
a.async_("redis-1", "redis-3", lag=2, ms=14, mbps=2000, paths=2)

# files, secrets, monitoring, the way of a change
a.node("files-dc", "object_storage", "Файлы (MinIO)", dc, "files", variant="minio", replicas=4, vcpu=4, ramGb=16, diskGb=12000, encryptionAtRest=True)
a.node("files-cloud", "object_storage", "Файлы (облачное S3)", cloud, "files-dr", variant="cloud", replicas=3, diskGb=12000, encryptionAtRest=True, vcpu=2, ramGb=4)
a.call("app-dc", "files-dc", 0.1, tls=True)
a.call("app-cloud", "files-cloud", 0.1, tls=True)
a.async_("files-dc", "files-cloud", lag=30, ms=14, mbps=2000, paths=2, label="репликация объектов")
a.node("vault-1", "vault", "Vault", dc, "vault", "primary", vcpu=2, ramGb=4, diskGb=50)
a.node("vault-2", "vault", "Vault (DR)", cloud, "vault", "replica", vcpu=2, ramGb=4, diskGb=50)
a.async_("vault-1", "vault-2", lag=5, ms=14, mbps=1000, paths=2, label="DR replication")
a.node("mon-dc", "monitoring", "Мониторинг", dc, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
a.node("mon-cloud", "monitoring", "Мониторинг (резерв)", cloud, "monitoring", replicas=1, vcpu=2, ramGb=8, diskGb=100)
a.node("gitops", "gitops", "GitOps · Argo CD", cloud, "gitops", replicas=2, vcpu=2, ramGb=4)
a.control("mon-dc", "app-dc", tls=False)
a.control("mon-cloud", "app-cloud", tls=False)
a.control("gitops", "app-dc")
a.control("gitops", "app-cloud")

# the copies live in the cloud, outside the domain of what they protect
a.node("backup", "backup_storage", "Бэкапы в облаке (object lock)", cloud, **backup_props(retentionDays=35, restoreSeconds=10800, diskGb=20000))
a.backup("pg-2", "backup", label="WAL + full")
a.backup("files-dc", "backup", every=3600)
a.backup("vault-1", "backup", every=3600)

TEMPLATE = a
