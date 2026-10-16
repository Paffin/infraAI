from lib import Arch, backup_props, requirements, store

# A retrieval-augmented assistant: the service orchestrates, a vector database finds the passages, the language model runs on GPUs of its own pool in every zone.
# The model is the scarce resource (about ten requests a second per GPU), so the design is sized by it and loses nothing when a zone goes: the other two carry the load.
a = Arch(
    "rag-llm", ("RAG-ассистент: LLM на GPU и векторная БД", "RAG assistant: LLM on GPUs and a vector database"),
    ("Сервис оркестрации, Qdrant на трёх AZ, vLLM на пулах GPU в каждой зоне, семантический кэш, история диалогов в PostgreSQL, документы в объектном хранилище, SSO через Keycloak.",
     "An orchestration service, Qdrant across three AZs, vLLM on GPU pools in every zone, a semantic cache, dialogue history in PostgreSQL, documents in object storage, SSO through Keycloak."),
    "AI", 16, ["ai", "llm", "rag", "vector-db", "gpu", "ha"],
    requirements(80, 50, 600, 60, 6500000, "site", 99.9),
    "RAG-ассистент: LLM на GPU и векторная БД",
    "Запрос проходит WAF, шлюз и сервис оркестрации; тот находит фрагменты документов в Qdrant (кластер на трёх зонах) и просит vLLM ответить с их учётом. Пулы GPU с vLLM стоят в каждой зоне и рассчитаны на потерю одной: "
    "две оставшиеся тянут пик. Диалоги — в PostgreSQL, документы — в объектном хранилище, повторяющиеся запросы закрывает семантический кэш.",
)

region = a.zone("region", "Регион eu-1", "region")
az = {x: a.zone(f"az-{x}", f"AZ {x.upper()}", "az", region, geo="eu-1", costModel="cloud") for x in "abc"}
az_backup = a.zone("site-backup", "Площадка копий", "cloud", geo="eu-2", costModel="cloud")

a.node("users", "users", "Пользователи ассистента", rpsLabel="80 запросов/с пик")
a.node("waf", "waf", "WAF", group="waf", replicas=3)
a.node("idp-a", "keycloak", "Keycloak (SSO)", az["a"], "idp", replicas=3, variant="multi-site")
a.node("idp-b", "keycloak", "Keycloak (SSO)", az["b"], "idp", replicas=3, variant="multi-site")
a.edge("users", "waf")
for x, z in az.items():
    a.node(f"lb-{x}", "l7", "Ingress", z, "ingress", variant="ingress-k8s", replicas=2)
    a.node(f"gw-{x}", "kong", "API-шлюз (Kong)", z, "gateway", variant="dbless", replicas=3)
    a.node(f"rag-{x}", "kubernetes", "Kubernetes · RAG-сервис", z, "rag", replicas=10, vcpu=2, ramGb=4, hpaMin=6, hpaMax=30, clusterName="ai")
    a.node(f"llm-{x}", "vllm", "vLLM · пул GPU", z, "llm", replicas=8, gpus=1, vcpu=8, ramGb=64, diskGb=500)
    a.node(f"vec-{x}", "qdrant", "Qdrant", z, "vector", replicas=1, replicationFactor=3, minInsync=2, vcpu=8, ramGb=32, diskGb=500, encryptionAtRest=True)
    a.node(f"mon-{x}", "monitoring", "Мониторинг", z, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
    a.edge("waf", f"lb-{x}")
    a.chain(f"lb-{x}", f"gw-{x}", f"rag-{x}")
    a.call(f"rag-{x}", f"vec-{x}", 1, tls=True)
    a.call(f"rag-{x}", f"llm-{x}", 1, tls=True)
    a.control(f"mon-{x}", f"rag-{x}", tls=False)
a.quorum("vec-a", "vec-b", "vec-c")
for x, idp in (("a", "idp-a"), ("b", "idp-b"), ("c", "idp-b")):
    a.call(f"gw-{x}", idp, 0.05, tls=True)

# the semantic cache, the dialogues, the documents
a.node("cache-a", "redis", "Redis (семантический кэш)", az["a"], "cache", "primary", mode="sentinel", vcpu=4, ramGb=32)
a.node("cache-b", "redis", "Redis (реплика)", az["b"], "cache", "replica", mode="sentinel", vcpu=4, ramGb=32)
a.node("cache-c", "redis", "Redis Sentinel", az["c"], "cache", "witness", mode="sentinel", vcpu=1, ramGb=1)
pg = dict(version="16", vcpu=8, ramGb=32, diskGb=500, failover="auto", failoverSeconds=30, dcs="etcd", connectionPooler=True)
a.node("pg-a", "postgresql", "PostgreSQL primary (диалоги)", az["a"], "pg", "primary", **store(**pg))
a.node("pg-b", "postgresql", "PostgreSQL sync replica", az["b"], "pg", "replica", **store(**pg))
a.node("pg-c", "postgresql", "PostgreSQL async replica", az["c"], "pg", "replica", **store(**pg))
for x in "abc":
    a.node(f"etcd-{x}", "etcd", "etcd", az[x], "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
    a.node(f"docs-{x}", "object_storage", "Документы (MinIO)", az[x], "docs", variant="minio", replicas=3, vcpu=4, ramGb=16, diskGb=4000, encryptionAtRest=True)
a.quorum("etcd-a", "etcd-b", "etcd-c")
a.quorum("docs-a", "docs-b", "docs-c")
for x, c in (("a", "cache-a"), ("b", "cache-b"), ("c", "cache-b")):
    a.call(f"rag-{x}", c, 1, tls=True)
    a.call(f"rag-{x}", f"pg-{x}", 0.3, fanoutColdCache=0.8, tls=True)
    a.call(f"rag-{x}", f"docs-{x}", 0.1, tls=True)
a.sync("pg-a", "pg-b")
a.async_("pg-a", "pg-c", lag=1)
a.async_("cache-a", "cache-b")

# the way of a change, the secrets and the copies
a.node("gitops", "gitops", "GitOps · Argo CD", az["c"], "gitops", replicas=2, vcpu=2, ramGb=4)
for x in "abc":
    a.control("gitops", f"rag-{x}")
a.node("vault-a", "vault", "Vault", az["a"], "vault", "primary", vcpu=2, ramGb=4, diskGb=50)
a.node("vault-b", "vault", "Vault (standby)", az["b"], "vault", "replica", vcpu=2, ramGb=4, diskGb=50)
a.async_("vault-a", "vault-b", lag=1)
a.node("backup", "backup_storage", "Бэкапы (объектное хранилище, object lock)", az_backup, **backup_props(retentionDays=35, restoreSeconds=10800, diskGb=20000))
a.backup("pg-b", "backup", label="WAL + full")
a.backup("vec-c", "backup", every=3600, label="снимки коллекций")
a.backup("docs-c", "backup", every=3600)
a.backup("vault-a", "backup", every=3600)

TEMPLATE = a
