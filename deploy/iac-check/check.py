#!/usr/bin/env python3
"""Validate generated infrastructure code against the real upstream artifacts.

Input  : a directory with exports  <name>/{kubernetes,helm,terraform}/…  (see export-all.sh)
Checks :
  1. every YAML file parses; kustomize builds the generated manifests
  2. every Kubernetes object conforms to its schema – core objects to the official Kubernetes
     schemas, custom resources to the CRDs of the *pinned operator charts* (strict: unknown or
     misspelled fields are errors)
  3. every Helm release of the helmfiles renders with the generated values, and the rendered
     objects conform too; the local charts render as well
  4. every Terraform stage passes `init`, `validate` and `fmt -check`
Exit code 0 only if nothing failed.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import yaml

OUT = Path(os.environ.get("IAC_OUT", "/out"))
DEFAULTS = Path(os.environ.get("DEFAULTS", "/defaults"))
K8S_VERSION = os.environ.get("K8S_VERSION", "1.32.0")
WORK = Path(tempfile.mkdtemp(prefix="iac-check-"))
SCHEMAS = WORK / "schemas"
SCHEMAS.mkdir()

failures: list[str] = []
passed = 0


def log(msg: str) -> None:
    print(msg, flush=True)


def fail(msg: str) -> None:
    failures.append(msg)
    log(f"  FAIL {msg}")


def ok(msg: str) -> None:
    global passed
    passed += 1
    log(f"  ok   {msg}")


def run(cmd: list[str], cwd: Path | None = None, check: bool = False) -> subprocess.CompletedProcess:
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if check and p.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)}\n{p.stderr.strip()[-1500:]}")
    return p


# ------------------------------------------------------------------------------------ CRD schemas
def strictify(node):
    """JSON schema from an OpenAPI v3 schema of a CRD: strict about unknown fields."""
    if isinstance(node, list):
        return [strictify(x) for x in node]
    if not isinstance(node, dict):
        return node
    out = {}
    for k, v in node.items():
        if k in ("x-kubernetes-list-map-keys", "x-kubernetes-list-type", "x-kubernetes-map-type", "x-kubernetes-validations", "x-kubernetes-embedded-resource"):
            continue
        if k in ("properties", "patternProperties"):
            out[k] = {pk: strictify(pv) for pk, pv in v.items()}
        elif k == "default":
            continue
        else:
            out[k] = strictify(v)
    if out.get("x-kubernetes-int-or-string"):
        out.pop("type", None)
        out["oneOf"] = [{"type": "integer"}, {"type": "string"}]
    if out.get("nullable") and "type" in out and isinstance(out["type"], str):
        out["type"] = [out["type"], "null"]
    if "properties" in out and "additionalProperties" not in out and not out.get("x-kubernetes-preserve-unknown-fields"):
        out["additionalProperties"] = False
    return out


def add_crds(docs: list[dict]) -> int:
    n = 0
    for d in docs:
        if not isinstance(d, dict) or d.get("kind") != "CustomResourceDefinition":
            continue
        spec = d["spec"]
        group, kind = spec["group"], spec["names"]["kind"]
        for v in spec["versions"]:
            schema = (v.get("schema") or {}).get("openAPIV3Schema")
            if not schema:
                continue
            target = SCHEMAS / group
            target.mkdir(parents=True, exist_ok=True)
            js = strictify(schema)
            js["$schema"] = "http://json-schema.org/draft-07/schema#"
            (target / f"{kind.lower()}_{v['name']}.json").write_text(json.dumps(js))
            n += 1
    return n


def helm_args(name: str, chart: str, repo: str | None, version: str | None, namespace: str, values: list[Path]) -> list[str]:
    args = ["helm", "template", name]
    if repo and repo.startswith("oci://"):
        args.append(repo.rstrip("/") + "/" + chart)
    elif repo:
        args += [chart, "--repo", repo]
    else:
        args.append(chart)
    if version:
        args += ["--version", version]
    args += ["--namespace", namespace, "--include-crds"]
    for v in values:
        args += ["-f", str(v)]
    return args


def split_docs(text: str) -> list[str]:
    return [d for d in re.split(r"(?m)^---\s*$", text) if d.strip()]


def load_docs(text: str, tolerant: bool = False) -> list[dict]:
    """Parsed documents. `tolerant` skips documents PyYAML (YAML 1.1) cannot read – chart internals
    that are not ours and not CRDs."""
    out = []
    for raw in split_docs(text):
        try:
            d = yaml.safe_load(raw)
        except yaml.YAMLError:
            if not tolerant:
                raise
            continue
        if isinstance(d, dict):
            out.append(d)
    return out


def without_crds(text: str) -> str:
    """The rendered output minus CustomResourceDefinitions (upstream content, not ours)."""
    keep = [d for d in split_docs(text) if not re.search(r"(?m)^kind:\s*CustomResourceDefinition\s*$", d)]
    return "\n---\n".join(keep)


def collect_operator_crds() -> None:
    log("== CRD schemas of the pinned operators (engine.json → export.k8s.operators)")
    ops = json.loads((DEFAULTS / "engine.json").read_text())["export"]["k8s"]["operators"]
    for oid, op in ops.items():
        values = []
        if op.get("values"):
            f = WORK / f"op-{oid}.values.yaml"
            f.write_text(yaml.safe_dump(op["values"]))
            values = [f]
        p = run(helm_args(op.get("release") or oid, op["chart"], op["repo"], op.get("version"), op.get("namespace") or oid, values))
        if p.returncode != 0:
            fail(f"operator {oid}: helm template failed: {p.stderr.strip()[-400:]}")
            continue
        n = add_crds(load_docs(p.stdout, tolerant=True))
        ok(f"operator {oid} {op.get('version')}: {n} CRD schema(s)")


# ----------------------------------------------------------------------------------- kubeconform
def kubeconform(files: list[Path] | None = None, stdin: str | None = None, label: str = "", record: bool = True) -> bool:
    cmd = [
        "kubeconform", "-strict", "-summary", "-output", "json", "-kubernetes-version", K8S_VERSION,
        "-schema-location", "default",
        "-schema-location", str(SCHEMAS) + "/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json",
    ]
    if stdin is not None:
        p = subprocess.run(cmd + ["-"], input=stdin, capture_output=True, text=True)
    else:
        p = run(cmd + [str(f) for f in files or []])
    try:
        res = json.loads(p.stdout or "{}")
    except json.JSONDecodeError:
        if record:
            fail(f"{label}: kubeconform produced no result: {p.stderr.strip()[-300:] or p.stdout[-300:]}")
        return False
    summ = res.get("summary", {})
    bad = [r for r in res.get("resources", []) if r.get("status") not in ("statusValid", "statusSkipped")]
    if bad or summ.get("invalid") or summ.get("errors"):
        if record:
            for r in bad[:8]:
                fail(f"{label}: {r.get('filename', '')} {r.get('kind', '')}/{r.get('name', '')}: {r.get('status')} {r.get('msg', '')[:300]}")
        return False
    if record:
        ok(f"{label}: {summ.get('valid', 0)} object(s) valid")
    return True


# --------------------------------------------------------------------------------------- formats
def check_kubernetes(root: Path, name: str) -> None:
    for cl in sorted(p for p in root.iterdir() if p.is_dir()):
        files = [f for f in cl.rglob("*.yaml") if f.name != "kustomization.yaml" and "prerequisites" not in f.parts]
        for f in cl.rglob("*.yaml"):
            try:
                load_docs(f.read_text())
            except yaml.YAMLError as e:
                fail(f"{name}/kubernetes/{cl.name}/{f.name}: invalid YAML: {e}")
        if (cl / "kustomization.yaml").exists():
            p = run(["kubectl", "kustomize", str(cl)])
            if p.returncode != 0:
                fail(f"{name}/kubernetes/{cl.name}: kustomize build failed: {p.stderr.strip()[-400:]}")
            else:
                ok(f"{name}/kubernetes/{cl.name}: kustomize builds ({len(load_docs(p.stdout))} objects)")
                kubeconform(stdin=p.stdout, label=f"{name}/kubernetes/{cl.name} (built)")
        elif files:
            kubeconform(files, label=f"{name}/kubernetes/{cl.name}")


def render_helmfile(cl: Path, label: str) -> None:
    hf = yaml.safe_load((cl / "helmfile.yaml").read_text())
    repos = {r["name"]: r for r in hf.get("repositories", [])}
    for rel in hf.get("releases", []):
        chart = rel["chart"]
        values = [cl / v for v in rel.get("values", [])]
        repo = None
        if chart.startswith("./"):
            args = ["helm", "template", rel["name"], str(cl / chart), "--namespace", rel["namespace"], "--include-crds"]
        else:
            alias, cname = chart.split("/", 1)
            r = repos[alias]
            repo = ("oci://" + r["url"]) if r.get("oci") else r["url"]
            args = helm_args(rel["name"], cname, repo, rel.get("version"), rel["namespace"], values)
        p = run(args)
        if p.returncode != 0:
            fail(f"{label} release {rel['name']}: helm template failed: {p.stderr.strip()[-600:]}")
            continue
        docs = load_docs(p.stdout, tolerant=True)
        add_crds(docs)
        ok(f"{label} release {rel['name']}: renders {len(split_docs(p.stdout))} object(s)")
        kubeconform(stdin=without_crds(p.stdout), label=f"{label} release {rel['name']}")


def check_helm(root: Path, name: str) -> None:
    for cl in sorted(p for p in root.iterdir() if p.is_dir()):
        if (cl / "helmfile.yaml").exists():
            render_helmfile(cl, f"{name}/helm/{cl.name}")


def check_terraform(root: Path, name: str) -> None:
    for cl in sorted(p for p in root.iterdir() if p.is_dir()):
        for stage in sorted(p for p in cl.iterdir() if p.is_dir()):
            label = f"{name}/terraform/{cl.name}/{stage.name}"
            tmp = WORK / "tf" / name / cl.name / stage.name
            shutil.copytree(stage, tmp)
            env = dict(os.environ, TF_IN_AUTOMATION="1")
            Path(env["TF_PLUGIN_CACHE_DIR"]).mkdir(parents=True, exist_ok=True)
            p = subprocess.run(["terraform", "init", "-backend=false", "-input=false", "-no-color"], cwd=tmp, capture_output=True, text=True, env=env)
            if p.returncode != 0:
                fail(f"{label}: terraform init failed: {p.stderr.strip()[-500:]}")
                continue
            p = subprocess.run(["terraform", "validate", "-no-color"], cwd=tmp, capture_output=True, text=True, env=env)
            if p.returncode != 0:
                fail(f"{label}: terraform validate failed: {(p.stdout + p.stderr).strip()[-800:]}")
            else:
                ok(f"{label}: terraform validate")
            p = subprocess.run(["terraform", "fmt", "-check", "-diff", "-no-color", "-recursive"], cwd=tmp, capture_output=True, text=True, env=env)
            if p.returncode != 0:
                fail(f"{label}: not canonically formatted (terraform fmt):\n{p.stdout[-800:]}")
            else:
                ok(f"{label}: terraform fmt")
            mf = tmp / "manifests"
            if mf.is_dir():
                kubeconform(sorted(mf.glob("*.yaml")), label=f"{label} manifests")


def self_test() -> None:
    """The validator must reject a misspelled field – otherwise a green run would prove nothing."""
    log("== self-test: the validator rejects wrong objects")
    bad = WORK / "bad.yaml"
    bad.write_text("apiVersion: postgresql.cnpg.io/v1\nkind: Cluster\nmetadata:\n  name: x\nspec:\n  instances: 1\n  instancess: 2\n")
    if kubeconform([bad], label="self-test", record=False):
        fail("self-test: kubeconform accepted a misspelled CRD field – the schemas are not strict")
    else:
        ok("a misspelled CRD field is rejected")


def main() -> int:
    if not OUT.is_dir() or not any(OUT.iterdir()):
        print(f"nothing to check in {OUT}", file=sys.stderr)
        return 2
    only = sys.argv[1:]
    collect_operator_crds()
    self_test()
    for ex in sorted(p for p in OUT.iterdir() if p.is_dir()):
        if only and ex.name not in only:
            continue
        log(f"== {ex.name}")
        if (ex / "kubernetes").is_dir():
            check_kubernetes(ex / "kubernetes", ex.name)
        if (ex / "helm").is_dir():
            check_helm(ex / "helm", ex.name)
        if (ex / "terraform").is_dir():
            check_terraform(ex / "terraform", ex.name)
    log("")
    log(f"{passed} checks passed, {len(failures)} failed")
    for f in failures:
        log(f"  - {f.splitlines()[0]}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
