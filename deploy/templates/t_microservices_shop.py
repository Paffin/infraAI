from lib import Arch, backup_props, requirements, store

# An online shop split into services on Kubernetes in three availability zones: the gateway fans a request out to the catalog (search and cache), the cart (cache) and the orders
# (database and message broker); a service mesh encrypts the traffic between them. Every stateful service has a quorum or a replica in each zone, so a zone can go.
a = Arch(
    "microservices-shop", ("Микросервисы: интернет-магазин на Kubernetes", "Microservices: an online shop on Kubernetes"),
    ("API-шлюз, каталог с OpenSearch, корзина на Redis, заказы на PostgreSQL и RabbitMQ, service mesh и наблюдаемость на трёх AZ. Каждый сервис переживает потерю зоны.",
     "An API gateway, a catalog on OpenSearch, a cart on Redis, orders on PostgreSQL and RabbitMQ, a service mesh and observability across three AZs. Every service survives the loss of a zone."),
    "Shop", 18, ["microservices", "kubernetes", "ecommerce", "mesh", "ha"],
    requirements(4000, 50, 300, 0, 3400000, "site", 99.9),
    "Микросервисы: интернет-магазин на Kubernetes",
    "Запрос проходит CDN, WAF, Ingress и API-шлюз Kong и расходится по сервисам: каталог (OpenSearch и кэш), корзина (Redis) и заказы (PostgreSQL и RabbitMQ). Service mesh шифрует трафик между сервисами, "
    "мониторинг стоит в каждой зоне, изменения идут через Argo CD. Хранилища с кворумом и репликами есть в каждой из трёх зон.",
)

region = a.zone("region", "Регион eu-1", "region")
az = {x: a.zone(f"az-{x}", f"AZ {x.upper()}", "az", region, geo="eu-1", costModel="cloud") for x in "abc"}
az_backup = a.zone("site-backup", "Площадка копий", "cloud", geo="eu-2", costModel="cloud")

a.node("users", "users", "Покупатели", rpsLabel="4k RPS пик")
a.node("cdn", "cdn", "CDN", group="cdn")
a.node("waf", "waf", "WAF", group="waf", replicas=3)
a.chain("users", "cdn", "waf")
share = {"catalog": 0.6, "cart": 0.25, "orders": 0.15}
for x, z in az.items():
    a.node(f"lb-{x}", "l7", "Ingress", z, "ingress", variant="ingress-k8s", replicas=2)
    a.node(f"gw-{x}", "kong", "API-шлюз (Kong)", z, "gateway", variant="dbless", replicas=3)
    a.node(f"mesh-{x}", "service_mesh", "Service mesh (Istio)", z, "mesh", variant="istio", replicas=2)
    a.node(f"mon-{x}", "monitoring", "Мониторинг", z, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
    for svc, name, replicas in (("catalog", "каталог", 14), ("cart", "корзина", 6), ("orders", "заказы", 6)):
        a.node(f"{svc}-{x}", "kubernetes", f"Сервис «{name}»", z, svc, replicas=replicas, vcpu=1, ramGb=2, hpaMin=max(2, replicas - 2), hpaMax=replicas * 4, clusterName="shop")
        a.call(f"gw-{x}", f"{svc}-{x}", share[svc], tls=True)
        a.control(f"mesh-{x}", f"{svc}-{x}")
        a.control(f"mon-{x}", f"{svc}-{x}", tls=False)
    a.node(f"search-{x}", "opensearch", "OpenSearch", z, "search", variant="dedicated", replicas=3, minInsync=2, vcpu=4, ramGb=16, diskGb=500, encryptionAtRest=True)
    a.node(f"rabbit-{x}", "rabbitmq", "RabbitMQ", z, "rabbit", variant="quorum-queues", replicas=1, replicationFactor=3, vcpu=4, ramGb=8, diskGb=100, encryptionAtRest=True)
    a.edge("waf", f"lb-{x}")
    a.chain(f"lb-{x}", f"gw-{x}")
    a.call(f"catalog-{x}", f"search-{x}", 1, tls=True)
    a.call(f"orders-{x}", f"rabbit-{x}", 1, tls=True)
a.quorum("search-a", "search-b", "search-c")
a.quorum("rabbit-a", "rabbit-b", "rabbit-c")

# state: the cache (catalog, cart) and the database of the orders
a.node("cache-a", "redis", "Redis (кэш и корзины)", az["a"], "cache", "primary", mode="sentinel", vcpu=4, ramGb=32)
a.node("cache-b", "redis", "Redis (реплика)", az["b"], "cache", "replica", mode="sentinel", vcpu=4, ramGb=32)
a.node("cache-c", "redis", "Redis Sentinel", az["c"], "cache", "witness", mode="sentinel", vcpu=1, ramGb=1)
pg = dict(version="16", vcpu=8, ramGb=32, diskGb=500, failover="auto", failoverSeconds=30, dcs="etcd", connectionPooler=True)
a.node("pg-a", "postgresql", "PostgreSQL primary (заказы)", az["a"], "pg", "primary", **store(**pg))
a.node("pg-b", "postgresql", "PostgreSQL sync replica", az["b"], "pg", "replica", **store(**pg))
a.node("pg-c", "postgresql", "PostgreSQL async replica", az["c"], "pg", "replica", **store(**pg))
for x in "abc":
    a.node(f"etcd-{x}", "etcd", "etcd", az[x], "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
a.quorum("etcd-a", "etcd-b", "etcd-c")
for x, c in (("a", "cache-a"), ("b", "cache-b"), ("c", "cache-b")):
    a.call(f"catalog-{x}", c, 1, tls=True)
    a.call(f"cart-{x}", c, 1, tls=True)
    a.call(f"orders-{x}", f"pg-{x}", 1, fanoutColdCache=1.2, tls=True)
a.sync("pg-a", "pg-b")
a.async_("pg-a", "pg-c", lag=1)
a.async_("cache-a", "cache-b")

# the way of a change, the secrets and the copies
a.node("gitops", "gitops", "GitOps · Argo CD", az["c"], "gitops", replicas=2, vcpu=2, ramGb=4)
for x in "abc":
    for svc in share:
        a.control("gitops", f"{svc}-{x}")
a.node("vault-a", "vault", "Vault", az["a"], "vault", "primary", vcpu=2, ramGb=4, diskGb=50)
a.node("vault-b", "vault", "Vault (standby)", az["b"], "vault", "replica", vcpu=2, ramGb=4, diskGb=50)
a.async_("vault-a", "vault-b", lag=1)
a.node("backup", "backup_storage", "Бэкапы (объектное хранилище, object lock)", az_backup, **backup_props(retentionDays=35, restoreSeconds=7200, diskGb=10000))
a.backup("pg-b", "backup", label="WAL + full")
a.backup("search-c", "backup", every=3600, label="снимки индексов")
a.backup("vault-a", "backup", every=3600)

TEMPLATE = a
