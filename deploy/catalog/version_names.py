"""Display names of the release tables, and the list of tables nobody uses."""
import json
NAMES = {
 "activemq": "Apache ActiveMQ Classic", "airflow": "Apache Airflow", "almalinux": "AlmaLinux", "alpine": "Alpine Linux", "amazon-aurora-mysql": "Amazon Aurora MySQL",
 "amazon-aurora-postgresql": "Amazon Aurora PostgreSQL", "amazon-documentdb": "Amazon DocumentDB", "amazon-eks": "Amazon EKS", "amazon-elasticache-redis": "Amazon ElastiCache for Redis",
 "amazon-linux": "Amazon Linux", "amazon-mq-activemq": "Amazon MQ for ActiveMQ", "amazon-mq-rabbitmq": "Amazon MQ for RabbitMQ", "amazon-msk": "Amazon MSK",
 "amazon-opensearch": "Amazon OpenSearch Service", "amazon-rds-mariadb": "Amazon RDS for MariaDB", "amazon-rds-mysql": "Amazon RDS for MySQL", "amazon-rds-postgresql": "Amazon RDS for PostgreSQL",
 "apache": "Apache HTTP Server", "apisix": "Apache APISIX", "arangodb": "ArangoDB", "argo_cd": "Argo CD", "artemis": "Apache ActiveMQ Artemis", "artifactory": "JFrog Artifactory",
 "authentik": "authentik", "azure-database-for-mysql": "Azure Database for MySQL", "azure-database-for-postgresql": "Azure Database for PostgreSQL", "azure-kubernetes-service": "Azure Kubernetes Service",
 "bind": "BIND 9", "boundary": "HashiCorp Boundary", "caddy": "Caddy", "calico": "Calico", "cassandra": "Apache Cassandra", "ceph": "Ceph", "cert_manager": "cert-manager", "checkmk": "Checkmk",
 "cilium": "Cilium", "cisco_ios_xe": "Cisco IOS XE", "clickhouse": "ClickHouse", "cockroachdb": "CockroachDB", "commvault": "Commvault", "consul": "HashiCorp Consul", "couchbase": "Couchbase Server",
 "couchdb": "Apache CouchDB", "debian": "Debian", "dotnet": ".NET", "eclipse-temurin": "Eclipse Temurin (Java)", "elasticsearch": "Elasticsearch", "envoy": "Envoy", "esxi": "VMware ESXi", "etcd": "etcd",
 "flink": "Apache Flink", "fluent_bit": "Fluent Bit", "flux": "Flux", "forgejo": "Forgejo", "fortigate": "FortiOS (FortiGate)", "foundationdb": "FoundationDB", "gatekeeper": "OPA Gatekeeper",
 "gitlab": "GitLab", "go": "Go", "google-kubernetes-engine": "Google Kubernetes Engine", "grafana": "Grafana", "graylog": "Graylog", "haproxy": "HAProxy", "harbor": "Harbor", "hbase": "Apache HBase",
 "ibm_mq": "IBM MQ", "icinga": "Icinga", "influxdb": "InfluxDB", "istio": "Istio", "jaeger": "Jaeger", "jenkins": "Jenkins", "kafka": "Apache Kafka", "keycloak": "Keycloak", "kibana": "Kibana",
 "kong_gateway": "Kong Gateway", "kubernetes": "Kubernetes", "kyverno": "Kyverno", "logstash": "Logstash", "loki": "Grafana Loki", "longhorn": "Longhorn", "mariadb": "MariaDB",
 "meilisearch": "Meilisearch", "memcached": "Memcached", "metallb": "MetalLB", "mongodb": "MongoDB", "mssqlserver": "Microsoft SQL Server", "mysql": "MySQL", "neo4j": "Neo4j",
 "netapp-ontap": "NetApp ONTAP", "nexus": "Sonatype Nexus Repository", "nginx": "NGINX", "nifi": "Apache NiFi", "nodejs": "Node.js", "nomad": "HashiCorp Nomad", "nutanix-aos": "Nutanix AOS",
 "openbao": "OpenBao", "opensearch": "OpenSearch", "openshift": "Red Hat OpenShift", "openvpn": "OpenVPN", "opnsense": "OPNsense", "oracle-linux": "Oracle Linux", "oracle_database": "Oracle Database",
 "ovirt": "oVirt", "palo_alto": "PAN-OS (Palo Alto Networks)", "php": "PHP", "postgresql": "PostgreSQL", "prometheus": "Prometheus", "proxmox_backup": "Proxmox Backup Server", "proxmox_ve": "Proxmox VE",
 "pulsar": "Apache Pulsar", "python": "Python", "rabbitmq": "RabbitMQ", "rancher": "Rancher", "redis": "Redis", "rhel": "Red Hat Enterprise Linux", "rocky-linux": "Rocky Linux", "ruby": "Ruby",
 "sles": "SUSE Linux Enterprise", "solr": "Apache Solr", "sonarqube": "SonarQube", "spark": "Apache Spark", "spring-boot": "Spring Boot", "squid": "Squid", "tarantool": "Tarantool", "telegraf": "Telegraf",
 "teleport": "Teleport", "tomcat": "Apache Tomcat", "traefik": "Traefik", "truenas": "TrueNAS", "ubuntu": "Ubuntu", "valkey": "Valkey", "varnish": "Varnish Cache", "vault": "HashiCorp Vault",
 "veeam_backup": "Veeam Backup & Replication", "vitess": "Vitess", "windows-server": "Windows Server", "xcp_ng": "XCP-ng", "zabbix": "Zabbix", "zookeeper": "Apache ZooKeeper",
}
DROP = {"containerd", "docker-engine"}


def render(path):
    """versions.json with the display names filled in and the tables nobody uses dropped (text, number of tables)."""
    data = json.load(open(path, encoding="utf-8"))
    out = []
    for s in data:
        if s["id"] in DROP:
            continue
        name = NAMES.get(s["id"])
        if not name:
            raise SystemExit(f"no display name for table {s['id']}")
        item = {"id": s["id"], "name": name}
        item.update({k: v for k, v in s.items() if k not in ("id", "name")})
        out.append(item)
    return json.dumps(out, indent=1, ensure_ascii=False) + "\n", len(out)
