from cloudweb import Spec, build

# A web service on Azure in three availability zones: AKS, Azure Database for PostgreSQL and Azure Cache for Redis in the zones, the regional services around them,
# the copies in a paired region.
TEMPLATE = build(Spec(
    id="azure-three-zones", order=10, badge="Azure", tags=["cloud", "azure", "ha", "kubernetes", "managed"],
    name=("Azure: веб-сервис в трёх зонах", "Azure: a web service across three zones"),
    description=("AKS, Azure Database for PostgreSQL и Azure Cache for Redis в трёх зонах доступности региона, Front Door, WAF, Application Gateway, Blob Storage, Service Bus и Key Vault, копии в парном регионе.",
                 "AKS, Azure Database for PostgreSQL and Azure Cache for Redis in three availability zones of a region, Front Door, WAF, Application Gateway, Blob Storage, Service Bus and Key Vault, copies in the paired region."),
    title="Azure: веб-сервис в трёх зонах доступности",
    about="Управляемые сервисы Azure в одном регионе: приложение в AKS, Azure Database for PostgreSQL с репликами в трёх зонах, Azure Cache for Redis, региональные Application Gateway, WAF, Blob Storage, Service Bus, Key Vault; копии — в парном регионе. "
          "Цель по доступности — 99.7 %: ровно столько дают SLA управляемых сервисов, стоящих друг за другом.",
    rps=4000, budget=2200000, slo=99.7,
    region=("Регион Azure West Europe", "westeurope"), dr_region=("Регион Azure North Europe (копии)", "northeurope"), zone_label="Зона",
    dns=("azure_traffic_manager", "Azure Traffic Manager", dict(healthCheckSeconds=10, failThreshold=3, dnsTtlSeconds=60)),
    cdn=("azure_front_door", "Azure Front Door", {}),
    waf=("azure_waf", "Azure WAF", {}),
    lb=("azure_application_gateway", "Application Gateway", {}),
    k8s=("kubernetes", "AKS", dict(variant="aks")),
    db=("postgresql", "Azure Database for PostgreSQL", dict(variant="azure")),
    cache=("redis", "Azure Cache for Redis", dict(variant="cloud")),
    queue=("azure_service_bus", "Azure Service Bus", {}),
    files=("azure_blob_storage", "Azure Blob Storage · файлы", {}),
    secrets=("azure_key_vault", "Azure Key Vault", {}),
    monitoring=("monitoring", "Azure Monitor", dict(variant="cloud")),
    backup=("backup_storage", "Azure Backup · хранилище копий в North Europe", dict(variant="s3-object-lock")),
    secrets_copy="резервная копия Key Vault + репликация в парный регион",
))
