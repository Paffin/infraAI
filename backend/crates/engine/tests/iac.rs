//! Infrastructure-as-code generation: placement, honesty about what is not generated, and the
//! shape of the three formats. The generated objects themselves are validated against the real
//! Kubernetes/CRD schemas, Helm and Terraform by `deploy/iac-check` (`make iac-check`).

use engine::config::EngineConfig;
use engine::defaults;
use engine::export::ExportFile;
use engine::iac::{self, Bundle};
use engine::model::Architecture;
use serde_json::json;

fn cfg() -> EngineConfig {
    defaults::engine_config("en")
}

fn template(id: &str) -> Architecture {
    defaults::templates().into_iter().find(|t| t.id == id).unwrap_or_else(|| panic!("template {id}")).architecture
}

fn file<'a>(b: &'a Bundle, path: &str) -> &'a ExportFile {
    b.files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("{path} not generated; have: {:?}", b.files.iter().map(|f| &f.path).collect::<Vec<_>>()))
}

fn set_prop(a: &mut Architecture, node: &str, key: &str, v: serde_json::Value) {
    a.nodes.iter_mut().find(|n| n.id == node).unwrap_or_else(|| panic!("node {node}")).props.insert(key.into(), v);
}

#[test]
fn shipped_templates_are_valid_and_cover_every_catalog_kind() {
    let c = cfg();
    for t in &c.iac {
        let errs = iac::validate_template(t);
        assert!(errs.is_empty(), "template {}: {errs:?}", t.id);
    }
    for k in &c.catalog.kinds {
        assert!(c.iac.iter().any(|t| t.id == k.id), "kind {} has no IaC template (a template with target `none` and a reason is enough)", k.id);
    }
    for t in c.iac.iter().filter(|t| t.target == engine::config::IacTarget::None) {
        assert!(t.reason.is_some(), "{}: a kind without a generator must say why", t.id);
    }
    for t in &c.iac {
        for r in &t.requires {
            assert!(c.settings.export.k8s.operators.contains_key(r.id()), "{} requires unknown operator {}", t.id, r.id());
        }
    }
}

#[test]
fn a_broken_expression_is_rejected_when_the_template_is_saved() {
    let mut t = defaults::iac_templates().into_iter().find(|t| t.id == "postgresql").unwrap();
    t.objects[0].body["spec"]["instances"] = json!("{{ site.instances + }}");
    t.vars[0].expr = "p.version ~".into();
    let errs = iac::validate_template(&t);
    assert!(errs.iter().any(|e| e.contains("objects[0].body.spec.instances")), "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("vars[0]")), "{errs:?}");
}

#[test]
fn what_cannot_be_generated_is_reported_never_stubbed() {
    let c = cfg();
    let b = iac::kubernetes(&template("two-dc-witness"), &c, "en");
    let skipped = |unit: &str, code: &str| b.report.skipped.iter().find(|s| s.unit == unit && s.code == code).unwrap_or_else(|| panic!("{unit}/{code} not reported: {:?}", b.report.skipped));
    assert!(skipped("gslb", "no_generator").message.contains("DNS"));
    skipped("ingress", "hosting"); // balancers in front of a cluster are VM-hosted by default
    assert_eq!(skipped("app", "needs_input").props, vec!["image".to_string()], "the application image cannot be invented");
    skipped("redis", "unsupported"); // Redis replicated between two clusters cannot be expressed with the operator
    skipped("redis", "no_cluster"); // the sentinel lives in a site without a cluster
    for f in b.files.iter().filter(|f| !f.path.ends_with("README.md")) {
        let low = f.content.to_lowercase();
        assert!(!low.contains("stub") && !low.contains("scaffold") && !low.contains("example.org") && !low.contains("terraform_data"), "{} looks like a placeholder:\n{}", f.path, f.content);
    }
    for fmt in [iac::helm(&template("two-dc-witness"), &c, "en"), iac::terraform(&template("two-dc-witness"), &c, "en")] {
        assert!(fmt.files.iter().filter(|f| !f.path.ends_with("README.md")).all(|f| {
            let low = f.content.to_lowercase();
            !low.contains("terraform_data") && !low.contains("stub") && !low.contains("example.org")
        }));
    }
}

#[test]
fn nodes_of_one_cluster_name_form_one_stretched_cluster() {
    let c = cfg();
    let b = iac::kubernetes(&template("three-az-region"), &c, "en");
    assert_eq!(b.report.clusters.len(), 1);
    let cl = &b.report.clusters[0];
    assert_eq!(cl.slug, "main");
    assert_eq!(cl.nodes.len(), 3, "three AZ nodes, one cluster");
    let pg = file(&b, "main/pg-cluster.yaml");
    assert!(pg.content.contains("instances: 3"), "{}", pg.content);
    assert!(pg.content.contains("topologyKey: topology.kubernetes.io/zone"), "replicas in different AZs spread over the zone label:\n{}", pg.content);
    assert!(pg.content.contains("synchronous:") && pg.content.contains("method: any"), "sync replication edges become synchronous standbys:\n{}", pg.content);
    assert!(!pg.content.contains("replica:\n"), "one cluster – no replica cluster");

    // without a cluster name every node is a cluster of its own
    let mut a = template("three-az-region");
    for n in a.nodes.iter_mut() {
        n.props.remove("clusterName");
    }
    let b = iac::kubernetes(&a, &c, "en");
    assert_eq!(b.report.clusters.len(), 3);
}

#[test]
fn a_standby_site_becomes_a_replica_cluster_streaming_from_the_primary_site() {
    let c = cfg();
    let b = iac::kubernetes(&template("two-dc-witness"), &c, "en");
    let primary = file(&b, "app-dc1/pg-cluster.yaml");
    let replica = file(&b, "app-dc2/pg-cluster.yaml");
    assert!(!primary.content.contains("bootstrap:") && !primary.content.contains("replica:"), "{}", primary.content);
    assert!(replica.content.contains("pg_basebackup:") && replica.content.contains("enabled: true"), "{}", replica.content);
    assert!(replica.content.contains("host: pg-rw.data.svc.clusterset.local"), "{}", replica.content);
    assert!(b.report.secrets.iter().any(|s| s.cluster == "app-dc2" && s.name == "pg-replication"), "TLS material has to be copied by the operator of the cluster");
    assert!(b.report.notes.iter().any(|n| n.level == "warn" && n.message.contains("synchronous")), "the model says sync, a replica cluster is asynchronous – that must be said");
}

#[test]
fn missing_inputs_are_asked_for_and_supplied_ones_are_used() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    for n in ["app-dc1", "app-dc2"] {
        set_prop(&mut a, n, "image", json!("registry.example.com/shop/api:1.4.2"));
        set_prop(&mut a, n, "hpaMin", json!(10));
        set_prop(&mut a, n, "hpaMax", json!(60));
        set_prop(&mut a, n, "healthPath", json!("/healthz"));
    }
    let b = iac::kubernetes(&a, &c, "en");
    assert!(b.report.skipped.iter().all(|s| s.unit != "app"), "{:?}", b.report.skipped);
    let d = file(&b, "app-dc1/app-deployment.yaml");
    assert!(d.content.contains("image: registry.example.com/shop/api:1.4.2"), "{}", d.content);
    assert!(!d.content.contains("replicas:"), "the HPA owns the replica count:\n{}", d.content);
    assert!(d.content.contains("path: /healthz") && d.content.contains("runAsNonRoot: true") && d.content.contains("topologySpreadConstraints"), "{}", d.content);
    let hpa = file(&b, "app-dc1/app-hpa.yaml");
    assert!(hpa.content.contains("minReplicas: 10") && hpa.content.contains("maxReplicas: 60"), "{}", hpa.content);
    assert!(b.files.iter().any(|f| f.path == "app-dc1/app-pdb.yaml"));
}

#[test]
fn hosting_decides_whether_a_component_is_deployed_onto_the_cluster() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    set_prop(&mut a, "ingress-dc1", "hosting", json!("kubernetes"));
    let b = iac::helm(&a, &c, "en");
    let hf = file(&b, "app-dc1/helmfile.yaml");
    assert!(hf.content.contains("traefik") && hf.content.contains("41.7.1"), "{}", hf.content);
    assert!(b.report.skipped.iter().any(|s| s.unit == "ingress" && s.code == "hosting" && s.nodes == vec!["ingress-dc2".to_string()]), "the other site stays on VMs: {:?}", b.report.skipped);

    // managed services are never deployed by us
    let mut a = template("two-dc-witness");
    set_prop(&mut a, "pg-primary", "hosting", json!("managed"));
    set_prop(&mut a, "pg-replica", "hosting", json!("managed"));
    let b = iac::kubernetes(&a, &c, "en");
    assert!(b.files.iter().all(|f| !f.path.ends_with("pg-cluster.yaml")));
    assert!(b.report.skipped.iter().any(|s| s.unit == "pg" && s.code == "hosting"));
}

#[test]
fn a_connected_backup_storage_wires_wal_archiving_and_its_prerequisites() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    set_prop(&mut a, "backup", "bucket", json!("pg-backups"));
    set_prop(&mut a, "backup", "endpoint", json!("https://s3.dr.example.net"));
    set_prop(&mut a, "backup", "retentionDays", json!(30));
    let b = iac::kubernetes(&a, &c, "en");
    let store = file(&b, "app-dc1/pg-objectstore.yaml");
    assert!(
        store.content.contains("destinationPath: s3://pg-backups/pg/") && store.content.contains("endpointURL: https://s3.dr.example.net") && store.content.contains("retentionPolicy: 30d"),
        "{}",
        store.content
    );
    let sched = file(&b, "app-dc1/pg-scheduledbackup.yaml");
    assert!(sched.content.contains("method: plugin") && sched.content.contains("schedule: \"0 0 3 * * *\""), "base backups daily; WAL is archived continuously:\n{}", sched.content);
    assert!(file(&b, "app-dc1/pg-cluster.yaml").content.contains("isWALArchiver: true"));
    let ids: Vec<_> = b.report.prerequisites.iter().filter(|p| p.cluster == "app-dc1").map(|p| p.id.as_str()).collect();
    let pos = |id: &str| ids.iter().position(|x| *x == id).unwrap_or_else(|| panic!("{id} missing in {ids:?}"));
    assert!(pos("cert-manager") < pos("barman-cloud") && pos("cnpg") < pos("barman-cloud"), "install order follows `after`: {ids:?}");
    assert!(b.report.secrets.iter().any(|s| s.name == "pg-backup-credentials" && s.keys.contains(&"ACCESS_SECRET_KEY".to_string())));
    // the replica site does not archive
    assert!(b.files.iter().all(|f| !f.path.starts_with("app-dc2/") || !f.path.contains("objectstore")));
    // and without a bucket nothing is faked
    let b = iac::kubernetes(&template("two-dc-witness"), &c, "en");
    assert!(b.files.iter().all(|f| !f.path.contains("objectstore")));
    assert!(b.report.notes.iter().any(|n| n.level == "warn" && n.message.contains("bucket")));
}

#[test]
fn the_three_formats_are_views_of_one_plan() {
    let c = cfg();
    let a = template("two-dc-witness");
    let k = iac::kubernetes(&a, &c, "en");
    let h = iac::helm(&a, &c, "en");
    let t = iac::terraform(&a, &c, "en");
    assert_eq!(k.report, h.report);
    assert_eq!(k.report, t.report);

    // kubernetes: kustomization lists every object file
    let kz = file(&k, "app-dc1/kustomization.yaml");
    for f in k.files.iter().filter(|f| f.path.starts_with("app-dc1/") && f.path.ends_with(".yaml") && !f.path.contains("prerequisites/") && !f.path.ends_with("kustomization.yaml")) {
        let name = f.path.trim_start_matches("app-dc1/");
        assert!(kz.content.contains(&format!("- {name}")), "{name} missing in\n{}", kz.content);
    }
    assert!(file(&k, "README.md").content.contains("kubectl apply -k ."));

    // helm: namespaces first, operators wait for them, the custom resources are a local chart
    let hf = file(&h, "app-dc1/helmfile.yaml");
    let first = hf.content.find("name: namespaces").unwrap();
    assert!(first < hf.content.find("name: cnpg").unwrap() && first < hf.content.find("name: workloads").unwrap(), "{}", hf.content);
    assert!(hf.content.contains("default/namespaces") && hf.content.contains("cnpg-system/cnpg"), "{}", hf.content);
    assert!(h.files.iter().any(|f| f.path == "app-dc1/charts/workloads/templates/pg-cluster.yaml"));

    // terraform: platform then workloads; providers pinned from settings; state key per stage
    let v = file(&t, "app-dc1/00-platform/versions.tf");
    assert!(v.content.contains("hashicorp/helm") && v.content.contains("~> 3.0") && v.content.contains("required_version = \">= 1.6.0\""), "{}", v.content);
    let main = file(&t, "app-dc1/00-platform/main.tf");
    assert!(main.content.contains("resource \"helm_release\" \"cnpg\"") && main.content.contains("depends_on = [kubernetes_namespace_v1.ns]"), "{}", main.content);
    let w = file(&t, "app-dc1/10-workloads/main.tf");
    assert!(w.content.contains("kubernetes_manifest") && w.content.contains("fileset"), "{}", w.content);
    assert!(t.files.iter().any(|f| f.path == "app-dc1/10-workloads/manifests/pg-cluster.yaml"));
    assert_eq!(file(&k, "app-dc1/pg-cluster.yaml").content, file(&t, "app-dc1/10-workloads/manifests/pg-cluster.yaml").content, "the same object in every format");
}

#[test]
fn generation_is_deterministic() {
    let c = cfg();
    let a = template("multi-region-pilot-light");
    for f in ["kubernetes", "helm", "terraform"] {
        let rep = engine::evaluate::evaluate(&a, &c, &[]);
        let x = engine::export::generate(f, &a, &c, &rep, &[], "en", "2026-10-12").unwrap();
        let y = engine::export::generate(f, &a, &c, &rep, &[], "en", "2026-10-12").unwrap();
        assert_eq!(x.files, y.files, "{f}");
    }
}

#[test]
fn hostile_names_cannot_break_out_of_the_generated_files() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    let evil = "evil\ninjected: true\n---\nkind: Secret \"quoted\" ${jndi:ldap://x} %{ if } {{ 7*7 }}";
    for n in a.nodes.iter_mut() {
        n.name = format!("{} {evil}", n.name);
    }
    for z in a.zones.iter_mut() {
        z.name = evil.into();
    }
    a.meta.name = Some(evil.into());
    for f in ["kubernetes", "helm", "terraform"] {
        let rep = engine::evaluate::evaluate(&a, &c, &[]);
        let out = engine::export::generate(f, &a, &c, &rep, &[], "en", "2026-10-12").unwrap();
        for file in &out.files {
            assert!(!file.path.contains(' ') && !file.path.contains('\n') && !file.path.contains(".."), "{f}: unsafe path {:?}", file.path);
            if file.path.ends_with("README.md") {
                continue; // documentation quotes names inside table cells
            }
            for line in file.content.lines() {
                assert!(!line.starts_with("injected:") && line != "kind: Secret" && !line.starts_with("kind: Secret \"quoted\""), "{f} {}: injected line {line:?}", file.path);
            }
            assert!(!file.content.contains("---\nkind: Secret"), "{f} {}: injected document", file.path);
            if file.path.ends_with(".tf") {
                assert!(!file.content.contains("${jndi") || file.content.contains("$${jndi"), "{f} {}: unescaped interpolation", file.path);
            }
        }
    }
}

#[test]
fn an_architecture_without_a_cluster_says_so() {
    let c = cfg();
    let mut a = template("two-dc-witness");
    a.nodes.retain(|n| n.kind != "kubernetes");
    a.edges.retain(|e| a.nodes.iter().any(|n| n.id == e.source) && a.nodes.iter().any(|n| n.id == e.target));
    let b = iac::terraform(&a, &c, "en");
    assert_eq!(b.files.len(), 1, "only the explanation: {:?}", b.files.iter().map(|f| &f.path).collect::<Vec<_>>());
    assert!(b.files[0].content.contains("no Kubernetes cluster"), "{}", b.files[0].content);
    assert!(b.report.notes.iter().any(|n| n.level == "warn"));
    assert!(b.report.skipped.iter().any(|s| s.code == "no_cluster"));
}

#[test]
fn texts_follow_the_language() {
    let c = cfg();
    let ru = iac::kubernetes(&template("two-dc-witness"), &c, "ru");
    assert!(ru.report.skipped.iter().any(|s| s.code == "no_generator" && s.message.contains("DNS") && s.message.chars().any(|ch| ('а'..='я').contains(&ch))), "{:?}", ru.report.skipped);
    assert!(file(&ru, "README.md").content.contains("Манифесты Kubernetes"));
}

#[test]
fn a_template_with_a_runtime_error_is_reported_and_the_rest_is_still_generated() {
    let mut c = cfg();
    let t = c.iac.iter_mut().find(|t| t.id == "kafka").unwrap();
    t.objects[0].body["spec"]["replicas"] = json!("{{ p.nonexistent }}");
    let b = iac::kubernetes(&template("two-dc-witness"), &c, "en");
    let e = b.report.skipped.iter().find(|s| s.code == "template_error").expect("template error reported");
    assert!(e.message.contains("objects[0].body.spec.replicas"), "the path of the broken expression is shown: {}", e.message);
    assert!(b.files.iter().any(|f| f.path.ends_with("pg-cluster.yaml")), "other components are unaffected");
}

#[test]
fn the_guard_rails_of_the_template_language_are_settings() {
    let arch = template("three-az-region");
    let ok = iac::kubernetes(&arch, &cfg(), "en");
    assert!(ok.report.skipped.iter().all(|s| s.code != "template_error"), "the shipped limits are generous enough: {:?}", ok.report.skipped);

    // a template that loops over the nodes of the cluster (none of the shipped ones loops at all)
    let looping = |max_items: usize| {
        let mut c = cfg();
        c.settings.export.template_limits.max_items = max_items;
        let t = c.iac.iter_mut().find(|t| t.id == "kafka").unwrap();
        t.objects[0].body["spec"]["replicas"] = json!({"$each": "cluster.nodes", "as": "m", "do": "{{ m }}"});
        iac::kubernetes(&arch, &c, "en")
    };
    assert!(looping(5_000).report.skipped.iter().all(|s| s.code != "template_error"), "the loop is fine within the limit");
    // longer than the administrator allows: refused and reported, not silently cut
    let b = looping(1);
    let errs: Vec<_> = b.report.skipped.iter().filter(|s| s.code == "template_error").collect();
    assert!(errs.iter().any(|e| e.message.contains("more than 1 items")), "{:?}", b.report.skipped.iter().map(|s| (&s.code, &s.message)).collect::<Vec<_>>());

    // a tiny instruction budget stops the expressions
    let mut c = cfg();
    c.settings.export.template_limits.fuel = 3;
    let b = iac::kubernetes(&arch, &c, "en");
    assert!(b.report.skipped.iter().any(|s| s.code == "template_error"), "{:?}", b.report.skipped.iter().map(|s| &s.code).collect::<Vec<_>>());
}
