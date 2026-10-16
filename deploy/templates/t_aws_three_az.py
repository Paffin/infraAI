from cloudweb import Spec, build

# A web service on AWS in three availability zones: EKS, Aurora PostgreSQL and ElastiCache in the zones, the regional services around them, the copies in a second region.
TEMPLATE = build(Spec(
    id="aws-three-az", order=9, badge="AWS", tags=["cloud", "aws", "ha", "kubernetes", "managed"],
    name=("AWS: веб-сервис в трёх зонах", "AWS: a web service across three zones"),
    description=("EKS, Aurora PostgreSQL и ElastiCache в трёх AZ региона, региональные CloudFront, WAF, ALB, S3, SQS и Secrets Manager, копии в другом регионе. Переживает потерю зоны без участия человека.",
                 "EKS, Aurora PostgreSQL and ElastiCache in three AZs of a region, regional CloudFront, WAF, ALB, S3, SQS and Secrets Manager, copies in another region. Survives the loss of a zone with nobody touching it."),
    title="AWS: веб-сервис в трёх зонах доступности",
    about="Управляемые сервисы AWS в одном регионе: приложение в EKS, Aurora PostgreSQL с репликами в трёх AZ, ElastiCache, региональные балансировщик, WAF, S3, SQS, Secrets Manager; копии — в AWS Backup другого региона. "
          "Цель по доступности — 99.7 %: ровно столько дают SLA управляемых сервисов, стоящих друг за другом.",
    rps=4000, budget=2200000, slo=99.7,
    region=("Регион AWS eu-central-1", "eu-central-1"), dr_region=("Регион AWS eu-west-1 (копии)", "eu-west-1"), zone_label="AZ",
    dns=("gslb", "Amazon Route 53", dict(variant="route53", healthCheckSeconds=10, failThreshold=3, dnsTtlSeconds=60)),
    cdn=("cdn", "Amazon CloudFront", dict(variant="cloudfront")),
    waf=("waf", "AWS WAF", dict(variant="cloud")),
    lb=("l7", "Application Load Balancer", dict(variant="cloud-alb")),
    k8s=("kubernetes", "EKS", dict(variant="eks")),
    db=("postgresql", "Aurora PostgreSQL", dict(variant="aurora")),
    cache=("redis", "ElastiCache", dict(variant="elasticache")),
    queue=("amazon_sqs", "Amazon SQS", {}),
    files=("amazon_s3", "Amazon S3 · файлы", {}),
    secrets=("aws_secrets_manager", "AWS Secrets Manager", {}),
    monitoring=("monitoring", "Amazon CloudWatch", dict(variant="cloud")),
    backup=("aws_backup", "AWS Backup · хранилище копий в eu-west-1", {}),
    secrets_copy="реплика в eu-west-1 + выгрузка",
))
