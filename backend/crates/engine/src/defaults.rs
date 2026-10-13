//! Default configuration shipped with the binary (embedded at compile time).
//!
//! These are *seeds*: the server copies them into the database on first start and the admin
//! panel edits the database copy. Upgrades never overwrite admin modifications (see the
//! server's seeding logic, which tracks a content hash per item).

use crate::catalog::Catalog;
use crate::config::{EngineSettings, IacTemplate, Rule, Template};
use crate::model::Architecture;
use crate::text::LText;
use serde::{Deserialize, Serialize};

pub const CATALOG_JSON: &str = include_str!("../defaults/catalog.json");
pub const ENGINE_JSON: &str = include_str!("../defaults/engine.json");
pub const RULES_JSON: &str = include_str!("../defaults/rules.json");
pub const DEMO_PROJECT_JSON: &str = include_str!("../defaults/demo_project.json");
pub const PLACEMENT_JSON: &str = include_str!("../defaults/placement.json");
pub const VERSIONS_JSON: &str = include_str!("../defaults/versions.json");

pub const TEMPLATES_JSON: &[(&str, &str)] = &[
    ("two-dc-witness", include_str!("../defaults/templates/two-dc-witness.json")),
    ("three-az-region", include_str!("../defaults/templates/three-az-region.json")),
    ("multi-region-pilot-light", include_str!("../defaults/templates/multi-region-pilot-light.json")),
    ("event-driven-kafka", include_str!("../defaults/templates/event-driven-kafka.json")),
    ("single-dc-racks", include_str!("../defaults/templates/single-dc-racks.json")),
    ("three-sites-quorum", include_str!("../defaults/templates/three-sites-quorum.json")),
    ("five-sites-two-failures", include_str!("../defaults/templates/five-sites-two-failures.json")),
    ("ten-sites-regions", include_str!("../defaults/templates/ten-sites-regions.json")),
];

/// Infrastructure-as-code generators, one per catalog kind (see `iac`).
pub const IAC_JSON: &[(&str, &str)] = &[
    ("users", include_str!("../defaults/iac/users.json")),
    ("gslb", include_str!("../defaults/iac/gslb.json")),
    ("cdn", include_str!("../defaults/iac/cdn.json")),
    ("waf", include_str!("../defaults/iac/waf.json")),
    ("l7", include_str!("../defaults/iac/l7.json")),
    ("l4", include_str!("../defaults/iac/l4.json")),
    ("vpn_dci", include_str!("../defaults/iac/vpn_dci.json")),
    ("kubernetes", include_str!("../defaults/iac/kubernetes.json")),
    ("vm_pool", include_str!("../defaults/iac/vm_pool.json")),
    ("functions", include_str!("../defaults/iac/functions.json")),
    ("api_gateway", include_str!("../defaults/iac/api_gateway.json")),
    ("postgresql", include_str!("../defaults/iac/postgresql.json")),
    ("redis", include_str!("../defaults/iac/redis.json")),
    ("kafka", include_str!("../defaults/iac/kafka.json")),
    ("clickhouse", include_str!("../defaults/iac/clickhouse.json")),
    ("object_storage", include_str!("../defaults/iac/object_storage.json")),
    ("opensearch", include_str!("../defaults/iac/opensearch.json")),
    ("backup_storage", include_str!("../defaults/iac/backup_storage.json")),
    ("etcd", include_str!("../defaults/iac/etcd.json")),
    ("vault", include_str!("../defaults/iac/vault.json")),
    ("monitoring", include_str!("../defaults/iac/monitoring.json")),
    ("gitops", include_str!("../defaults/iac/gitops.json")),
    ("nginx", include_str!("../defaults/iac/nginx.json")),
    ("haproxy", include_str!("../defaults/iac/haproxy.json")),
    ("envoy", include_str!("../defaults/iac/envoy.json")),
    ("traefik", include_str!("../defaults/iac/traefik.json")),
    ("caddy", include_str!("../defaults/iac/caddy.json")),
    ("apache_httpd", include_str!("../defaults/iac/apache_httpd.json")),
    ("varnish", include_str!("../defaults/iac/varnish.json")),
    ("squid", include_str!("../defaults/iac/squid.json")),
    ("metallb", include_str!("../defaults/iac/metallb.json")),
    ("firewall", include_str!("../defaults/iac/firewall.json")),
    ("network_device", include_str!("../defaults/iac/network_device.json")),
    ("dns_server", include_str!("../defaults/iac/dns_server.json")),
    ("nomad", include_str!("../defaults/iac/nomad.json")),
    ("kong", include_str!("../defaults/iac/kong.json")),
    ("apisix", include_str!("../defaults/iac/apisix.json")),
    ("airflow", include_str!("../defaults/iac/airflow.json")),
    ("spark", include_str!("../defaults/iac/spark.json")),
    ("flink", include_str!("../defaults/iac/flink.json")),
    ("nifi", include_str!("../defaults/iac/nifi.json")),
    ("mysql", include_str!("../defaults/iac/mysql.json")),
    ("mariadb", include_str!("../defaults/iac/mariadb.json")),
    ("mssql", include_str!("../defaults/iac/mssql.json")),
    ("oracle", include_str!("../defaults/iac/oracle.json")),
    ("cockroachdb", include_str!("../defaults/iac/cockroachdb.json")),
    ("yugabytedb", include_str!("../defaults/iac/yugabytedb.json")),
    ("tidb", include_str!("../defaults/iac/tidb.json")),
    ("vitess", include_str!("../defaults/iac/vitess.json")),
    ("mongodb", include_str!("../defaults/iac/mongodb.json")),
    ("couchbase", include_str!("../defaults/iac/couchbase.json")),
    ("couchdb", include_str!("../defaults/iac/couchdb.json")),
    ("arangodb", include_str!("../defaults/iac/arangodb.json")),
    ("neo4j", include_str!("../defaults/iac/neo4j.json")),
    ("cassandra", include_str!("../defaults/iac/cassandra.json")),
    ("scylladb", include_str!("../defaults/iac/scylladb.json")),
    ("hbase", include_str!("../defaults/iac/hbase.json")),
    ("foundationdb", include_str!("../defaults/iac/foundationdb.json")),
    ("tarantool", include_str!("../defaults/iac/tarantool.json")),
    ("influxdb", include_str!("../defaults/iac/influxdb.json")),
    ("valkey", include_str!("../defaults/iac/valkey.json")),
    ("memcached", include_str!("../defaults/iac/memcached.json")),
    ("rabbitmq", include_str!("../defaults/iac/rabbitmq.json")),
    ("pulsar", include_str!("../defaults/iac/pulsar.json")),
    ("activemq", include_str!("../defaults/iac/activemq.json")),
    ("ibm_mq", include_str!("../defaults/iac/ibm_mq.json")),
    ("nats", include_str!("../defaults/iac/nats.json")),
    ("redpanda", include_str!("../defaults/iac/redpanda.json")),
    ("elasticsearch", include_str!("../defaults/iac/elasticsearch.json")),
    ("solr", include_str!("../defaults/iac/solr.json")),
    ("meilisearch", include_str!("../defaults/iac/meilisearch.json")),
    ("ceph", include_str!("../defaults/iac/ceph.json")),
    ("longhorn", include_str!("../defaults/iac/longhorn.json")),
    ("nas_san", include_str!("../defaults/iac/nas_san.json")),
    ("consul", include_str!("../defaults/iac/consul.json")),
    ("zookeeper", include_str!("../defaults/iac/zookeeper.json")),
    ("openbao", include_str!("../defaults/iac/openbao.json")),
    ("zabbix", include_str!("../defaults/iac/zabbix.json")),
    ("icinga", include_str!("../defaults/iac/icinga.json")),
    ("checkmk", include_str!("../defaults/iac/checkmk.json")),
    ("telegraf", include_str!("../defaults/iac/telegraf.json")),
    ("fluent_bit", include_str!("../defaults/iac/fluent_bit.json")),
    ("otel_collector", include_str!("../defaults/iac/otel_collector.json")),
    ("grafana", include_str!("../defaults/iac/grafana.json")),
    ("loki", include_str!("../defaults/iac/loki.json")),
    ("graylog", include_str!("../defaults/iac/graylog.json")),
    ("logstash", include_str!("../defaults/iac/logstash.json")),
    ("kibana", include_str!("../defaults/iac/kibana.json")),
    ("jaeger", include_str!("../defaults/iac/jaeger.json")),
    ("jenkins", include_str!("../defaults/iac/jenkins.json")),
    ("gitlab", include_str!("../defaults/iac/gitlab.json")),
    ("forgejo", include_str!("../defaults/iac/forgejo.json")),
    ("sonarqube", include_str!("../defaults/iac/sonarqube.json")),
    ("artifactory", include_str!("../defaults/iac/artifactory.json")),
    ("nexus", include_str!("../defaults/iac/nexus.json")),
    ("harbor", include_str!("../defaults/iac/harbor.json")),
    ("keycloak", include_str!("../defaults/iac/keycloak.json")),
    ("authentik", include_str!("../defaults/iac/authentik.json")),
    ("teleport", include_str!("../defaults/iac/teleport.json")),
    ("boundary", include_str!("../defaults/iac/boundary.json")),
    ("cert_manager", include_str!("../defaults/iac/cert_manager.json")),
    ("kyverno", include_str!("../defaults/iac/kyverno.json")),
    ("gatekeeper", include_str!("../defaults/iac/gatekeeper.json")),
    ("cni", include_str!("../defaults/iac/cni.json")),
    ("service_mesh", include_str!("../defaults/iac/service_mesh.json")),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoProject {
    pub name: String,
    pub description: LText,
    pub architecture: Architecture,
}

pub fn catalog() -> Catalog {
    serde_json::from_str(CATALOG_JSON).expect("embedded catalog.json is valid")
}

pub fn engine_settings() -> EngineSettings {
    serde_json::from_str(ENGINE_JSON).expect("embedded engine.json is valid")
}

pub fn rules() -> Vec<Rule> {
    serde_json::from_str(RULES_JSON).expect("embedded rules.json is valid")
}

pub fn templates() -> Vec<Template> {
    TEMPLATES_JSON.iter().map(|(id, s)| serde_json::from_str(s).unwrap_or_else(|e| panic!("embedded template {id} is valid: {e}"))).collect()
}

pub fn iac_templates() -> Vec<IacTemplate> {
    IAC_JSON.iter().map(|(id, s)| serde_json::from_str(s).unwrap_or_else(|e| panic!("embedded iac template {id} is valid: {e}"))).collect()
}

pub fn placement() -> crate::placement::PlacementConfig {
    serde_json::from_str(PLACEMENT_JSON).expect("embedded placement.json is valid")
}

pub fn versions() -> Vec<crate::lifecycle::VersionSet> {
    serde_json::from_str(VERSIONS_JSON).expect("embedded versions.json is valid")
}

/// A clock that says nothing: lifecycle checks are not applicable without a date.
pub fn no_clock() -> String {
    String::new()
}

/// The complete engine configuration as shipped (what a fresh installation starts with). It has no clock.
pub fn engine_config(lang: &str) -> crate::config::EngineConfig {
    crate::config::EngineConfig {
        catalog: catalog(),
        rules: rules(),
        settings: engine_settings(),
        iac: iac_templates(),
        placement: placement(),
        versions: versions(),
        today: no_clock,
        lang: lang.into(),
    }
}

pub fn demo_project() -> DemoProject {
    serde_json::from_str(DEMO_PROJECT_JSON).expect("embedded demo_project.json is valid")
}
