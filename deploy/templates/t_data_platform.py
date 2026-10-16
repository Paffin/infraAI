from lib import Arch, backup_props, requirements, store

# An event analytics platform in three availability zones: events come through the ingest API into Kafka, Flink turns them into tables in ClickHouse and files in the object-storage
# lake, Airflow runs the batch jobs, dashboards read ClickHouse. The services that keep state (Kafka, ClickHouse, the lake) are spread over all three zones with their own quorum.
a = Arch(
    "data-platform", ("Платформа данных: Kafka, Flink, ClickHouse", "Data platform: Kafka, Flink, ClickHouse"),
    ("Приём событий через Kafka на трёх AZ, потоковая обработка Flink, аналитика в ClickHouse и озеро в объектном хранилище, пакетные задачи в Airflow, дашборды в Grafana и Superset.",
     "Event ingest through Kafka across three AZs, stream processing with Flink, analytics in ClickHouse and a lake in object storage, batch jobs in Airflow, dashboards in Grafana and Superset."),
    "Data", 15, ["data", "analytics", "kafka", "clickhouse", "ha"],
    requirements(2000, 60, 900, 5, 5000000, "site", 99.9),
    "Платформа данных: Kafka, Flink, ClickHouse",
    "События приходят через API приёма в Kafka (три AZ, репликация ×3). Flink обрабатывает поток и пишет в ClickHouse (реплики в трёх зонах, Keeper) и в озеро на объектном хранилище. Пакетные задачи ведёт Airflow, "
    "дашборды читают ClickHouse. Потеря зоны не останавливает приём и не теряет подтверждённые события; копии ClickHouse и озера лежат на отдельной площадке.",
)

region = a.zone("region", "Регион eu-1", "region")
az = {x: a.zone(f"az-{x}", f"AZ {x.upper()}", "az", region, geo="eu-1", costModel="cloud") for x in "abc"}
az_backup = a.zone("site-backup", "Площадка копий", "cloud", geo="eu-2", costModel="cloud")

a.node("users", "users", "Источники событий (приложения, устройства)", rpsLabel="2k пакетов/с (≈100k событий/с)")
a.node("waf", "waf", "WAF", group="waf", replicas=3)
a.edge("users", "waf")
for x, z in az.items():
    a.node(f"lb-{x}", "l7", "Ingress", z, "ingress", variant="ingress-k8s", replicas=2)
    a.node(f"ingest-{x}", "kubernetes", "Kubernetes · API приёма", z, "ingest", replicas=12, vcpu=2, ramGb=4, hpaMin=8, hpaMax=40, clusterName="data")
    a.node(f"kafka-{x}", "kafka", "Kafka (KRaft)", z, "kafka", variant="kraft", replicas=2, replicationFactor=3, minInsync=2, vcpu=8, ramGb=32, diskGb=1000, storageClass="nvme", encryptionAtRest=True)
    a.node(f"flink-{x}", "flink", "Flink", z, "flink", replicas=4, vcpu=8, ramGb=32, diskGb=200)
    a.node(f"ch-{x}", "clickhouse", "ClickHouse + Keeper", z, "clickhouse", variant="replicated", replicas=2, minInsync=2, vcpu=16, ramGb=64, diskGb=2000, storageClass="nvme", encryptionAtRest=True)
    a.node(f"lake-{x}", "object_storage", "Озеро данных (MinIO)", z, "lake", variant="minio", replicas=3, vcpu=4, ramGb=16, diskGb=8000, encryptionAtRest=True)
    a.node(f"mon-{x}", "monitoring", "Мониторинг", z, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
    a.edge("waf", f"lb-{x}")
    a.chain(f"lb-{x}", f"ingest-{x}")
    a.call(f"ingest-{x}", f"kafka-{x}", 1, tls=True)
    a.call(f"kafka-{x}", f"flink-{x}", 1, tls=True)
    a.call(f"flink-{x}", f"ch-{x}", 0.2, tls=True)   # inserts are batches: one in five requests
    a.call(f"flink-{x}", f"lake-{x}", 0.1, tls=True)
    a.control(f"mon-{x}", f"ingest-{x}", tls=False)
a.quorum("kafka-a", "kafka-b", "kafka-c")
a.quorum("ch-a", "ch-b", "ch-c")
a.quorum("lake-a", "lake-b", "lake-c")

# batch jobs and dashboards: two zones each; they read, they are not on the way of an event
a.node("airflow-a", "airflow", "Airflow", az["a"], "airflow", replicas=2, vcpu=4, ramGb=8)
a.node("airflow-b", "airflow", "Airflow", az["b"], "airflow", replicas=2, vcpu=4, ramGb=8)
a.node("bi-b", "apache_superset", "Superset", az["b"], "bi", replicas=2, vcpu=2, ramGb=4)
a.node("bi-c", "apache_superset", "Superset", az["c"], "bi", replicas=2, vcpu=2, ramGb=4)
a.node("grafana", "grafana", "Grafana", az["c"], "grafana", replicas=2, vcpu=2, ramGb=4)
for src, dst in (("airflow-a", "ch-a"), ("airflow-b", "ch-b"), ("bi-b", "ch-b"), ("bi-c", "ch-c"), ("grafana", "ch-c")):
    a.control(src, dst)

# the way of a change and the secrets
a.node("gitops", "gitops", "GitOps · Argo CD", az["c"], "gitops", replicas=2, vcpu=2, ramGb=4)
for x in "abc":
    a.control("gitops", f"ingest-{x}")
a.node("vault-a", "vault", "Vault", az["a"], "vault", "primary", vcpu=2, ramGb=4, diskGb=50)
a.node("vault-b", "vault", "Vault (standby)", az["b"], "vault", "replica", vcpu=2, ramGb=4, diskGb=50)
a.async_("vault-a", "vault-b", lag=1)

# the copies: the lake and the tables are copied out of the region, locked
a.node("backup", "backup_storage", "Бэкапы (объектное хранилище, object lock)", az_backup, **backup_props(retentionDays=35, restoreSeconds=14400, diskGb=40000))
a.backup("ch-b", "backup", every=3600, label="снимки таблиц")
a.backup("lake-c", "backup", every=3600, label="копия озера")
a.backup("vault-a", "backup", every=3600)
a.backup("kafka-c", "backup", every=3600, label="Tiered storage / MirrorMaker")

TEMPLATE = a
