from lib import Arch, backup_props, requirements, store

# Telemetry from devices: they connect over MQTT with TLS to a TCP balancer, an EMQX cluster holds the sessions, ThingsBoard parses the messages and writes the readings to TimescaleDB, Grafana shows them. The devices reconnect by themselves, which is why the design counts on a zone going away rather than on keeping every connection.
a = Arch(
    "iot-telemetry", ("IoT-телеметрия: MQTT, EMQX и TimescaleDB", "IoT telemetry: MQTT, EMQX and TimescaleDB"),
    ("Устройства подключаются по MQTT через L4-балансировщик к кластеру EMQX на трёх AZ, ThingsBoard разбирает сообщения и пишет показания в TimescaleDB, Grafana их показывает. Потеря зоны — переподключение устройств, не потеря данных.",
     "Devices connect over MQTT through an L4 balancer to an EMQX cluster across three AZs, ThingsBoard parses the messages and writes the readings to TimescaleDB, Grafana shows them. A lost zone means devices reconnecting, not lost data."),
    "IoT", 20, ["iot", "mqtt", "timeseries", "ha"],
    requirements(5000, 100, 300, 5, 2600000, "site", 99.9),
    "IoT-телеметрия: MQTT, EMQX и TimescaleDB",
    "Устройства держат MQTT-соединения с TLS на L4-балансировщике; кластер EMQX на трёх зонах принимает сообщения, ThingsBoard разбирает их и пишет в TimescaleDB (primary и две реплики), "
    "Grafana читает базу. Потеря зоны означает переподключение устройств к другим узлам; подтверждённые показания не теряются.",
)

region = a.zone("region", "Регион eu-1", "region")
az = {x: a.zone(f"az-{x}", f"AZ {x.upper()}", "az", region, geo="eu-1", costModel="cloud") for x in "abc"}
az_backup = a.zone("site-backup", "Площадка копий", "cloud", geo="eu-2", costModel="cloud")

a.node("users", "users", "Устройства (MQTT поверх TLS)", rpsLabel="5k сообщений/с пик")
for x, z in az.items():
    a.node(f"lb-{x}", "l4", "L4-балансировщик", z, "lb", replicas=2, healthCheckSeconds=3, failThreshold=2)
    a.node(f"mqtt-{x}", "emqx", "EMQX", z, "mqtt", variant="cluster", replicas=2, vcpu=8, ramGb=16, diskGb=100, encryptionAtRest=True)
    a.node(f"tb-{x}", "thingsboard", "ThingsBoard (транспорт и правила)", z, "tb", replicas=3, vcpu=4, ramGb=16)
    a.node(f"mon-{x}", "monitoring", "Мониторинг", z, "monitoring", replicas=2, vcpu=2, ramGb=8, diskGb=200)
    a.node(f"etcd-{x}", "etcd", "etcd", z, "etcd", "voter", vcpu=2, ramGb=4, diskGb=50)
    a.edge("users", f"lb-{x}", tls=True)
    a.chain(f"lb-{x}", f"mqtt-{x}", f"tb-{x}")
    a.control(f"mon-{x}", f"tb-{x}", tls=False)
a.quorum("etcd-a", "etcd-b", "etcd-c")

# the readings: TimescaleDB with a synchronous replica and an asynchronous one
ts = dict(version="17", vcpu=16, ramGb=64, diskGb=4000, failover="auto", failoverSeconds=30, dcs="etcd", connectionPooler=True)
a.node("ts-a", "timescaledb", "TimescaleDB primary", az["a"], "ts", "primary", **store(**ts))
a.node("ts-b", "timescaledb", "TimescaleDB sync replica", az["b"], "ts", "replica", **store(**ts))
a.node("ts-c", "timescaledb", "TimescaleDB async replica", az["c"], "ts", "replica", **store(**ts))
for x in "abc":
    a.call(f"tb-{x}", f"ts-{x}", 0.2, fanoutColdCache=0.4, tls=True)
a.sync("ts-a", "ts-b")
a.async_("ts-a", "ts-c", lag=1)

# what people see: Grafana reads the database, it is not on the way of a message
a.node("grafana-a", "grafana", "Grafana", az["a"], "grafana", replicas=2, vcpu=2, ramGb=4)
a.node("grafana-c", "grafana", "Grafana", az["c"], "grafana", replicas=2, vcpu=2, ramGb=4)
a.control("grafana-a", "ts-a")
a.control("grafana-c", "ts-c")

# the way of a change, the secrets and the copies
a.node("gitops", "gitops", "GitOps · Argo CD", az["c"], "gitops", replicas=2, vcpu=2, ramGb=4)
for x in "abc":
    a.control("gitops", f"tb-{x}")
a.node("vault-a", "vault", "Vault", az["a"], "vault", "primary", vcpu=2, ramGb=4, diskGb=50)
a.node("vault-b", "vault", "Vault (standby)", az["b"], "vault", "replica", vcpu=2, ramGb=4, diskGb=50)
a.async_("vault-a", "vault-b", lag=1)
a.node("backup", "backup_storage", "Бэкапы (объектное хранилище, object lock)", az_backup, **backup_props(retentionDays=35, restoreSeconds=10800, diskGb=30000))
a.backup("ts-b", "backup", label="WAL + full")
a.backup("vault-a", "backup", every=3600)

TEMPLATE = a
