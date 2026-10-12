//! Writers: the plan as Kubernetes manifests (kustomize), Helm (helmfile) and Terraform/OpenTofu.
//!
//! All three are views of the same plan; YAML and HCL are produced from structured values.

use super::plan::{build_plan, Plan, PlannedRelease, Report};
use super::yaml::{to_yaml, to_yaml_stream};
use crate::config::EngineConfig;
use crate::export::ExportFile;
use crate::model::Architecture;
use crate::text::interpolate_l;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Files plus the report of what was and was not generated.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    pub files: Vec<ExportFile>,
    pub report: Report,
}

fn file(path: impl Into<String>, content: impl Into<String>) -> ExportFile {
    ExportFile { path: path.into(), content: content.into() }
}

struct T<'a> {
    cfg: &'a EngineConfig,
    lang: &'a str,
}

impl T<'_> {
    fn get(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let map: BTreeMap<String, Value> = vars.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        interpolate_l(&self.cfg.settings.export.text(self.lang, key), &map, self.lang)
    }
}

// -------------------------------------------------------------------------------------------
// Shared README parts
// -------------------------------------------------------------------------------------------

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn readme(plan: &Plan, t: &T<'_>, fmt: &str, steps: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", t.get(&format!("iac.readme.title.{fmt}"), &[("name", &plan.arch_name)])));
    out.push_str(&format!("{}\n\n", t.get(&format!("iac.readme.intro.{fmt}"), &[])));
    out.push_str(&format!("{}\n\n", t.get("iac.readme.honest", &[])));

    if plan.clusters.is_empty() {
        out.push_str(&format!("> {}\n\n", t.get("iac.readme.no_clusters", &[("trait", &t.cfg.settings.export.k8s.cluster_trait)])));
    } else {
        out.push_str(&format!(
            "## {}\n\n| {} | {} | {} |\n|---|---|---|\n",
            t.get("iac.readme.clusters", &[]),
            t.get("iac.readme.col.cluster", &[]),
            t.get("iac.readme.col.dir", &[]),
            t.get("iac.readme.col.content", &[])
        ));
        for (c, cl) in plan.clusters.iter().enumerate() {
            let n_obj = plan.cluster_objects(c).count();
            let n_rel = plan.cluster_releases(c).filter(|r| !r.operator).count();
            let n_op = plan.cluster_releases(c).filter(|r| r.operator).count();
            out.push_str(&format!(
                "| {} | `{}/` | {} |\n",
                md_cell(&cl.name),
                cl.slug,
                t.get("iac.readme.content", &[("objects", &n_obj.to_string()), ("releases", &n_rel.to_string()), ("operators", &n_op.to_string())])
            ));
        }
        out.push('\n');
        out.push_str(&format!("## {}\n\n", t.get("iac.readme.apply", &[])));
        for cl in &plan.clusters {
            out.push_str(&format!("### {} (`{}`)\n\n```sh\n{}\n```\n\n", cl.name, cl.slug, steps(&cl.slug)));
        }
    }

    if !plan.report.prerequisites.is_empty() {
        out.push_str(&format!(
            "## {}\n\n{}\n\n| {} | {} | {} | {} |\n|---|---|---|---|\n",
            t.get("iac.readme.prereq", &[]),
            t.get("iac.readme.prereq.text", &[]),
            t.get("iac.readme.col.cluster", &[]),
            t.get("iac.readme.col.component", &[]),
            t.get("iac.readme.col.chart", &[]),
            t.get("iac.readme.col.version", &[])
        ));
        for p in &plan.report.prerequisites {
            out.push_str(&format!("| {} | {} | `{}` | `{}` |\n", p.cluster, md_cell(&p.title), md_cell(&format!("{}/{}", p.repo, p.chart)), p.version));
        }
        out.push('\n');
    }

    if !plan.report.secrets.is_empty() {
        out.push_str(&format!(
            "## {}\n\n{}\n\n| {} | {} | {} | {} |\n|---|---|---|---|\n",
            t.get("iac.readme.secrets", &[]),
            t.get("iac.readme.secrets.text", &[]),
            t.get("iac.readme.col.cluster", &[]),
            t.get("iac.readme.col.namespace", &[]),
            t.get("iac.readme.col.secret", &[]),
            t.get("iac.readme.col.keys", &[])
        ));
        for s in &plan.report.secrets {
            let hint = s.hint.as_ref().map(|h| format!(" — {}", md_cell(h))).unwrap_or_default();
            out.push_str(&format!("| {} | {} | `{}`{} | {} |\n", s.cluster, s.namespace, s.name, hint, s.keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")));
        }
        out.push('\n');
    }

    out.push_str(&format!("## {}\n\n", t.get("iac.readme.skipped", &[])));
    if plan.report.skipped.is_empty() {
        out.push_str(&format!("{}\n\n", t.get("iac.readme.skipped.none", &[])));
    } else {
        out.push_str(&format!("| {} | {} |\n|---|---|\n", t.get("iac.readme.col.component", &[]), t.get("iac.readme.col.reason", &[])));
        for s in &plan.report.skipped {
            out.push_str(&format!("| {} (`{}`) | {} |\n", md_cell(&s.name), s.kind, md_cell(&s.message)));
        }
        out.push('\n');
    }

    if !plan.report.notes.is_empty() {
        out.push_str(&format!("## {}\n\n", t.get("iac.readme.notes", &[])));
        for n in &plan.report.notes {
            out.push_str(&format!("- {}{}\n", if n.level == "warn" { "⚠ " } else { "" }, n.message));
        }
        out.push('\n');
    }
    out
}

fn helm_install_cmd(r: &PlannedRelease, values_file: Option<&str>) -> String {
    let mut cmd = String::new();
    let chart = if r.repo.starts_with("oci://") {
        format!("{}/{}", r.repo.trim_end_matches('/'), r.chart)
    } else if r.repo.is_empty() {
        r.chart.clone()
    } else {
        cmd.push_str(&format!("helm repo add {} {}\n", repo_alias(&r.repo), r.repo));
        format!("{}/{}", repo_alias(&r.repo), r.chart)
    };
    cmd.push_str(&format!("helm upgrade --install {} {} --namespace {} --create-namespace", r.name, chart, r.namespace));
    if let Some(v) = &r.version {
        cmd.push_str(&format!(" --version {v}"));
    }
    if let Some(f) = values_file {
        cmd.push_str(&format!(" -f {f}"));
    }
    cmd.push_str(" --wait");
    cmd
}

fn repo_alias(url: &str) -> String {
    let s: String = url.trim_start_matches("https://").trim_start_matches("http://").chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    s.split('-').filter(|p| !p.is_empty()).take(3).collect::<Vec<_>>().join("-")
}

fn is_empty_values(v: &Value) -> bool {
    v.is_null() || v.as_object().map(|m| m.is_empty()).unwrap_or(false)
}

fn namespaces_doc(names: &[String], plan: &Plan) -> Vec<Value> {
    names
        .iter()
        .map(|n| {
            let mut labels = Map::new();
            labels.insert("app.kubernetes.io/managed-by".into(), json!("infra-architect"));
            labels.insert("app.kubernetes.io/part-of".into(), json!(plan.arch_slug));
            json!({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": n, "labels": labels}})
        })
        .collect()
}

// -------------------------------------------------------------------------------------------
// Kubernetes manifests
// -------------------------------------------------------------------------------------------

pub fn kubernetes(arch: &Architecture, cfg: &EngineConfig, lang: &str) -> Bundle {
    let plan = build_plan(arch, cfg, lang);
    let t = T { cfg, lang };
    let mut files = vec![];
    for (c, cl) in plan.clusters.iter().enumerate() {
        let objs: Vec<_> = plan.cluster_objects(c).collect();
        if objs.is_empty() {
            continue;
        }
        let mut resources = vec![];
        let ns = plan.object_namespaces(c);
        if !ns.is_empty() {
            files.push(file(format!("{}/namespaces.yaml", cl.slug), to_yaml_stream(&namespaces_doc(&ns, &plan))));
            resources.push("namespaces.yaml".to_string());
        }
        for o in objs {
            files.push(file(format!("{}/{}", cl.slug, o.file), to_yaml(&o.body)));
            resources.push(o.file.clone());
        }
        files.push(file(format!("{}/kustomization.yaml", cl.slug), to_yaml(&json!({"apiVersion": "kustomize.config.k8s.io/v1beta1", "kind": "Kustomization", "resources": resources}))));
    }
    // prerequisite charts as values files + the commands in the README
    let mut install: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in plan.releases.iter() {
        let slug = &plan.clusters[r.cluster].slug;
        let vf = if is_empty_values(&r.values) {
            None
        } else {
            let p = format!("{}/prerequisites/{}.values.yaml", slug, r.name);
            files.push(file(p, to_yaml(&r.values)));
            Some(format!("prerequisites/{}.values.yaml", r.name))
        };
        install.entry(slug.clone()).or_default().push(helm_install_cmd(r, vf.as_deref()));
    }
    let has_objects = |slug: &str| files.iter().any(|f| f.path == format!("{slug}/kustomization.yaml"));
    let steps = |slug: &str| {
        let mut s = String::new();
        s.push_str(&format!("cd {slug}\n"));
        for c in install.get(slug).into_iter().flatten() {
            s.push_str(c);
            s.push('\n');
        }
        if has_objects(slug) {
            s.push_str("kubectl apply -k .");
        }
        s.trim_end().to_string()
    };
    let readme_md = readme(&plan, &t, "kubernetes", &steps);
    files.insert(0, file("README.md", readme_md));
    Bundle { files, report: plan.report }
}

// -------------------------------------------------------------------------------------------
// Helm / helmfile
// -------------------------------------------------------------------------------------------

/// Raw manifests must survive Helm's template engine untouched.
fn helm_escape(s: &str) -> String {
    s.replace("{{", "{{ \"{{\" }}")
}

pub fn helm(arch: &Architecture, cfg: &EngineConfig, lang: &str) -> Bundle {
    let plan = build_plan(arch, cfg, lang);
    let t = T { cfg, lang };
    let k8s = &cfg.settings.export.k8s;
    let mut files = vec![];
    for (c, cl) in plan.clusters.iter().enumerate() {
        let rels: Vec<&PlannedRelease> = plan.cluster_releases(c).collect();
        let objs: Vec<_> = plan.cluster_objects(c).collect();
        if rels.is_empty() && objs.is_empty() {
            continue;
        }
        let ns = plan.cluster_namespaces(c);
        let mut repos: BTreeMap<String, String> = BTreeMap::new(); // url -> alias
        let mut repo_entries: Vec<Value> = vec![];
        let mut releases: Vec<Value> = vec![];
        let key = |r: &PlannedRelease| format!("{}/{}", r.namespace, r.name);
        let ns_release = "namespaces";
        let ns_key = format!("{}/{}", k8s.release_namespace, ns_release);

        // all namespaces are created first by one small chart, so no release has to create (and
        // possibly collide on) a namespace
        if !ns.is_empty() {
            let dir = format!("{}/charts/namespaces", cl.slug);
            files.push(file(
                format!("{dir}/Chart.yaml"),
                to_yaml(&json!({"apiVersion": "v2", "name": ns_release, "description": "Namespaces of the architecture", "version": "0.1.0", "type": "application"})),
            ));
            files.push(file(format!("{dir}/templates/namespaces.yaml"), helm_escape(&to_yaml_stream(&namespaces_doc(&ns, &plan)))));
            releases.push(json!({"name": ns_release, "namespace": k8s.release_namespace, "chart": "./charts/namespaces", "wait": true}));
        }
        let ns_needs = |needs: &mut Vec<String>| {
            if !ns.is_empty() {
                needs.insert(0, ns_key.clone());
            }
        };

        for r in &rels {
            let chart = if r.repo.is_empty() {
                r.chart.clone()
            } else {
                let next = repos.len() + 1;
                let alias = repos.entry(r.repo.clone()).or_insert_with(|| format!("repo{next}")).clone();
                if !repo_entries.iter().any(|e| e["name"] == json!(alias)) {
                    match r.repo.strip_prefix("oci://") {
                        Some(rest) => repo_entries.push(json!({"name": alias, "url": rest, "oci": true})),
                        None => repo_entries.push(json!({"name": alias, "url": r.repo})),
                    }
                }
                format!("{}/{}", alias, r.chart)
            };
            let mut rel = Map::new();
            rel.insert("name".into(), json!(r.name));
            rel.insert("namespace".into(), json!(r.namespace));
            rel.insert("chart".into(), json!(chart));
            if let Some(v) = &r.version {
                rel.insert("version".into(), json!(v));
            }
            rel.insert("wait".into(), json!(true));
            if !is_empty_values(&r.values) {
                files.push(file(format!("{}/values/{}.yaml", cl.slug, r.name), to_yaml(&r.values)));
                rel.insert("values".into(), json!([format!("values/{}.yaml", r.name)]));
            }
            // operators wait for their own prerequisites, workload charts for the operators they require
            let mut needs: Vec<String> = rels.iter().filter(|x| r.needs.contains(&x.name)).map(|x| key(x)).collect();
            ns_needs(&mut needs);
            if !needs.is_empty() {
                rel.insert("needs".into(), json!(needs));
            }
            releases.push(Value::Object(rel));
        }
        if !objs.is_empty() {
            let chart_dir = format!("{}/charts/workloads", cl.slug);
            files.push(file(
                format!("{chart_dir}/Chart.yaml"),
                to_yaml(&json!({"apiVersion": "v2", "name": "workloads", "description": format!("Objects generated from the architecture model “{}”", plan.arch_name), "version": "0.1.0", "type": "application"})),
            ));
            for o in &objs {
                files.push(file(format!("{chart_dir}/templates/{}", o.file), helm_escape(&to_yaml(&o.body))));
            }
            let wanted: std::collections::BTreeSet<&String> = objs.iter().flat_map(|o| o.requires.iter()).collect();
            let mut needs: Vec<String> = rels.iter().filter(|x| x.op_id.as_ref().map(|i| wanted.contains(i)).unwrap_or(false)).map(|x| key(x)).collect();
            ns_needs(&mut needs);
            let mut rel = Map::new();
            rel.insert("name".into(), json!("workloads"));
            rel.insert("namespace".into(), json!(k8s.release_namespace));
            rel.insert("chart".into(), json!("./charts/workloads"));
            rel.insert("wait".into(), json!(true));
            if !needs.is_empty() {
                rel.insert("needs".into(), json!(needs));
            }
            releases.push(Value::Object(rel));
        }
        let mut doc = Map::new();
        if !repo_entries.is_empty() {
            doc.insert("repositories".into(), Value::Array(repo_entries));
        }
        doc.insert("releases".into(), Value::Array(releases));
        files.push(file(format!("{}/helmfile.yaml", cl.slug), to_yaml(&Value::Object(doc))));
    }
    let steps = |slug: &str| format!("cd {slug}\nhelmfile diff   # review\nhelmfile apply");
    let readme_md = readme(&plan, &t, "helm", &steps);
    files.insert(0, file("README.md", readme_md));
    Bundle { files, report: plan.report }
}

// -------------------------------------------------------------------------------------------
// Terraform / OpenTofu
// -------------------------------------------------------------------------------------------

fn hcl_str(s: &str) -> String {
    let e = s.replace('\\', "\\\\").replace('"', "\\\"").replace("${", "$${").replace("%{", "%%{").replace('\n', "\\n");
    format!("\"{}\"", e)
}

fn hcl_ident(s: &str) -> String {
    let mut o: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if o.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
        o = format!("r_{o}");
    }
    o
}

/// `key = value` lines of one group, aligned on `=` the way `terraform fmt` writes them.
fn attrs(indent: usize, items: &[(&str, String)]) -> String {
    let w = items.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let pad = "  ".repeat(indent);
    items.iter().map(|(k, v)| format!("{pad}{k:<w$} = {v}\n")).collect()
}

fn tf_versions(cfg: &EngineConfig, providers: &[&str], plan: &Plan, cluster: &str, stage: &str) -> String {
    let ex = &cfg.settings.export;
    let mut s = format!("terraform {{\n  required_version = {}\n\n  required_providers {{\n", hcl_str(&ex.terraform_required_version));
    for p in providers {
        let req = ex.terraform_providers.get(*p);
        let source = req.map(|r| r.source.clone()).filter(|x| !x.is_empty()).unwrap_or_else(|| format!("hashicorp/{p}"));
        let mut a = vec![("source", hcl_str(&source))];
        if let Some(v) = req.map(|r| r.version.clone()).filter(|x| !x.is_empty()) {
            a.push(("version", hcl_str(&v)));
        }
        s.push_str(&format!("    {p} = {{\n{}    }}\n", attrs(3, &a)));
    }
    s.push_str("  }\n\n  ");
    let backend = ex.terraform_backend.replace("{arch}", &plan.arch_slug).replace("{cluster}", cluster).replace("{stage}", stage);
    s.push_str(backend.trim());
    s.push_str("\n}\n");
    s
}

const TF_VARIABLES: &str = "variable \"kubeconfig_path\" {\n  description = \"Path to the kubeconfig of this cluster\"\n  type        = string\n  default     = \"~/.kube/config\"\n}\n\nvariable \"kube_context\" {\n  description = \"Context inside the kubeconfig (null = current context)\"\n  type        = string\n  default     = null\n}\n";

pub fn terraform(arch: &Architecture, cfg: &EngineConfig, lang: &str) -> Bundle {
    let plan = build_plan(arch, cfg, lang);
    let t = T { cfg, lang };
    let mut files = vec![];
    for (c, cl) in plan.clusters.iter().enumerate() {
        let rels: Vec<&PlannedRelease> = plan.cluster_releases(c).collect();
        let objs: Vec<_> = plan.cluster_objects(c).collect();
        let ns = plan.cluster_namespaces(c);

        // stage 00: namespaces, operators and chart-based workloads
        if !rels.is_empty() || (!ns.is_empty() && !objs.is_empty()) {
            let dir = format!("{}/00-platform", cl.slug);
            files.push(file(format!("{dir}/versions.tf"), tf_versions(cfg, &["kubernetes", "helm"], &plan, &cl.slug, "platform")));
            files.push(file(
                format!("{dir}/providers.tf"),
                "provider \"kubernetes\" {\n  config_path    = var.kubeconfig_path\n  config_context = var.kube_context\n}\n\nprovider \"helm\" {\n  kubernetes = {\n    config_path    = var.kubeconfig_path\n    config_context = var.kube_context\n  }\n}\n",
            ));
            files.push(file(format!("{dir}/variables.tf"), TF_VARIABLES));
            let mut main = String::from("# Generated by Infra Architect from the architecture model.\n\n");
            if !ns.is_empty() {
                main.push_str(&format!("locals {{\n  namespaces = toset([{}])\n}}\n\n", ns.iter().map(|n| hcl_str(n)).collect::<Vec<_>>().join(", ")));
                main.push_str(&format!(
                    "resource \"kubernetes_namespace_v1\" \"ns\" {{\n  for_each = local.namespaces\n\n  metadata {{\n    name = each.value\n\n    labels = {{\n      \"app.kubernetes.io/managed-by\" = \"infra-architect\"\n      \"app.kubernetes.io/part-of\"    = {}\n    }}\n  }}\n}}\n\n",
                    hcl_str(&plan.arch_slug)
                ));
            }
            for r in &rels {
                let id = hcl_ident(&r.name);
                let mut a = vec![("name", hcl_str(&r.name)), ("namespace", hcl_str(&r.namespace))];
                if r.repo.starts_with("oci://") {
                    a.push(("chart", hcl_str(&format!("{}/{}", r.repo.trim_end_matches('/'), r.chart))));
                } else {
                    if !r.repo.is_empty() {
                        a.push(("repository", hcl_str(&r.repo)));
                    }
                    a.push(("chart", hcl_str(&r.chart)));
                }
                if let Some(v) = &r.version {
                    a.push(("version", hcl_str(v)));
                }
                a.push(("wait", "true".into()));
                if !is_empty_values(&r.values) {
                    files.push(file(format!("{dir}/values/{}.yaml", r.name), to_yaml(&r.values)));
                    a.push(("values", format!("[file(\"${{path.module}}/values/{}.yaml\")]", r.name)));
                }
                let mut deps: Vec<String> = vec![];
                if !ns.is_empty() {
                    deps.push("kubernetes_namespace_v1.ns".into());
                }
                for n in &r.needs {
                    if *n != r.name && rels.iter().any(|x| x.name == *n) {
                        deps.push(format!("helm_release.{}", hcl_ident(n)));
                    }
                }
                main.push_str(&format!("resource \"helm_release\" \"{id}\" {{\n{}", attrs(1, &a)));
                if !deps.is_empty() {
                    main.push_str(&format!("\n  depends_on = [{}]\n", deps.join(", ")));
                }
                main.push_str("}\n\n");
            }
            files.push(file(format!("{dir}/main.tf"), main));
        }

        // stage 10: objects (custom resources need the CRDs installed by stage 00)
        if !objs.is_empty() {
            let dir = format!("{}/10-workloads", cl.slug);
            files.push(file(format!("{dir}/versions.tf"), tf_versions(cfg, &["kubernetes"], &plan, &cl.slug, "workloads")));
            files.push(file(format!("{dir}/providers.tf"), "provider \"kubernetes\" {\n  config_path    = var.kubeconfig_path\n  config_context = var.kube_context\n}\n"));
            files.push(file(format!("{dir}/variables.tf"), TF_VARIABLES));
            for o in &objs {
                files.push(file(format!("{dir}/manifests/{}", o.file), to_yaml(&o.body)));
            }
            files.push(file(
                format!("{dir}/main.tf"),
                "# Generated by Infra Architect from the architecture model.\n# One object per file in ./manifests; apply 00-platform first (operators install the CRDs).\n\nlocals {\n  manifest_files = fileset(\"${path.module}/manifests\", \"*.yaml\")\n}\n\nresource \"kubernetes_manifest\" \"objects\" {\n  for_each = local.manifest_files\n\n  manifest = yamldecode(file(\"${path.module}/manifests/${each.value}\"))\n}\n"
                    .to_string(),
            ));
        }
    }
    let has_platform = |slug: &str| files.iter().any(|f| f.path == format!("{slug}/00-platform/main.tf"));
    let has_workloads = |slug: &str| files.iter().any(|f| f.path == format!("{slug}/10-workloads/main.tf"));
    let steps = |slug: &str| {
        let mut s = String::new();
        if has_platform(slug) {
            s.push_str(&format!("cd {slug}/00-platform && tofu init && tofu apply && cd ../..\n"));
        }
        if has_workloads(slug) {
            s.push_str(&format!("cd {slug}/10-workloads && tofu init && tofu apply"));
        }
        s.trim_end().to_string()
    };
    let readme_md = readme(&plan, &t, "terraform", &steps);
    files.insert(0, file("README.md", readme_md));
    Bundle { files, report: plan.report }
}
