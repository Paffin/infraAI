from cloudweb import Spec, build

# A web service on Yandex Cloud in three availability zones: Managed Kubernetes, Managed PostgreSQL and Managed Redis in the zones, the regional services around them,
# the copies in another cloud.
TEMPLATE = build(Spec(
    id="yandex-cloud-three-zones", order=12, badge="YC", tags=["cloud", "yandex", "ha", "kubernetes", "managed", "ru"],
    name=("Yandex Cloud: веб-сервис в трёх зонах", "Yandex Cloud: a web service across three zones"),
    description=("Managed Kubernetes, Managed PostgreSQL и Managed Redis в трёх зонах доступности, Cloud CDN, Smart Web Security, Application Load Balancer, Object Storage, Message Queue и Lockbox; копии — в другом облаке.",
                 "Managed Kubernetes, Managed PostgreSQL and Managed Redis in three availability zones, Cloud CDN, Smart Web Security, Application Load Balancer, Object Storage, Message Queue and Lockbox; the copies are in another cloud."),
    title="Yandex Cloud: веб-сервис в трёх зонах доступности",
    about="Управляемые сервисы Yandex Cloud в регионе ru-central1: приложение в Managed Kubernetes, Managed PostgreSQL с репликами в трёх зонах, Managed Redis, региональные Application Load Balancer, "
          "Smart Web Security, Object Storage, Message Queue, Lockbox; копии лежат у другого поставщика (Selectel), вне домена отказа облака. Цель по доступности — 99.7 %: ровно столько дают SLA управляемых сервисов, стоящих друг за другом.",
    rps=4000, budget=2200000, slo=99.7,
    region=("Регион Yandex Cloud ru-central1", "ru-central1"), dr_region=("Selectel · Санкт-Петербург (копии)", "selectel-spb"), zone_label="Зона",
    cdn=("yandex_cdn", "Yandex Cloud CDN", {}),
    waf=("yandex_smart_web_security", "Smart Web Security", {}),
    lb=("yandex_alb", "Application Load Balancer", dict(healthCheckSeconds=5, failThreshold=2)),
    k8s=("yandex_managed_kubernetes", "Managed Kubernetes", {}),
    db=("postgresql", "Managed Service for PostgreSQL", dict(variant="cloud")),
    cache=("redis", "Managed Service for Redis", dict(variant="cloud")),
    queue=("yandex_message_queue", "Yandex Message Queue", {}),
    files=("yandex_object_storage", "Yandex Object Storage · файлы", {}),
    secrets=("yandex_lockbox", "Yandex Lockbox", {}),
    monitoring=("yandex_monitoring", "Yandex Monitoring", {}),
    backup=("backup_storage", "Хранилище копий (Selectel S3, object lock)", dict(variant="s3-object-lock")),
    secrets_copy="выгрузка секретов в зашифрованном виде",
))
