"""Part D: platform. Coordination, secrets, observability, delivery, identity, cluster add-ons."""
from arch import *

PURPLE = "#7a42d7"
PURPLE2 = "#6d3fc4"
ORANGE = "#c2410c"
BLUE = "#2a64e0"
SLATE = "#4c596c"


def platform_kind(id, fn, abbr, color, ru, en, table, desc, traits, replicas=2, vcpu=2, ram=4, disk=None, variants=None, rep=("Экземпляры", "Instances"), cap=None, behavior="stateless", extra=(), sub="{replicas}× · {version}", runbook=None, versions=None, roles=None):
    props = [pv(table), pr(replicas, *rep)] + list(extra) + res(vcpu, ram, disk)
    return k(id, "platform", fn, abbr, color, ru, en, behavior, ["platform"] + traits, props, desc, sub=sub, cap=cap, variants=variants, versions=versions if versions is not None else (table if table and table != id else None), runbook=runbook, roles=roles)


def vkind(id, ru, en, d_ru, d_en, default=False, **kw):
    return variant(id, ru, en, d_ru, d_en, default=default, **kw)


K8S = {"hosting": "kubernetes"}

KINDS_CONSENSUS = [
    k("consul", "platform", "consensus", "CNS", PURPLE, "Consul", "Consul", "consensus", ["consensus", "platform", "service-discovery", "dcs-capable"],
      [pv("consul"), votes_prop(), num("electionSeconds", "Время выбора лидера", "Leader election time", unit_="s", min=0, default=5, group="failover")] + res(2, 4, 50),
      ("Каталог сервисов и хранилище ключ-значение на Raft. Серверы (3 или 5) выбирают лидера; кластер Consul часто служит DCS для Patroni.", "A service catalog and key-value store on Raft. The servers (3 or 5) elect a leader; a Consul cluster often serves as the DCS for Patroni."),
      sub="{replicas}× · {version}", roles=R_VOTER,
      runbook=rb(failover=[("Проверьте consul operator raft list-peers: есть лидер и большинство серверов.", "Check consul operator raft list-peers: there is a leader and a majority of servers.")], restore=[("Восстановите снимок: consul snapshot restore.", "Restore a snapshot: consul snapshot restore.")])),
    k("zookeeper", "platform", "consensus", "ZK", PURPLE, "Apache ZooKeeper", "Apache ZooKeeper", "consensus", ["consensus", "platform", "dcs-capable"],
      [pv("zookeeper"), votes_prop(), num("electionSeconds", "Время выбора лидера", "Leader election time", unit_="s", min=0, default=5, group="failover")] + res(2, 4, 50),
      ("Координационный сервис с кворумом (ZAB). Ансамбль из 3 или 5 серверов нужен Kafka (старых версий), Solr, HBase, Pulsar и Patroni.", "A coordination service with a quorum (ZAB). An ensemble of 3 or 5 servers is needed by Kafka (older versions), Solr, HBase, Pulsar and Patroni."),
      sub="{replicas}× · {version}", roles=R_VOTER,
      runbook=rb(failover=[("Проверьте echo srvr | nc <узел> 2181 на каждом сервере: один Mode: leader, остальные follower.", "Check echo srvr | nc <node> 2181 on every server: one Mode: leader, the rest follower.")])),
    k("openbao", "platform", "secrets", "OBA", ORANGE, "OpenBao", "OpenBao", "consensus", ["secrets", "stateful", "backup-required", "platform"],
      [pv("openbao"), votes_prop(), num("electionSeconds", "Время выбора лидера", "Leader election time", unit_="s", min=0, default=10, group="failover"), boolean("encryptionAtRest", "Шифрование на диске", "Encryption at rest", group="security", default=True)] + res(2, 4, 50),
      ("Открытый форк Vault под управлением Linux Foundation: совместимое API, встроенный Raft, автоматическое переключение при 3 или 5 узлах.", "The open fork of Vault under the Linux Foundation: a compatible API, an integrated Raft, automatic failover with 3 or 5 nodes."),
      sub="{replicas}× · {version}", roles=R_VOTER),
]

KINDS_OBS = [
    k("zabbix", "platform", "monitoring", "ZBX", PURPLE, "Zabbix", "Zabbix", "primary_replica", ["monitoring", "platform", "stateful"],
      [pv("zabbix"), failover_enum("auto"), failover_secs(60)] + res(4, 16, 200),
      ("Мониторинг с агентами и прокси. Узлы сервера работают в режиме активный/резервный: кто активен, определяет общая база данных, поэтому она должна быть отказоустойчивой.", "Monitoring with agents and proxies. The server nodes run active/standby: a shared database decides who is active, so it must be resilient itself."),
      sub="{version} · {vcpu} vCPU", roles=R_PR(primary="Активный", replica=("Резервный", "Standby")),
      variants=[vkind("ha-cluster", "Кластер active/standby", "Active/standby cluster", "Нативный HA Zabbix: резервный узел принимает работу, когда активный не отвечает.", "Native Zabbix HA: a standby takes over when the active node stops answering.", default=True, add_traits=["platform-quorum"], defaults={"failover": "auto", "failoverSeconds": 60}),
                vkind("single", "Один сервер", "Single server", "Резерв — копия базы и образ машины; простой при отказе.", "The standby is a database backup and a machine image; downtime on failure.", defaults={"failover": "manual"})]),
    platform_kind("icinga", "monitoring", "ICG", PURPLE, "Icinga", "Icinga", "icinga", ("Мониторинг с зонами: два мастера в одной зоне делят конфигурацию, проверки выполняют агенты и сателлиты.", "Monitoring with zones: two masters in one zone share the configuration, checks are executed by agents and satellites."), ["monitoring"], rep=("Мастера", "Masters")),
    platform_kind("checkmk", "monitoring", "CMK", PURPLE, "Checkmk", "Checkmk", "checkmk", ("Мониторинг с распределёнными сайтами: центральный сайт собирает данные удалённых; для высокой доступности сайт ставят в кластер.", "Monitoring with distributed sites: a central site gathers data from remote ones; for high availability a site goes on a cluster."), ["monitoring"], rep=("Сайты", "Sites")),
    platform_kind("telegraf", "telemetry", "TLG", PURPLE, "Telegraf", "Telegraf", "telegraf", ("Агент и сборщик метрик с сотнями плагинов. Узел на схеме — сборщик (gateway), принимающий метрики от агентов и пишущий в хранилище.", "A metrics agent and collector with hundreds of plugins. A node on the diagram is a gateway collector that receives metrics from agents and writes to storage."), ["telemetry"], vcpu=1, ram=2),
    platform_kind("fluent_bit", "telemetry", "FLB", PURPLE, "Fluent Bit", "Fluent Bit", "fluent_bit", ("Лёгкий сборщик логов и метрик. На схеме — агрегатор: принимает записи от агентов на узлах и отправляет в хранилище логов.", "A light log and metrics collector. On the diagram it is an aggregator: it receives records from the agents on hosts and ships them to the log store."), ["telemetry", "logging"], vcpu=1, ram=2),
    platform_kind("otel_collector", "telemetry", "OTL", PURPLE, "OpenTelemetry Collector", "OpenTelemetry Collector", None, ("Приёмник, обработчик и экспортёр трасс, метрик и логов. Работает агентом на узле и шлюзом перед хранилищами.", "A receiver, processor and exporter of traces, metrics and logs. Runs as an agent on a host and as a gateway in front of the stores."), ["telemetry"], vcpu=1, ram=2, sub="{replicas}×"),
    platform_kind("grafana", "dashboards", "GRF", PURPLE, "Grafana", "Grafana", "grafana", ("Панели и оповещения поверх Prometheus, Loki, Elasticsearch и других источников. Состояние лежит в базе данных; реплики без состояния за балансировщиком.", "Dashboards and alerts over Prometheus, Loki, Elasticsearch and other sources. The state lives in a database; stateless replicas behind a balancer."), ["dashboard"],
                  variants=[vkind("self-hosted", "Свой Grafana", "Own Grafana", "Реплики за балансировщиком с общей базой PostgreSQL или MySQL.", "Replicas behind a balancer with a shared PostgreSQL or MySQL database.", default=True),
                            vkind("cloud", "Grafana Cloud", "Grafana Cloud", "Панели и хранилища у провайдера.", "Dashboards and stores at the provider.", add_traits=["managed"], defaults={"hosting": "managed"}, versions="")]),
    platform_kind("loki", "logging", "LOK", PURPLE, "Grafana Loki", "Grafana Loki", "loki", ("Хранилище логов, индексирующее только метки. Принимающие узлы хранят свежие данные в памяти и журнале, остальное — в объектном хранилище.", "A log store that indexes labels only. Ingesters keep fresh data in memory and a WAL, the rest is in object storage."), ["logging"], replicas=3, vcpu=4, ram=8, disk=200, rep=("Узлы", "Nodes")),
    platform_kind("graylog", "logging", "GLG", PURPLE, "Graylog", "Graylog", "graylog", ("Управление логами с поиском и оповещениями. Для работы нужны хранилище OpenSearch и MongoDB для настроек.", "Log management with search and alerts. It needs an OpenSearch store and MongoDB for the settings."), ["logging"], vcpu=4, ram=16, disk=100),
    platform_kind("logstash", "logging", "LSH", PURPLE, "Logstash", "Logstash", "logstash", ("Конвейер приёма, преобразования и отправки логов. Узлы без состояния; надёжность очереди обеспечивает постоянная очередь на диске или Kafka перед ним.", "A pipeline that ingests, transforms and ships logs. Stateless nodes; the queue is made reliable by a persistent disk queue or Kafka in front."), ["logging"], vcpu=4, ram=8, disk=50),
    platform_kind("kibana", "logging", "KIB", PURPLE, "Kibana", "Kibana", "kibana", ("Интерфейс к Elasticsearch: поиск, панели, оповещения. Состояние лежит в самом Elasticsearch, поэтому экземпляры взаимозаменяемы.", "The interface to Elasticsearch: search, dashboards, alerts. The state lives in Elasticsearch itself, so instances are interchangeable."), ["logging", "dashboard"], vcpu=2, ram=4),
    platform_kind("jaeger", "tracing", "JGR", PURPLE, "Jaeger", "Jaeger", "jaeger", ("Распределённая трассировка. Коллекторы без состояния, трассы хранятся в Elasticsearch, OpenSearch или Cassandra.", "Distributed tracing. Stateless collectors, traces are stored in Elasticsearch, OpenSearch or Cassandra."), ["tracing"], vcpu=2, ram=4),
]

KINDS_DELIVERY = [
    platform_kind("jenkins", "ci", "JNK", PURPLE2, "Jenkins", "Jenkins", "jenkins", ("Сервер сборок. Контроллер один: активно-активного режима нет, отказ контроллера останавливает сборки. Защищают копией JENKINS_HOME и холодным резервом; агенты масштабируются отдельно.", "A build server. There is one controller: no active-active mode, a controller failure stops the builds. Protect it with a JENKINS_HOME backup and a cold standby; agents scale separately."), ["ci", "stateful", "backup-required"], replicas=1, vcpu=4, ram=8, disk=200,
                  rep=("Контроллеры", "Controllers")),
    platform_kind("gitlab", "ci", "GL", PURPLE2, "GitLab", "GitLab", "gitlab", ("Git-хостинг, CI/CD и реестры в одном продукте. В эталонных архитектурах приложение, Gitaly (репозитории), PostgreSQL и Redis — отдельные узлы; здесь — слой приложения.", "Git hosting, CI/CD and registries in one product. In the reference architectures the application, Gitaly (repositories), PostgreSQL and Redis are separate nodes; this is the application tier."), ["ci", "scm"], vcpu=8, ram=16, disk=200,
                  variants=[vkind("self-managed", "Свой GitLab", "Self-managed GitLab", "Узлы приложения за балансировщиком; Gitaly, база и кэш — отдельно.", "Application nodes behind a balancer; Gitaly, the database and the cache apart.", default=True),
                            vkind("saas", "GitLab.com / GitLab Dedicated", "GitLab.com / GitLab Dedicated", "Сервис у GitLab; вы отвечаете за раннеры.", "A service at GitLab; the runners are yours.", add_traits=["managed"], defaults={"hosting": "managed"}, versions="")]),
    platform_kind("forgejo", "ci", "FJO", PURPLE2, "Forgejo", "Forgejo", "forgejo", ("Лёгкий Git-хостинг с Actions. Реплики без состояния за балансировщиком; репозитории на общем хранилище, база — PostgreSQL или MySQL.", "A light Git host with Actions. Stateless replicas behind a balancer; repositories on shared storage, the database is PostgreSQL or MySQL."), ["ci", "scm"], vcpu=2, ram=4, disk=100),
    platform_kind("sonarqube", "quality", "SNR", PURPLE2, "SonarQube", "SonarQube", "sonarqube", ("Анализ качества и безопасности кода. Редакция Data Center работает кластером; остальные — одним узлом с базой данных PostgreSQL.", "Code quality and security analysis. The Data Center edition runs as a cluster; the others as a single node with a PostgreSQL database."), ["quality"], replicas=1, vcpu=4, ram=8, disk=100, rep=("Узлы", "Nodes")),
    platform_kind("artifactory", "registry", "ART", PURPLE2, "JFrog Artifactory", "JFrog Artifactory", "artifactory", ("Универсальный реестр артефактов. Высокая доступность — несколько узлов с общей базой и общим хранилищем файлов (Enterprise).", "A universal artifact registry. High availability means several nodes with a shared database and a shared file store (Enterprise)."), ["registry", "stateful", "backup-required"], vcpu=4, ram=8, disk=500),
    platform_kind("nexus", "registry", "NXS", PURPLE2, "Sonatype Nexus Repository", "Sonatype Nexus Repository", "nexus", ("Реестр артефактов и прокси к внешним репозиториям. Высокая доступность — в редакции Pro на Kubernetes с внешней базой и хранилищем.", "An artifact registry and a proxy to external repositories. High availability is in the Pro edition on Kubernetes with an external database and storage."), ["registry", "stateful", "backup-required"], replicas=1, vcpu=4, ram=8, disk=500),
    platform_kind("harbor", "registry", "HRB", PURPLE2, "Harbor", "Harbor", "harbor", ("Реестр образов контейнеров со сканированием уязвимостей и подписями. Компоненты без состояния, база PostgreSQL, Redis и объектное хранилище — отдельные узлы.", "A container image registry with vulnerability scanning and signatures. Stateless components, the PostgreSQL database, Redis and object storage are separate nodes."), ["registry"], vcpu=4, ram=8, disk=100,
                  variants=[vkind("self-hosted", "Свой Harbor", "Own Harbor", "Реплики в Kubernetes с внешними базой, кэшем и хранилищем.", "Replicas on Kubernetes with an external database, cache and storage.", default=True, defaults=K8S)]),
]

KINDS_SECURITY = [
    platform_kind("keycloak", "identity", "KC", SLATE, "Keycloak", "Keycloak", "keycloak", ("Единый вход и управление доступом (OIDC, SAML). Узлы без состояния делят кэш сессий; пользователи лежат в PostgreSQL. Вход недоступен — недоступны все приложения за ним.", "Single sign-on and access management (OIDC, SAML). Stateless nodes share a session cache; users live in PostgreSQL. When login is down every application behind it is down."), ["identity"], replicas=3, vcpu=4, ram=8,
                  variants=[vkind("single-site", "Кластер на одной площадке", "Cluster on one site", "Узлы делят кэш Infinispan; база — PostgreSQL.", "Nodes share an Infinispan cache; the database is PostgreSQL.", default=True),
                            vkind("multi-site", "Несколько площадок (active/active)", "Several sites (active/active)", "Отдельные кластеры на площадках с внешним Infinispan и синхронной репликацией базы.", "Separate clusters on sites with an external Infinispan and synchronous database replication.")]),
    platform_kind("authentik", "identity", "ATK", SLATE, "authentik", "authentik", "authentik", ("Провайдер идентификации (OIDC, SAML, LDAP) с настраиваемыми потоками входа. Сервер и воркеры без состояния; база PostgreSQL и Redis — отдельно.", "An identity provider (OIDC, SAML, LDAP) with configurable login flows. Stateless server and workers; the PostgreSQL database and Redis apart."), ["identity"], vcpu=2, ram=4),
    platform_kind("teleport", "access", "TLP", SLATE, "Teleport", "Teleport", "teleport", ("Доступ к серверам, базам и Kubernetes по сертификатам с записью сессий. Прокси и сервис авторизации масштабируются, состояние лежит во внешнем бэкенде.", "Certificate-based access to servers, databases and Kubernetes with session recording. Proxy and auth services scale out, the state lives in an external backend."), ["access"], vcpu=2, ram=4,
                  variants=[vkind("self-hosted", "Свой кластер", "Own cluster", "Прокси и сервис авторизации в нескольких зонах, бэкенд — etcd или PostgreSQL.", "Proxy and auth services in several zones, the backend is etcd or PostgreSQL.", default=True),
                            vkind("cloud", "Teleport Enterprise Cloud", "Teleport Enterprise Cloud", "Управляющий слой у Teleport.", "The control plane at Teleport.", add_traits=["managed"], defaults={"hosting": "managed"}, versions="")]),
    platform_kind("boundary", "access", "BND", SLATE, "HashiCorp Boundary", "HashiCorp Boundary", "boundary", ("Доступ к ресурсам по ролям без выдачи сетевого доступа и учётных данных. Контроллеры хранят состояние в PostgreSQL, воркеры подключаются к целям.", "Role-based access to resources without handing out network access or credentials. Controllers keep the state in PostgreSQL, workers connect to the targets."), ["access"], vcpu=2, ram=4),
    platform_kind("cert_manager", "pki", "CM", SLATE, "cert-manager", "cert-manager", "cert_manager", ("Выпуск и продление сертификатов в Kubernetes (ACME, Vault, внутренний CA). Реплики выбирают лидера; простой контроллера сказывается, только когда подходит срок продления.", "Issues and renews certificates on Kubernetes (ACME, Vault, an internal CA). Replicas elect a leader; a controller outage matters only when a renewal is due."), ["pki"], replicas=2, vcpu=0.5, ram=0.5, variants=[]),
    platform_kind("kyverno", "policy", "KYV", SLATE, "Kyverno", "Kyverno", "kyverno", ("Политики для Kubernetes: проверка, изменение и генерация ресурсов. При недоступности контроллера допуска запросы либо отклоняются, либо пропускаются — зависит от failurePolicy.", "Policies for Kubernetes: validate, mutate and generate resources. When the admission controller is unavailable requests are either rejected or let through, depending on failurePolicy."), ["policy"], replicas=3, vcpu=1, ram=1),
    platform_kind("gatekeeper", "policy", "GK", SLATE, "OPA Gatekeeper", "OPA Gatekeeper", "gatekeeper", ("Политики Open Policy Agent как контроллер допуска Kubernetes. Несколько реплик; failurePolicy определяет, что делать при его недоступности.", "Open Policy Agent policies as a Kubernetes admission controller. Several replicas; failurePolicy decides what to do when it is unavailable."), ["policy"], replicas=3, vcpu=1, ram=1),
]

KINDS_NETPLAT = [
    k("cni", "platform", "cni", "CNI", BLUE, "Сеть кластера (CNI)", "Cluster network (CNI)", "stateless", ["platform", "network"],
      [pv(None, "Версия", "Version")],
      ("Сетевой плагин Kubernetes: адреса подов, сетевые политики, иногда балансировка и шифрование. Отказ плагина на узле отрезает его поды от сети.", "The Kubernetes network plugin: pod addresses, network policies, sometimes balancing and encryption. A plugin failure on a node cuts its pods off the network."),
      sub="{version}", versions="", variants=[
          vkind("cilium", "Cilium (eBPF)", "Cilium (eBPF)", "Сеть, политики и наблюдаемость на eBPF; может заменить kube-proxy.", "Networking, policies and observability on eBPF; can replace kube-proxy.", default=True, versions="cilium", defaults=K8S),
          vkind("calico", "Calico", "Calico", "Маршрутизация BGP или оверлей, сетевые политики.", "BGP routing or an overlay, network policies.", versions="calico", defaults=K8S),
          vkind("flannel", "Flannel", "Flannel", "Простой оверлей без политик.", "A simple overlay without policies.", versions="", defaults=K8S),
          vkind("cloud-cni", "CNI провайдера (VPC CNI, Azure CNI)", "Provider CNI (VPC CNI, Azure CNI)", "Поды получают адреса сети провайдера.", "Pods get addresses from the provider’s network.", add_traits=["managed"], versions="", defaults=K8S)]),
    k("service_mesh", "platform", "mesh", "MSH", BLUE, "Сервисная сетка", "Service mesh", "stateless", ["platform", "network", "service-mesh"],
      [pv(None, "Версия", "Version"), pr(2, "Экземпляры управляющего слоя", "Control plane instances")] + res(1, 2),
      ("Взаимная аутентификация, повторы и политики трафика между сервисами. Управляющий слой должен быть в нескольких экземплярах; уже запущенные прокси продолжают работать без него.", "Mutual authentication, retries and traffic policy between services. The control plane should run in several instances; proxies already running keep working without it."),
      sub="{replicas}× · {version}", versions="", variants=[
          vkind("istio", "Istio", "Istio", "Универсальная сетка на Envoy с sidecar или ambient-режимом.", "A universal mesh on Envoy with sidecar or ambient mode.", default=True, versions="istio", defaults=K8S),
          vkind("linkerd", "Linkerd", "Linkerd", "Лёгкая сетка со своими прокси на Rust.", "A light mesh with its own Rust proxies.", versions="", defaults=K8S),
          vkind("cilium-mesh", "Cilium Service Mesh", "Cilium Service Mesh", "Сетка без sidecar на eBPF и Envoy на узле.", "A sidecar-free mesh on eBPF and a per-node Envoy.", versions="cilium", defaults=K8S),
          vkind("consul-connect", "Consul Connect", "Consul Connect", "Сетка на Consul с прокси Envoy.", "A mesh on Consul with Envoy proxies.", versions="consul")]),
]

KINDS_PLATFORM = KINDS_CONSENSUS + KINDS_OBS + KINDS_DELIVERY + KINDS_SECURITY + KINDS_NETPLAT
