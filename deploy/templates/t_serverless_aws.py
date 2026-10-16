from lib import Arch, backup_props, requirements, store

# A serverless back end on AWS: nothing to size and nothing to patch, so the design is the chain of what the provider runs and the promise it makes. The availability is the product
# of their SLAs, and the copies are the part the provider does not do for you: DynamoDB and S3 are copied into the vault of another region.
a = Arch(
    "serverless-aws", ("Serverless на AWS: API, функции, DynamoDB", "Serverless on AWS: API, functions, DynamoDB"),
    ("CloudFront, WAF, API Gateway, Lambda, DynamoDB, S3, SQS и Cognito: ни серверов, ни кластеров. Копии данных — в хранилище AWS Backup другого региона.",
     "CloudFront, WAF, API Gateway, Lambda, DynamoDB, S3, SQS and Cognito: no servers, no clusters. The copies of the data go to the AWS Backup vault of another region."),
    "AWS", 17, ["cloud", "aws", "serverless", "managed", "startup"],
    requirements(1500, 100, 900, 60, 600000, "site", 99.5),
    "Serverless на AWS: API, функции, DynamoDB",
    "Бессерверная архитектура: запрос проходит CloudFront, WAF и API Gateway, обрабатывается функциями Lambda, данные — в DynamoDB и S3, фоновые задачи — через SQS, пользователи — в Cognito. Каждый из сервисов региональный, "
    "поэтому потеря зоны не заметна; доступность сервиса — произведение SLA звеньев цепочки, а копии данных в другом регионе делает сама схема.",
)

region_dr = a.zone("region-dr", "Регион AWS eu-west-1 (копии)", "region")
az_dr = a.zone("dr-a", "Копии · AZ A", "az", region_dr, geo="eu-west-1", costModel="cloud")

a.node("users", "users", "Пользователи", rpsLabel="1.5k RPS пик")
a.node("cdn", "cdn", "Amazon CloudFront", group="cdn", variant="cloudfront", vcpu=1, ramGb=2)
a.node("waf", "waf", "AWS WAF", group="waf", variant="cloud", vcpu=1, ramGb=2)
a.node("api", "aws_api_gateway", "Amazon API Gateway", vcpu=1, ramGb=2)
a.node("auth", "amazon_cognito", "Amazon Cognito", vcpu=1, ramGb=2)
a.node("fn", "aws_lambda", "AWS Lambda · обработчики", vcpu=2, ramGb=4)
a.node("fn-worker", "aws_lambda", "AWS Lambda · фоновые задачи", vcpu=1, ramGb=2)
a.node("queue", "amazon_sqs", "Amazon SQS", **store(vcpu=1, ramGb=2))
a.node("db", "amazon_dynamodb", "Amazon DynamoDB", **store(vcpu=4, ramGb=16, diskGb=500))
a.node("files", "amazon_s3", "Amazon S3 · файлы", **store(vcpu=2, ramGb=4, diskGb=2000))
a.node("secrets", "aws_secrets_manager", "AWS Secrets Manager", vcpu=0.5, ramGb=1)
a.node("mon", "monitoring", "Amazon CloudWatch", group="monitoring", variant="cloud", vcpu=2, ramGb=8)
a.chain("users", "cdn", "waf", "api")
a.call("api", "auth", 0.1, tls=True)
a.call("api", "fn", 1, tls=True)
a.call("fn", "db", 1, tls=True)
a.call("fn", "files", 0.1, tls=True)
a.call("fn", "queue", 0.2, tls=True)
a.call("queue", "fn-worker", 1, tls=True)
a.call("fn-worker", "db", 0.2, tls=True)
a.control("mon", "fn", tls=False)

# what the provider does not do for you: copies in another region, locked and tested
a.node("backup", "aws_backup", "AWS Backup · хранилище копий в eu-west-1", az_dr, **backup_props(restoreSeconds=1800, vcpu=1, ramGb=2))
a.backup("db", "backup", every=3600, label="PITR + снимки")
a.backup("files", "backup", every=3600)
a.backup("secrets", "backup", every=3600, label="реплика в eu-west-1 + выгрузка")

TEMPLATE = a
