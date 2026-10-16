from lib import Arch, backup_props, requirements, store

# A payment system in two data centers and a third site that holds the arbiter: no committed payment may be lost (RPO 0), the keys are in HSMs of both data centers,
# access of administrators goes through a bastion, the events of the platform go to a SIEM. Built on the quorum of "2 DCs + witness" with the layers a regulator asks for.
a = Arch(
    "payments-two-dc", ("Платёжная система: 2 ЦОД, HSM, SIEM", "Payments: 2 DCs, HSM, SIEM"),
    ("Два активных ЦОД и арбитр на третьей площадке: синхронная репликация базы (RPO 0), ключи в HSM обоих ЦОД, доступ администраторов через бастион, события в SIEM. Переживает потерю ЦОД и разрыв канала без split-brain.",
     "Two active data centers and an arbiter on a third site: synchronous database replication (RPO 0), keys in the HSMs of both data centers, administrators through a bastion, events to a SIEM. Survives the loss of a data center and a link cut without split-brain."),
    "Bank", 14, ["finance", "security", "dr", "on-prem", "hsm", "compliance"],
    requirements(3000, 30, 300, 0, 3600000, "site", 99.95, compliance=["PCI DSS", "ГОСТ Р 57580"]),
    "Платёжная система: 2 ЦОД, HSM, SIEM",
    "Active/active в двух ЦОД с арбитром кворума на третьей площадке. Данные платежей реплицируются синхронно (RPO 0), криптографические ключи хранятся в HSM обоих ЦОД, администраторы работают через бастион, "
    "события безопасности собираются в SIEM; копии лежат на третьей площадке в неизменяемом хранилище.",
)

dc1 = a.zone("dc1", "ЦОД-1", "dc", role="active", tier="III", geo="moscow", costModel="onprem")
dc2 = a.zone("dc2", "ЦОД-2", "dc", role="active", tier="III", geo="moscow", costModel="onprem")
site3 = a.zone("site3", "Площадка 3 · арбитр и копии", "dc", role="witness", tier="III", geo="spb", costModel="onprem")
dcs = {1: dc1, 2: dc2}

a.node("users", "users", "Клиенты и банки-партнёры", rpsLabel="3k RPS пик")
a.node("gslb", "gslb", "GSLB", group="gslb", replicas=3, healthCheckSeconds=5, failThreshold=1, dnsTtlSeconds=30)
a.node("waf", "waf", "WAF", group="waf", replicas=3)
a.chain("users", "gslb", "waf")
for n, z in dcs.items():
    a.node(f"ingress-{n}", "l7", "Ingress", z, "ingress", replicas=2, healthCheckSeconds=3, failThreshold=2)
    a.node(f"gw-{n}", "api_gateway", "API-шлюз", z, "gateway", replicas=3)
    a.node(f"app-{n}", "kubernetes", "Kubernetes · платёжные сервисы", z, "app", replicas=32, vcpu=1, ramGb=2, hpaMin=24, hpaMax=64)
    a.node(f"kafka-{n}", "kafka", "Kafka", z, f"kafka-{n}", replicas=3, replicationFactor=3, minInsync=2, vcpu=4, ramGb=16, diskGb=500, storageClass="nvme", encryptionAtRest=True)
    a.node(f"mon-{n}", "monitoring", "Мониторинг", z, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
    a.node(f"hsm-{n}", "thales_luna_hsm", "HSM (ключи платёжных операций)", z, "hsm", replicas=2, vcpu=2, ramGb=4)
    a.node(f"siem-{n}", "wazuh", "SIEM · сбор событий", z, "siem", variant="self-hosted", replicas=2, vcpu=4, ramGb=8, diskGb=500)
    a.node(f"etcd-{n}", "etcd", "etcd", z, "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
    a.edge("waf", f"ingress-{n}", "traffic")
    a.chain(f"ingress-{n}", f"gw-{n}", f"app-{n}")
    a.call(f"app-{n}", f"kafka-{n}", 0.2, tls=True)
    a.call(f"app-{n}", f"hsm-{n}", 0.4, tls=True)
    a.control(f"mon-{n}", f"app-{n}", tls=False)
    a.control(f"siem-{n}", f"app-{n}")
a.async_("kafka-1", "kafka-2", lag=2, ms=1.2, mbps=10000, paths=2, label="MirrorMaker · async",
         mitigation="Платежи защищены transactional outbox в PostgreSQL: события в полёте (≈2 с) публикуются повторно после переключения.")
a.async_("hsm-1", "hsm-2", lag=1, ms=1.2, mbps=1000, paths=2, label="кластер HSM")

# the data: PostgreSQL with a synchronous replica in the other data center; Redis for sessions
pg = dict(version="16", vcpu=16, ramGb=64, diskGb=1000, storageClass="nvme", failover="auto", failoverSeconds=30, dcs="etcd", connectionPooler=True)
a.node("pg-1", "postgresql", "PostgreSQL primary", dc1, "pg", "primary", **store(**pg))
a.node("pg-2", "postgresql", "PostgreSQL sync replica", dc2, "pg", "replica", **store(**pg))
a.node("redis-1", "redis", "Redis (сессии)", dc1, "redis", "primary", mode="sentinel", vcpu=4, ramGb=32)
a.node("redis-2", "redis", "Redis (реплика)", dc2, "redis", "replica", mode="sentinel", vcpu=4, ramGb=32)
a.node("redis-s", "redis", "Redis Sentinel", site3, "redis", "witness", mode="sentinel", vcpu=1, ramGb=1)
a.node("etcd-s", "etcd", "etcd (арбитр)", site3, "etcd", "witness", vcpu=2, ramGb=4, diskGb=50)
for n in (1, 2):
    a.call(f"app-{n}", f"pg-{n}", 0.3, fanoutColdCache=0.8, tls=True)
    a.call(f"app-{n}", f"redis-{n}", 1)
a.sync("pg-1", "pg-2", ms=1.2, mbps=10000, paths=2, label="sync · RTT 1.2 мс")
a.async_("redis-1", "redis-2", lag=1, ms=1.2, mbps=10000, paths=2)
a.quorum("etcd-1", "etcd-2", "etcd-s", ms=1.2, paths=2)

# secrets, access, the way of a change into production, and the copies
a.node("vault-1", "vault", "Vault", dc1, "vault", "primary", vcpu=2, ramGb=4, diskGb=50)
a.node("vault-3", "vault", "Vault (DR)", site3, "vault", "replica", vcpu=2, ramGb=4, diskGb=50)
a.async_("vault-1", "vault-3", lag=5, ms=12, mbps=1000, paths=2, label="DR replication")
a.node("bastion", "teleport", "Бастион (Teleport)", site3, "access", replicas=2, vcpu=2, ramGb=4)
a.node("gitops", "gitops", "GitOps · Argo CD", site3, "gitops", replicas=2, vcpu=2, ramGb=4)
for n in (1, 2):
    a.control("gitops", f"app-{n}")
a.node("backup", "backup_storage", "Бэкапы (неизменяемое хранилище)", site3, **backup_props(restoreSeconds=7200, diskGb=20000))
a.backup("pg-2", "backup", label="WAL + full / сутки")
a.backup("vault-1", "backup", every=3600)
a.backup("hsm-1", "backup", every=86400, label="резервная копия ключей")
a.backup("siem-1", "backup", every=3600, label="индексы событий")
a.backup("siem-2", "backup", every=3600, label="индексы событий")

TEMPLATE = a
