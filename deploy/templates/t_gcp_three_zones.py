from cloudweb import Spec, build

# A web service on Google Cloud in three zones of a region: GKE, Cloud SQL for PostgreSQL and Memorystore in the zones, the regional services around them,
# the copies in another region.
TEMPLATE = build(Spec(
    id="gcp-three-zones", order=11, badge="GCP", tags=["cloud", "gcp", "ha", "kubernetes", "managed"],
    name=("Google Cloud: веб-сервис в трёх зонах", "Google Cloud: a web service across three zones"),
    description=("GKE, Cloud SQL for PostgreSQL и Memorystore в трёх зонах региона, Cloud DNS, Cloud CDN, Cloud Armor, глобальный балансировщик, Cloud Storage, Pub/Sub и Secret Manager, копии в другом регионе.",
                 "GKE, Cloud SQL for PostgreSQL and Memorystore in three zones of a region, Cloud DNS, Cloud CDN, Cloud Armor, the global load balancer, Cloud Storage, Pub/Sub and Secret Manager, copies in another region."),
    title="Google Cloud: веб-сервис в трёх зонах",
    about="Управляемые сервисы Google Cloud в одном регионе: приложение в GKE, Cloud SQL с репликами в трёх зонах, Memorystore, глобальные балансировщик, Cloud Armor, Cloud Storage, Pub/Sub, Secret Manager; копии — в другом регионе. "
          "Цель по доступности — 99.7 %: ровно столько дают SLA управляемых сервисов, стоящих друг за другом.",
    rps=4000, budget=2200000, slo=99.7,
    region=("Регион Google Cloud europe-west4", "europe-west4"), dr_region=("Регион Google Cloud europe-west1 (копии)", "europe-west1"), zone_label="Зона",
    dns=("gslb", "Cloud DNS", dict(variant="cloud-dns", healthCheckSeconds=10, failThreshold=3, dnsTtlSeconds=60)),
    cdn=("google_cloud_cdn", "Cloud CDN", {}),
    waf=("google_cloud_armor", "Cloud Armor", {}),
    lb=("gcp_https_lb", "Cloud Load Balancing (HTTPS)", {}),
    k8s=("kubernetes", "GKE", dict(variant="gke")),
    db=("postgresql", "Cloud SQL for PostgreSQL", dict(variant="cloud")),
    cache=("redis", "Memorystore for Redis", dict(variant="cloud")),
    queue=("google_pubsub", "Pub/Sub", {}),
    files=("google_cloud_storage", "Cloud Storage · файлы", {}),
    secrets=("gcp_secret_manager", "Secret Manager", {}),
    monitoring=("monitoring", "Cloud Monitoring", dict(variant="cloud")),
    backup=("backup_storage", "Backup and DR · хранилище копий в europe-west1", dict(variant="s3-object-lock")),
    secrets_copy="реплика секретов в другом регионе + выгрузка",
))
