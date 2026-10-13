"""Part A: functions and variants of the existing kinds."""
from lib import *

FUNCTIONS = {
    "users": "users", "gslb": "gslb", "cdn": "cdn", "waf": "waf", "l7": "l7", "l4": "l4", "vpn_dci": "vpn",
    "kubernetes": "k8s", "vm_pool": "vm", "functions": "functions", "api_gateway": "api-gateway",
    "postgresql": "rdbms", "redis": "cache", "kafka": "mq", "clickhouse": "olap", "object_storage": "object-store",
    "opensearch": "search", "backup_storage": "backup", "etcd": "consensus", "vault": "secrets", "monitoring": "monitoring", "gitops": "gitops",
}

# version tables of the existing kinds (the kind id is the table id unless named here)
VERSIONS = {"kafka": "kafka", "vault": "vault"}

VARIANTS = {
    "postgresql": [
        variant("patroni", "Patroni + etcd/Consul", "Patroni + etcd/Consul", "Автоматическое переключение лидера через DCS (etcd, Consul, ZooKeeper). Де-факто стандарт для PostgreSQL на своих серверах.",
                "Automatic leader election through a DCS (etcd, Consul, ZooKeeper). The de-facto standard for self-hosted PostgreSQL.",
                default=True, defaults={"failover": "auto", "failoverSeconds": 30}),
        variant("repmgr", "repmgr + witness", "repmgr + witness", "Автоматическое переключение по голосам узлов и witness, без отдельного DCS. Медленнее Patroni и чувствительнее к разделению сети.",
                "Automatic failover by the votes of the nodes and a witness, no separate DCS. Slower than Patroni and more sensitive to partitions.",
                defaults={"failover": "auto", "failoverSeconds": 60}),
        variant("streaming", "Потоковая репликация, ручное переключение", "Streaming replication, manual failover", "Реплики по WAL, лидера повышает оператор. Просто и предсказуемо, но RTO определяется человеком.",
                "WAL streaming replicas, an operator promotes the leader. Simple and predictable, but the RTO is a human’s.",
                defaults={"failover": "manual"}),
        variant("cloudnativepg", "CloudNativePG в Kubernetes", "CloudNativePG on Kubernetes", "Оператор в Kubernetes: лидера выбирает через API Kubernetes, резервные копии — в объектное хранилище.",
                "A Kubernetes operator: the leader is elected through the Kubernetes API, backups go to object storage.",
                add_traits=["platform-quorum"], defaults={"failover": "auto", "failoverSeconds": 30, "hosting": "kubernetes"}),
        managed("rds", "Amazon RDS Multi-AZ", "Amazon RDS Multi-AZ", versions="amazon-rds-postgresql", secs=90, mult=1.6),
        managed("aurora", "Amazon Aurora PostgreSQL", "Amazon Aurora PostgreSQL", versions="amazon-aurora-postgresql", secs=30, mult=1.9,
                desc_ru="Общее хранилище на 6 копий в 3 зонах, быстрое переключение; цена выше RDS.", desc_en="Shared storage with 6 copies across 3 zones, fast failover; priced above RDS."),
        managed("azure", "Azure Database for PostgreSQL", "Azure Database for PostgreSQL", versions="azure-database-for-postgresql", secs=60, mult=1.7),
        managed("cloud", "Другой облачный PostgreSQL", "Other managed PostgreSQL", secs=60, mult=1.6,
                desc_ru="Cloud SQL, Yandex Managed Service, Selectel и аналоги: переключение и копии на стороне провайдера.", desc_en="Cloud SQL, Yandex Managed Service and the like: failover and backups on the provider’s side."),
    ],
    "redis": [
        variant("standalone", "Один узел", "Standalone", "Без репликации и переключения: допустимо для кэша, не для данных.", "No replication or failover: fine for a cache, not for data.",
                default=True, defaults={"mode": "standalone", "failover": "none"}),
        variant("sentinel", "Sentinel (master + реплики)", "Sentinel (master + replicas)", "Мастер, реплики и три Sentinel: автоматическое переключение, один набор данных.",
                "A master, replicas and three Sentinels: automatic failover, one data set.", add_traits=["self-quorum"], defaults={"mode": "sentinel", "failover": "auto", "failoverSeconds": 15}),
        variant("cluster", "Redis Cluster (шарды)", "Redis Cluster (shards)", "Данные делятся на 16384 слота между мастерами, у каждого есть реплика; переключение встроено.",
                "Data is split into 16384 slots between masters, each with a replica; failover is built in.", defaults={"mode": "cluster", "failover": "auto", "failoverSeconds": 15}, add_traits=["self-quorum"]),
        managed("elasticache", "Amazon ElastiCache", "Amazon ElastiCache", versions="amazon-elasticache-redis", secs=30, mult=1.5, extra={"mode": "cluster"}),
        managed("cloud", "Другой облачный Redis", "Other managed Redis", secs=30, mult=1.5, extra={"mode": "sentinel"}),
    ],
    "kafka": [
        variant("kraft", "KRaft (без ZooKeeper)", "KRaft (no ZooKeeper)", "Кворум контроллеров встроен в кластер: проще эксплуатация, быстрые выборы.",
                "The controller quorum is part of the cluster: simpler operations, fast elections.", default=True, defaults={"electionSeconds": 5}),
        variant("zookeeper", "С ZooKeeper (устаревает)", "With ZooKeeper (legacy)", "Метаданные в отдельном ансамбле ZooKeeper; поддержка снимается в новых версиях.",
                "Metadata in a separate ZooKeeper ensemble; support is removed in newer releases.", defaults={"electionSeconds": 15}),
        managed("msk", "Amazon MSK", "Amazon MSK", versions="amazon-msk", secs=None, mult=1.8),
        managed("confluent", "Confluent Cloud / другой облачный Kafka", "Confluent Cloud / other managed Kafka", secs=None, mult=2.2),
    ],
    "clickhouse": [
        variant("replicated", "Реплики (ClickHouse Keeper)", "Replicas (ClickHouse Keeper)", "Реплицируемые таблицы, согласие через ClickHouse Keeper.", "Replicated tables with consensus through ClickHouse Keeper.", default=True),
        variant("sharded", "Шарды + реплики", "Shards + replicas", "Данные делятся на шарды, каждый шард реплицирован; нужна ёмкость на каждый шард.", "Data is split into shards, every shard replicated; capacity per shard is needed."),
        managed("cloud", "ClickHouse Cloud / другой управляемый", "ClickHouse Cloud / other managed", secs=None, mult=1.9),
    ],
    "opensearch": [
        variant("dedicated", "Выделенные мастер-узлы", "Dedicated master nodes", "Три мастер-узла отдельно от data-узлов: кластер устойчив к потере данных-узлов.", "Three master nodes apart from the data nodes: the cluster survives losing data nodes.", default=True),
        variant("all-roles", "Совмещённые роли", "All roles on every node", "Каждый узел и master, и data: дешевле, но потеря узла задевает оба слоя.", "Every node is master and data: cheaper, but losing a node hits both layers."),
        managed("amazon", "Amazon OpenSearch Service", "Amazon OpenSearch Service", versions="amazon-opensearch", secs=None, mult=1.6),
    ],
    "kubernetes": [
        variant("self-managed", "Свой кластер (kubeadm / RKE2 / k0s)", "Self-managed (kubeadm / RKE2 / k0s)", "Управляющий слой на своих серверах: вы отвечаете за etcd, обновления и сертификаты.", "The control plane on your own servers: etcd, upgrades and certificates are yours.", default=True),
        variant("openshift", "Red Hat OpenShift", "Red Hat OpenShift", "Дистрибутив Kubernetes с поддержкой вендора, оператором обновлений и встроенным реестром.", "A vendor-supported Kubernetes distribution with an upgrade operator and a built-in registry.", versions="openshift", mult=1.4),
        variant("rancher", "Rancher (RKE2)", "Rancher (RKE2)", "Кластеры под управлением Rancher: единая консоль, обновления по цепочке.", "Clusters managed by Rancher: one console, chained upgrades.", versions="rancher"),
        managed("eks", "Amazon EKS", "Amazon EKS", versions="amazon-eks", secs=None, mult=1.1, extra={"failover": None},
                desc_ru="Управляющий слой у провайдера, рабочие узлы ваши; версия живёт короче, чем в апстриме.", desc_en="The provider runs the control plane, the worker nodes are yours; versions live shorter than upstream."),
        managed("gke", "Google GKE", "Google GKE", versions="google-kubernetes-engine", secs=None, mult=1.1, extra={"failover": None}),
        managed("aks", "Azure AKS", "Azure AKS", versions="azure-kubernetes-service", secs=None, mult=1.1, extra={"failover": None}),
    ],
    "etcd": [
        variant("dedicated", "Выделенный кластер", "Dedicated cluster", "Отдельные узлы etcd: потеря узла приложений не задевает кворум.", "Separate etcd nodes: losing an application node does not touch the quorum.", default=True),
        variant("stacked", "На узлах control-plane Kubernetes", "Stacked on Kubernetes control-plane nodes", "etcd рядом с API-сервером (kubeadm stacked): меньше узлов, общая судьба с control-plane.", "etcd next to the API server (kubeadm stacked): fewer nodes, a shared fate with the control plane.", defaults={"electionSeconds": 10}),
    ],
    "vault": [
        variant("replication", "Репликация (DR / performance)", "Replication (DR / performance)", "Основной кластер и вторичный в другой площадке, переключение вручную.", "A primary cluster and a secondary on another site, failover by hand.", default=True),
        variant("raft", "Встроенный Raft (HA-кластер)", "Integrated Raft (HA cluster)", "Три-пять узлов с лидером по Raft: автоматическое переключение без внешнего DCS.", "Three to five nodes with a Raft leader: automatic failover without an external DCS.", behavior="consensus", roles=ROLES_CONSENSUS, defaults={"electionSeconds": 10}),
        managed("hcp", "HCP Vault / облачный сервис секретов", "HCP Vault / managed secrets service", secs=None, mult=2.0),
    ],
    "object_storage": [
        variant("minio", "MinIO (distributed, erasure coding)", "MinIO (distributed, erasure coding)", "S3-совместимое хранилище на своих серверах; данные защищены erasure coding в пределах пула.", "S3-compatible storage on your own servers; data is protected by erasure coding within a pool.", default=True),
        variant("ceph-rgw", "Ceph RGW", "Ceph RGW", "S3-шлюз поверх кластера Ceph: репликация и multisite средствами Ceph.", "An S3 gateway over a Ceph cluster: replication and multisite through Ceph."),
        managed("cloud", "Облачное S3 (AWS S3, Yandex Object Storage…)", "Cloud S3 (AWS S3, Yandex Object Storage…)", secs=None, mult=1.0,
                desc_ru="Надёжность и репликация между зонами — на стороне провайдера.", desc_en="Durability and cross-zone replication are the provider’s."),
    ],
    "monitoring": [
        variant("prometheus", "Prometheus HA (пара)", "Prometheus HA (pair)", "Две независимые копии Prometheus собирают одно и то же; Alertmanager в кластере.", "Two independent Prometheus copies scrape the same targets; Alertmanager is clustered.", default=True, versions="prometheus"),
        variant("victoria", "VictoriaMetrics (кластер)", "VictoriaMetrics (cluster)", "Кластер с отдельными vminsert / vmselect / vmstorage: дешевле хранение, горизонтальная запись.", "A cluster with separate vminsert / vmselect / vmstorage: cheaper storage, horizontal writes.", versions=""),
        managed("cloud", "Облачный мониторинг (Managed Prometheus, Grafana Cloud)", "Cloud monitoring (Managed Prometheus, Grafana Cloud)", secs=None, mult=1.5),
    ],
    "gitops": [
        variant("argocd", "Argo CD", "Argo CD", "Контроллер в кластере тянет состояние из Git и приводит кластер к нему.", "An in-cluster controller pulls the state from Git and reconciles the cluster to it.", default=True, versions="argo_cd"),
        variant("flux", "Flux", "Flux", "Набор контроллеров GitOps Toolkit; манифесты и Helm из Git или OCI.", "The GitOps Toolkit controllers; manifests and Helm from Git or OCI.", versions="flux"),
    ],
}

