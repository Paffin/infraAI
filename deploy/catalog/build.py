"""Builds backend/crates/engine/defaults/catalog.json from base.json (the hand-written kinds) and the parts a-d (products,
variants, functions, versions), and puts display names on the release tables of versions.json.

    python3 deploy/catalog/build.py            # write catalog.json, versions.json (names) and the missing IaC templates
    python3 deploy/catalog/build.py --check    # exit 1 if a generated file is out of date (used in CI); writes nothing

The catalog is data and can also be edited in the admin panel; this is how the shipped defaults are produced, so that
a hundred products stay consistent (recommended versions come from the release tables, the same variants are
described once). Edit the parts, run the tool, commit the result. See docs/CATALOG.md.
"""
import copy
import json
import sys

from arch import *
import part_a as A
import part_b as B
import part_c as C
import part_d as D
import iac as IAC
import version_names

CHECK = "--check" in sys.argv
CATALOG = f"{ENGINE}/defaults/catalog.json"
base = json.load(open(f"{HERE}/base.json", encoding="utf-8"))

# ---- categories and functions ---------------------------------------------------------------------------------------
FUNCTIONS = {
    "edge": [
        ("users", "Пользователи и клиенты", "Users and clients"), ("gslb", "Глобальная балансировка (GSLB)", "Global balancing (GSLB)"), ("dns-server", "Серверы DNS", "DNS servers"),
        ("cdn", "CDN", "CDN"), ("http-cache", "HTTP-кэш", "HTTP cache"), ("waf", "WAF", "WAF"), ("l7", "Балансировщики L7 и ingress", "L7 balancers and ingress"),
        ("l4", "Балансировщики L4", "L4 balancers"), ("proxy", "Прокси", "Proxies"), ("firewall", "Межсетевые экраны", "Firewalls"),
        ("network", "Сетевое оборудование", "Network devices"), ("vpn", "VPN и каналы между площадками", "VPN and links between sites")],
    "compute": [
        ("k8s", "Kubernetes", "Kubernetes"), ("orchestrator", "Оркестраторы", "Orchestrators"), ("vm", "Виртуальные машины", "Virtual machines"),
        ("functions", "Функции (serverless)", "Functions (serverless)"), ("api-gateway", "API-шлюзы", "API gateways"), ("data-processing", "Обработка данных", "Data processing")],
    "data": [
        ("rdbms", "Реляционные СУБД", "Relational databases"), ("newsql", "Распределённые SQL", "Distributed SQL"), ("nosql-doc", "Документные базы", "Document databases"),
        ("graph", "Графовые базы", "Graph databases"), ("wide-column", "Колоночные базы (wide-column)", "Wide-column databases"), ("kv", "Ключ-значение", "Key-value"),
        ("timeseries", "Временные ряды", "Time series"), ("cache", "Кэши", "Caches"), ("mq", "Очереди и потоки", "Queues and streams"), ("search", "Поиск", "Search"),
        ("olap", "Аналитические базы", "Analytical databases"), ("object-store", "Объектные хранилища", "Object storage"), ("sds", "Хранилища (SDS, СХД)", "Storage (SDS, arrays)"),
        ("backup", "Резервное копирование", "Backup")],
    "platform": [
        ("consensus", "Консенсус и координация", "Consensus and coordination"), ("secrets", "Секреты", "Secrets"), ("monitoring", "Мониторинг", "Monitoring"),
        ("telemetry", "Сбор телеметрии", "Telemetry collection"), ("logging", "Логи", "Logs"), ("tracing", "Трассировка", "Tracing"), ("dashboards", "Дашборды", "Dashboards"),
        ("gitops", "GitOps", "GitOps"), ("ci", "CI/CD и Git-хостинг", "CI/CD and Git hosting"), ("quality", "Качество кода", "Code quality"), ("registry", "Реестры артефактов", "Artifact registries"),
        ("identity", "Идентификация", "Identity"), ("access", "Доступ к инфраструктуре", "Infrastructure access"), ("pki", "Сертификаты", "Certificates"), ("policy", "Политики", "Policies"),
        ("cni", "Сеть кластера", "Cluster network"), ("mesh", "Сервисная сетка", "Service mesh")],
}
for cat in base["categories"]:
    cat["functions"] = [{"id": i, "name": L(ru, en)} for i, ru, en in FUNCTIONS[cat["id"]]]

# ---- existing kinds -------------------------------------------------------------------------------------------------
VARIANTS = {}
for src in (A.VARIANTS, B.VARIANTS_B, C.VARIANTS_C):
    for kid, vs in src.items():
        VARIANTS.setdefault(kid, []).extend(vs)


DESC = {
    "gslb": ("Глобальная балансировка на уровне DNS: отвечает адресом живой площадки по проверкам состояния. Время переключения = интервал проверки × порог + TTL записи.", "DNS-level global balancing: answers with the address of a live site based on health checks. Failover time = check interval × threshold + the record’s TTL."),
    "cdn": ("Сеть доставки контента: кэш и защита на границе, ближе к пользователю. Отказ источника скрыт, пока кэш свеж.", "A content delivery network: cache and protection at the edge, close to the user. An origin failure is hidden while the cache is fresh."),
    "waf": ("Фильтрация веб-атак перед приложением. Должен стоять на пути трафика от пользователей к приложению и работать парой.", "Web attack filtering in front of the application. It must sit on the path from users to the application and run as a pair."),
    "l7": ("Точка входа и балансировщик HTTP: TLS, маршрутизация по имени и пути, проверки состояния. Обычно пара узлов с плавающим адресом.", "The HTTP entry point and balancer: TLS, routing by host and path, health checks. Usually a pair of nodes with a floating address."),
    "l4": ("Балансировщик TCP/UDP: раскладывает соединения по узлам, не разбирая содержимое. Быстрый и простой, ставится перед L7 или перед сервисами без HTTP.", "A TCP/UDP balancer: spreads connections over nodes without looking at the content. Fast and simple, goes in front of L7 or non-HTTP services."),
    "vpn_dci": ("Канал между площадками (VPN, выделенная линия, оптика). Его отказ — разделение сети; проверяется, что оно не приводит к split-brain.", "A link between sites (VPN, leased line, fibre). Its failure is a network partition; the model checks that it does not cause split-brain."),
    "kubernetes": ("Приложение в кластере Kubernetes: поды с автомасштабированием. Разнесение по зонам и PodDisruptionBudget определяют, переживёт ли оно потерю площадки.", "An application on a Kubernetes cluster: pods with autoscaling. Zone spread and PodDisruptionBudgets decide whether it survives a site loss."),
    "vm_pool": ("Группа виртуальных машин с приложением за балансировщиком. Платформа виртуализации и ОС задаются вариантом и свойствами.", "A group of virtual machines running an application behind a balancer. The virtualization platform and the OS are set by the variant and the properties."),
    "functions": ("Функции как сервис: платформа запускает и масштабирует экземпляры по событиям. Отказоустойчивость обеспечивает платформа.", "Functions as a service: the platform starts and scales instances on events. Resilience is the platform’s."),
    "api_gateway": ("Единая точка входа для API: аутентификация, лимиты, маршрутизация. Один экземпляр — точка отказа всех API.", "A single entry point for APIs: authentication, limits, routing. One instance is a single point of failure for every API."),
    "postgresql": ("Реляционная СУБД. Один основной узел принимает запись, реплики копируют данные; автоматическое переключение требует кворума (DCS или арбитр).", "A relational database. One primary accepts writes, replicas copy the data; automatic failover needs a quorum (a DCS or an arbiter)."),
    "redis": ("Хранилище ключ-значение в памяти: кэш, сессии, очереди. Для кэша допустим холодный старт, для данных нужны репликация и запись на диск.", "An in-memory key-value store: cache, sessions, queues. As a cache a cold start is acceptable; for data replication and persistence are needed."),
    "kafka": ("Журнал сообщений: запись подтверждается минимальным числом синхронных реплик (min.insync.replicas), поэтому кластеру нужно большинство брокеров по одну сторону разрыва.", "A message log: a write is acknowledged by a minimum number of in-sync replicas (min.insync.replicas), so the cluster needs a majority of brokers on one side of a partition."),
    "clickhouse": ("Колоночная аналитическая СУБД. Реплицируемые таблицы согласуются через ClickHouse Keeper; шарды масштабируют запись и хранение.", "A columnar analytical database. Replicated tables agree through ClickHouse Keeper; shards scale writes and storage."),
    "object_storage": ("S3-совместимое хранилище объектов. Надёжность даёт erasure coding или репликация между узлами и, чтобы пережить потерю площадки, между площадками.", "S3-compatible object storage. Durability comes from erasure coding or replication between nodes and, to survive a site loss, between sites."),
    "opensearch": ("Поиск и аналитика по логам и документам. Кластер переживает потерю меньшинства мастер-узлов; чтобы пережить потерю площадки, мастера ставят на три площадки.", "Search and analytics over logs and documents. A cluster survives losing a minority of master nodes; to survive a site loss the masters go on three sites."),
    "backup_storage": ("Хранилище резервных копий вне основных площадок. Копии должны быть неизменяемыми и зашифрованными, а восстановление — проверяться.", "A backup store outside the main sites. Copies must be immutable and encrypted, and restores must be tested."),
    "etcd": ("Распределённое хранилище на консенсусе (Raft). Кластер из 3 или 5 узлов; служит DCS для Patroni и хранилищем Kubernetes.", "A distributed store on consensus (Raft). A cluster of 3 or 5 nodes; serves as the DCS for Patroni and as the Kubernetes store."),
    "vault": ("Хранилище секретов и ключей. Вне пути запросов, если секреты читаются при старте, но без него нельзя развернуть новые экземпляры.", "A store for secrets and keys. Off the request path if secrets are read at start-up, but without it new instances cannot be deployed."),
    "monitoring": ("Сбор метрик и оповещения. Должен жить минимум на двух площадках, иначе при потере площадки вы не узнаете о потере.", "Metrics and alerting. It must live on at least two sites, otherwise when a site is lost you will not learn about it."),
    "gitops": ("Состояние кластера описано в Git, контроллер приводит кластер к нему. Нужен для воспроизводимого восстановления площадки.", "The cluster state is described in Git and a controller reconciles the cluster to it. Needed to rebuild a site reproducibly."),
}

KIND_TABLE = {"monitoring": "prometheus", "gitops": "argo_cd"}          # kind id -> release table when it differs from the id
KIND_VERSION_PROP = {  # existing kinds that had no version prop: (table, help)
    "redis": "redis", "kafka": "kafka", "clickhouse": "clickhouse", "opensearch": "opensearch", "etcd": "etcd", "vault": "vault", "monitoring": "prometheus", "gitops": "argo_cd",
}
SUBTITLES = {"monitoring": ("{variantName}", "{variantName}"), "gitops": ("{variantName}", "{variantName}")}
DCS_VARIANTS = {"postgresql": ["patroni"]}


def insert_after(props, key, new):
    i = next((n for n, p in enumerate(props) if p["key"] == key), len(props) - 1)
    for j, p in enumerate(new):
        props.insert(i + 1 + j, p)


kinds = []
for kd in base["kinds"]:
    kd = copy.deepcopy(kd)
    kid = kd["id"]
    kd["function"] = A.FUNCTIONS[kid]
    if kid in DESC and not kd.get("description"):
        kd["description"] = L(*DESC[kid])
    if kid in KIND_TABLE:
        kd["versions"] = KIND_TABLE[kid]
    if kid in KIND_VERSION_PROP and not any(p["key"] == "version" for p in kd["props"]):
        kd["props"].insert(0, pv(KIND_VERSION_PROP[kid]))
    for p in kd["props"]:
        if p["key"] == "dcs":
            p["refTrait"] = "dcs-capable"
            if kid in DCS_VARIANTS:
                p["variants"] = DCS_VARIANTS[kid]
    if kid == "etcd":
        kd["traits"].append("dcs-capable")
    if kid == "postgresql":
        kd["traits"].append("connection-pooling")
    if kid == "kubernetes":
        kd["props"] += B.runtime_props()
    if kid == "functions":
        kd["props"] += B.runtime_props()
    if kid == "vm_pool":
        kd["props"].insert(0, pv(None, "Версия платформы виртуализации", "Virtualization platform version", help=("Гипервизор или платформа: срок поддержки проверяется по выбранному варианту.", "The hypervisor or platform: its support dates are checked for the chosen variant.")))
        kd["props"] += B.os_props()
    if kid in SUBTITLES:
        kd["subtitle"] = L(*SUBTITLES[kid])
    if kid in VARIANTS:
        kd["variants"] = VARIANTS[kid]
    # keep the key order of the hand-written kinds
    order = ["id", "category", "abbr", "color", "behavior", "traits", "name", "description", "subtitle", "capacity", "cost", "roles", "props", "runbook", "function", "variants", "versions"]
    kd = {key: kd[key] for key in order if key in kd} | {key: v for key, v in kd.items() if key not in order}
    kinds.append(kd)

new_kinds = B.KINDS_EDGE + B.KINDS_COMPUTE + C.KINDS_DATA + D.KINDS_PLATFORM
ids = [x["id"] for x in kinds] + [x["id"] for x in new_kinds]
dup = {i for i in ids if ids.count(i) > 1}
if dup:
    sys.exit(f"duplicate kind ids: {sorted(dup)}")
kinds += new_kinds
base["kinds"] = kinds

# ---- checks ---------------------------------------------------------------------------------------------------------
errs = []
fn_of = {c["id"]: {f["id"] for f in c["functions"]} for c in base["categories"]}
for kd in kinds:
    if kd.get("function") and kd["function"] not in fn_of[kd["category"]]:
        errs.append(f"{kd['id']}: function {kd['function']} not declared in category {kd['category']}")
    if len(kd["abbr"]) > 4:
        errs.append(f"{kd['id']}: abbr {kd['abbr']} longer than 4")
    if sum(1 for v in kd.get("variants", []) if v.get("default")) > 1:
        errs.append(f"{kd['id']}: more than one default variant")
    named = ([kd["versions"]] if kd.get("versions") else []) + [v["versions"] for v in kd.get("variants", []) if v.get("versions")]
    for t in named + ([kd["id"]] if kd["id"] in VERS else []):
        if t not in VERS:
            errs.append(f"{kd['id']}: version table {t!r} does not exist")
        else:
            USED_TABLES.add(t)
for kd in kinds:
    pk = [p["key"] for p in kd["props"]]
    if len(pk) != len(set(pk)):
        errs.append(f"{kd['id']}: duplicate props {pk}")
if errs:
    print("\n".join(errs))
    sys.exit(1)

# ---- write ----------------------------------------------------------------------------------------------------------
catalog_text = json.dumps(base, indent=1, ensure_ascii=False) + "\n"
versions_text, n_tables = version_names.render(VERSIONS_PATH)
stale = []
if CHECK:
    if open(CATALOG, encoding="utf-8").read() != catalog_text:
        stale.append("defaults/catalog.json")
    if open(VERSIONS_PATH, encoding="utf-8").read() != versions_text:
        stale.append("defaults/versions.json")
else:
    open(CATALOG, "w", encoding="utf-8").write(catalog_text)
    open(VERSIONS_PATH, "w", encoding="utf-8").write(versions_text)
missing, include_stale = IAC.ensure(base, write=not CHECK)
if CHECK and missing:
    stale.append("defaults/iac: no template for " + ", ".join(missing))
if CHECK and include_stale:
    stale.append("src/defaults.rs: the list of IaC templates")
if stale:
    print("out of date:\n  " + "\n  ".join(stale) + "\nrun: python3 deploy/catalog/build.py")
    sys.exit(1)
print(f"{len(kinds)} kinds ({len(new_kinds)} new), {sum(len(k.get('variants', [])) for k in kinds)} variants, {sum(len(c['functions']) for c in base['categories'])} functions, {n_tables} release tables" + (" – up to date" if CHECK else f", {len(missing)} IaC templates added"))
unused = sorted(set(VERS) - USED_TABLES)
if unused and not CHECK:
    print("release tables used through `lifecycle` props (@os, @runtime) or not at all:", ", ".join(unused))
