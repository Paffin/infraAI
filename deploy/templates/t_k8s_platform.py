from lib import Arch, backup_props, requirements, store

# A Kubernetes platform for several teams on three sites of one city: one cluster stretched over the sites (etcd on three voters), GitOps as the only way
# a change reaches it, secrets in Vault, metrics, logs and dashboards, an image registry, certificates and a service mesh, the platform database with replicas.
a = Arch(
    "k8s-platform", ("Kubernetes-платформа: GitOps и наблюдаемость", "Kubernetes platform: GitOps and observability"),
    ("Один кластер на трёх площадках города: etcd на трёх голосах, Argo CD, Vault, Prometheus, Loki, Grafana, Harbor, cert-manager и service mesh. Платформа переживает потерю площадки.",
     "One cluster over three sites of a city: etcd on three voters, Argo CD, Vault, Prometheus, Loki, Grafana, Harbor, cert-manager and a service mesh. The platform survives the loss of a site."),
    "K8s", 13, ["kubernetes", "platform", "gitops", "observability", "on-prem", "ha"],
    requirements(6000, 40, 600, 0, 3200000, "site", 99.95),
    "Kubernetes-платформа: GitOps, секреты, наблюдаемость",
    "Растянутый кластер Kubernetes на трёх площадках одного города: изменения попадают в него только через Argo CD, секреты — в Vault, метрики, логи и дашборды — Prometheus, Loki и Grafana, образы — в Harbor, "
    "сертификаты выпускает cert-manager, трафик между сервисами шифрует service mesh. Данные платформы — PostgreSQL с репликами на всех площадках.",
)

sites = {n: a.zone(f"dc{n}", f"ЦОД-{n}", "dc", role="active", tier="III", geo="moscow", costModel="onprem") for n in (1, 2, 3)}

a.node("users", "users", "Разработчики и сервисы", rpsLabel="6k RPS пик")
a.node("waf", "waf", "WAF", group="waf", replicas=3)
for n, z in sites.items():
    a.node(f"ingress-{n}", "l7", "Ingress", z, "ingress", variant="ingress-k8s", replicas=2, healthCheckSeconds=3, failThreshold=2)
    a.node(f"app-{n}", "kubernetes", "Kubernetes · приложения", z, "app", replicas=28, vcpu=1, ramGb=2, hpaMin=20, hpaMax=70, clusterName="platform")
    a.node(f"etcd-{n}", "etcd", "etcd кластера", z, "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
    a.node(f"mon-{n}", "monitoring", "Prometheus + Alertmanager", z, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
    a.node(f"loki-{n}", "loki", "Loki", z, "loki", replicas=3, vcpu=4, ramGb=8, diskGb=500)
    a.node(f"mesh-{n}", "service_mesh", "Service mesh (Istio)", z, "mesh", variant="istio", replicas=2)
    a.chain("users", "waf") if n == 1 else None
    a.edge("waf", f"ingress-{n}", "traffic")
    a.edge(f"ingress-{n}", f"app-{n}", "traffic")
    a.control(f"mon-{n}", f"app-{n}", tls=False)
    a.control(f"mesh-{n}", f"app-{n}")
a.quorum("etcd-1", "etcd-2", "etcd-3")

# GitOps and the registry: two sites each, the cluster pulls from them
a.node("gitops-1", "gitops", "GitOps · Argo CD", sites[1], "gitops", replicas=2, vcpu=2, ramGb=4)
a.node("gitops-2", "gitops", "GitOps · Argo CD", sites[2], "gitops", replicas=2, vcpu=2, ramGb=4)
a.node("registry-2", "harbor", "Harbor (образы)", sites[2], "registry", replicas=2, vcpu=4, ramGb=8, diskGb=2000)
a.node("registry-3", "harbor", "Harbor (образы)", sites[3], "registry", replicas=2, vcpu=4, ramGb=8, diskGb=2000)
a.node("certs-1", "cert_manager", "cert-manager", sites[1], "certs", replicas=2)
a.node("certs-3", "cert_manager", "cert-manager", sites[3], "certs", replicas=2)
a.node("grafana", "grafana", "Grafana", sites[3], "grafana", replicas=2, vcpu=2, ramGb=4)
for n in (1, 2, 3):
    for g in ("gitops-1", "gitops-2"):
        a.control(g, f"app-{n}")

# secrets: Vault on three sites
for n in (1, 2, 3):
    a.node(f"vault-{n}", "vault", "Vault (Raft)", sites[n], "vault", "voter", variant="raft", vcpu=2, ramGb=4, diskGb=50)
a.quorum("vault-1", "vault-2", "vault-3", ms=1.2, paths=2)

# the platform database: PostgreSQL with a synchronous replica and an asynchronous one
pg = dict(version="16", vcpu=8, ramGb=32, diskGb=500, failover="auto", failoverSeconds=30, dcs="etcd", connectionPooler=True)
a.node("pg-1", "postgresql", "PostgreSQL primary", sites[1], "pg", "primary", **store(**pg))
a.node("pg-2", "postgresql", "PostgreSQL sync replica", sites[2], "pg", "replica", **store(**pg))
a.node("pg-3", "postgresql", "PostgreSQL async replica", sites[3], "pg", "replica", **store(**pg))
for n in (1, 2, 3):
    a.call(f"app-{n}", f"pg-{n}", 0.3, fanoutColdCache=0.8, tls=True)
a.sync("pg-1", "pg-2", ms=1.2, mbps=10000, paths=2)
a.async_("pg-1", "pg-3", lag=1, ms=1.4, mbps=10000, paths=2)

# the copies: a locked store on the third site
a.node("backup", "backup_storage", "Бэкапы (объектное хранилище)", sites[3], **backup_props(restoreSeconds=3600, diskGb=20000))
a.backup("pg-2", "backup", label="WAL + full")
a.backup("vault-1", "backup", every=3600)

TEMPLATE = a
