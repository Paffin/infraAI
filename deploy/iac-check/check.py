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

How it is fast (the same checks, a third of the time; nothing is skipped):
  * every chart (repository, name, version) is pulled ONCE into $IAC_CACHE/charts and rendered from the file. `helm template --repo …` fetches and parses the repository's
    index (6 MB for prometheus-community) on every call, warm cache or not: 135 renders spent 277 of the 366 seconds there, and a cache directory alone does not change it
  * the cache directory ($IAC_CACHE, default /cache) is meant to outlive the container: CI restores it between runs (charts, Terraform providers, Kubernetes schemas)
  * the work is done in phases: charts → CRDs of the operators → every Helm render, in parallel → CRDs of everything rendered, in a fixed order → the validation of the
    examples, in parallel ($IAC_JOBS workers, default 4). A custom resource may be checked against the CRD of a chart that another example renders, so no validation starts
    before every CRD is known, and the log is printed in the order of the examples whatever the order of finishing
  * kubeconform runs one at a time (its schema cache is a plain directory that two processes would write at once) and `terraform init` runs one at a time (the plugin
    cache likewise); renders, kustomize, `validate` and `fmt` are the parallel part
"""
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import NamedTuple

import yaml

OUT = Path(os.environ.get("IAC_OUT", "/out"))
DEFAULTS = Path(os.environ.get("DEFAULTS", "/defaults"))
K8S_VERSION = os.environ.get("K8S_VERSION", "1.32.0")
CACHE = Path(os.environ.get("IAC_CACHE", "/cache"))
JOBS = max(1, int(os.environ.get("IAC_JOBS", "4")))
PULL_ATTEMPTS = 3
WORK = Path(tempfile.mkdtemp(prefix="iac-check-"))
SCHEMAS = WORK / "schemas"
SCHEMAS.mkdir()
CHARTS = CACHE / "charts"
KUBECONFORM_CACHE = CACHE / "kubeconform"

KUBECONFORM_LOCK = threading.Lock()
TERRAFORM_INIT_LOCK = threading.Lock()
_spent: dict[str, float] = {}
_spent_lock = threading.Lock()


def timed(what: str, fn, *args, **kwargs):
    """Run fn and add its wall time to `what`: the last lines of the log say where the time went (summed over the workers, so it can exceed the wall time)."""
    t0 = time.monotonic()
    try:
        return fn(*args, **kwargs)
    finally:
        with _spent_lock:
            _spent[what] = _spent.get(what, 0.0) + time.monotonic() - t0


class Report:
    """What one piece of work says: its lines, how many checks passed, which failed. Pieces run in threads and are merged in a fixed order."""

    def __init__(self) -> None:
        self.lines: list[str] = []
        self.passed = 0
        self.failures: list[str] = []

    def log(self, msg: str) -> None:
        self.lines.append(msg)

    def fail(self, msg: str) -> None:
        self.failures.append(msg)
        self.lines.append(f"  FAIL {msg}")

    def ok(self, msg: str) -> None:
        self.passed += 1
        self.lines.append(f"  ok   {msg}")


class Total:
    """The whole run: reports are added in order and printed as they are added."""

    def __init__(self) -> None:
        self.passed = 0
        self.failures: list[str] = []

    def add(self, rep: Report) -> None:
        for line in rep.lines:
            print(line, flush=True)
        self.passed += rep.passed
        self.failures += rep.failures


def run(cmd: list[str], cwd: Path | None = None, check: bool = False) -> subprocess.CompletedProcess:
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if check and p.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)}\n{p.stderr.strip()[-1500:]}")
    return p


class Chart(NamedTuple):
    """A Helm chart as the engine pins it: where it is, what it is called, and the version (None: not pinned, then it is rendered from the repository every time)."""

    repo: str  # https://… for a chart repository, oci://… for a registry
    name: str
    version: str | None

    def slug(self) -> str:
        return re.sub(r"[^A-Za-z0-9._-]+", "_", self.repo.removeprefix("https://").removeprefix("http://").rstrip("/"))

    def file(self) -> Path | None:
        if not self.version:
            return None
        return CHARTS / self.slug() / f"{self.name}-{re.sub(r'[^A-Za-z0-9._+-]+', '_', self.version)}.tgz"

    def ref(self) -> list[str]:
        """The arguments that name this chart for `helm pull` / `helm template` when it is fetched from where it lives."""
        if self.repo.startswith("oci://"):
            return [self.repo.rstrip("/") + "/" + self.name]
        return [self.name, "--repo", self.repo]


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
            name = target / f"{kind.lower()}_{v['name']}.json"
            tmp = target / f".{name.name}.tmp"
            tmp.write_text(json.dumps(js))
            os.replace(tmp, name)  # a schema is never read half-written
            n += 1
    return n


def split_docs(text: str) -> list[str]:
    return [d for d in re.split(r"(?m)^---\s*$", text) if d.strip()]


def load_docs(text: str, tolerant: bool = False, only_crds: bool = False) -> list[dict]:
    """Parsed documents. `tolerant` skips documents PyYAML (YAML 1.1) cannot read – chart internals
    that are not ours and not CRDs. `only_crds` does not even parse the documents that cannot be a
    CustomResourceDefinition (PyYAML is slow, and a chart renders megabytes)."""
    out = []
    for raw in split_docs(text):
        if only_crds and "CustomResourceDefinition" not in raw:
            continue
        try:
            d = yaml.safe_load(raw)
        except yaml.YAMLError:
            if not tolerant:
                raise
            continue
        if isinstance(d, dict):
            out.append(d)
    return out


_seen_crds: set[str] = set()


def new_crds(text: str) -> list[dict]:
    """The CustomResourceDefinitions in a rendered chart that no earlier render had. Twenty renders of one chart carry the same CRDs, and PyYAML parses a megabyte of
    them in seconds: a document that is the same text as one already taken is not parsed again (called from one thread, in a fixed order)."""
    fresh = []
    for raw in split_docs(text):
        if "CustomResourceDefinition" not in raw:
            continue
        digest = hashlib.sha1(raw.encode()).hexdigest()
        if digest in _seen_crds:
            continue
        _seen_crds.add(digest)
        fresh.append(raw)
    return load_docs("\n---\n".join(fresh), tolerant=True)


def without_crds(text: str) -> str:
    """The rendered output minus CustomResourceDefinitions (upstream content, not ours)."""
    keep = [d for d in split_docs(text) if not re.search(r"(?m)^kind:\s*CustomResourceDefinition\s*$", d)]
    return "\n---\n".join(keep)


# ------------------------------------------------------------------------------------------ charts
def pull_chart(chart: Chart) -> tuple[bool, str]:
    """(ok, what to say): the chart is in the cache afterwards. Pulled once per (repository, name, version), kept between runs; a download that fails is tried again
    (a repository answers 502 now and then)."""
    target = chart.file()
    assert target is not None
    if target.is_file() and target.stat().st_size > 0:
        return True, "cached"
    target.parent.mkdir(parents=True, exist_ok=True)
    last = ""
    for attempt in range(1, PULL_ATTEMPTS + 1):
        dest = Path(tempfile.mkdtemp(prefix="pull-", dir=target.parent))
        try:
            p = timed("helm pull", run, ["helm", "pull", *chart.ref(), "--version", chart.version, "--destination", str(dest)])
            got = sorted(dest.glob("*.tgz"))
            if p.returncode == 0 and len(got) == 1:
                os.replace(got[0], target)  # the name in the cache is the one asked for, whatever the chart calls its own version
                return True, "pulled" if attempt == 1 else f"pulled (attempt {attempt})"
            last = p.stderr.strip()[-400:] or f"helm pull wrote {len(got)} files"
        finally:
            shutil.rmtree(dest, ignore_errors=True)
        if attempt < PULL_ATTEMPTS:
            time.sleep(2 * attempt)
    return False, last


def pull_charts(charts: list[Chart], rep: Report, keep_all: bool) -> set[Chart]:
    """Pull every pinned chart once. Charts of one repository go one after the other (helm keeps the repository's index in a file that two pulls would write at once),
    the repositories in parallel. Returns the charts that are in the cache; the others are reported as failures."""
    pinned = sorted({c for c in charts if c.version}, key=lambda c: (c.repo, c.name, c.version))
    rep.log(f"== Helm charts: {len(pinned)} pinned chart(s) pulled once into {CHARTS}, every render below is from these files")
    by_repo: dict[str, list[Chart]] = {}
    for c in pinned:
        by_repo.setdefault(c.repo, []).append(c)

    def work(group: list[Chart]) -> list[tuple[Chart, bool, str]]:
        return [(c, *pull_chart(c)) for c in group]

    have: set[Chart] = set()
    with concurrent.futures.ThreadPoolExecutor(max_workers=JOBS) as pool:
        results = [r for group in pool.map(work, [by_repo[k] for k in sorted(by_repo)]) for r in group]
    for c, ok, what in sorted(results):
        if ok:
            have.add(c)
            rep.log(f"  chart {c.name} {c.version} ({c.repo}): {what}")
        else:
            rep.fail(f"chart {c.name} {c.version} ({c.repo}): helm pull failed: {what}")
    if not keep_all:
        prune_charts({c.file() for c in pinned})
    return have


def prune_charts(wanted: set) -> list[Path]:
    """Remove the cached charts that are not `wanted` (and the directories that stay empty); returns what was removed. A cache restored from the last run would
    otherwise carry every version the engine ever pinned."""
    removed = []
    if not CHARTS.is_dir():
        return removed
    for f in sorted(CHARTS.rglob("*.tgz")):
        if f not in wanted:
            f.unlink()
            removed.append(f)
    for d in sorted((p for p in CHARTS.rglob("*") if p.is_dir()), reverse=True):
        if not any(d.iterdir()):
            d.rmdir()
    return removed


def template_args(name: str, chart: Chart, namespace: str, values: list[Path], have: set[Chart]) -> list[str] | None:
    """`helm template` for a chart: from the cached file when there is one, from where the chart lives when it is not pinned; None when it is pinned and could not be pulled."""
    if chart.version and chart in have:
        args = ["helm", "template", name, str(chart.file())]
    elif chart.version:
        return None
    else:
        args = ["helm", "template", name, *chart.ref()]
    args += ["--namespace", namespace, "--include-crds"]
    for v in values:
        args += ["-f", str(v)]
    return args


# ------------------------------------------------------------------------------------- the engine's pins
class Release(NamedTuple):
    label: str  # "<example>/helm/<cluster> release <name>"
    args: list[str] | None  # None: the chart could not be pulled
    chart: Chart | None  # None: a local chart


def operators() -> dict:
    return json.loads((DEFAULTS / "engine.json").read_text())["export"]["k8s"]["operators"]


def operator_chart(op: dict) -> Chart:
    return Chart(op["repo"], op["chart"], op.get("version"))


def read_helmfile(cl: Path) -> list[tuple[dict, Chart | None]]:
    """The releases of a helmfile and the chart of each (None: a local chart, `./…`)."""
    hf = yaml.safe_load((cl / "helmfile.yaml").read_text())
    repos = {r["name"]: r for r in hf.get("repositories", [])}
    out = []
    for rel in hf.get("releases", []):
        chart = rel["chart"]
        if chart.startswith("./"):
            out.append((rel, None))
            continue
        alias, cname = chart.split("/", 1)
        r = repos[alias]
        out.append((rel, Chart(("oci://" + r["url"]) if r.get("oci") else r["url"], cname, rel.get("version"))))
    return out


def helmfile_dirs(examples: list[Path]) -> list[tuple[Path, str]]:
    found = []
    for ex in examples:
        root = ex / "helm"
        if root.is_dir():
            for cl in sorted(p for p in root.iterdir() if p.is_dir() and (p / "helmfile.yaml").exists()):
                found.append((cl, f"{ex.name}/helm/{cl.name}"))
    return found


def render_key(args: list[str], values: list[Path]) -> tuple:
    """Two releases with the same chart, name, namespace and the same values (by content) render the same: the second is not rendered."""
    plain = [a for i, a in enumerate(args) if a != "-f" and not (i and args[i - 1] == "-f")]
    return (tuple(plain), tuple(hashlib.sha1(v.read_bytes()).hexdigest() for v in values))


# ------------------------------------------------------------------------------------ CRD schemas of the operators
def collect_operator_crds(rep: Report, have: set[Chart]) -> None:
    rep.log("== CRD schemas of the pinned operators (engine.json → export.k8s.operators)")
    for oid, op in operators().items():
        values = []
        if op.get("values"):
            f = WORK / f"op-{oid}.values.yaml"
            f.write_text(yaml.safe_dump(op["values"]))
            values = [f]
        args = template_args(op.get("release") or oid, operator_chart(op), op.get("namespace") or oid, values, have)
        if args is None:
            rep.fail(f"operator {oid}: its chart could not be pulled (see above)")
            continue
        p = timed("helm template", run, args)
        if p.returncode != 0:
            rep.fail(f"operator {oid}: helm template failed: {p.stderr.strip()[-400:]}")
            continue
        n = add_crds(new_crds(p.stdout))
        rep.ok(f"operator {oid} {op.get('version')}: {n} CRD schema(s)")


# ----------------------------------------------------------------------------------- kubeconform
_kubeconform_memo: dict[str, tuple[bool, str, list[str], int]] = {}


def kubeconform(rep: Report, files: list[Path] | None = None, stdin: str | None = None, label: str = "", record: bool = True) -> bool:
    cmd = [
        "kubeconform", "-strict", "-summary", "-output", "json", "-kubernetes-version", K8S_VERSION,
        "-schema-location", "default",
        "-schema-location", str(SCHEMAS) + "/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json",
    ]
    if KUBECONFORM_CACHE.is_dir():
        cmd += ["-cache", str(KUBECONFORM_CACHE)]
    # one kubeconform at a time: the schemas it downloads are kept in a directory that two processes would write at once. The same input gives the same answer.
    key = hashlib.sha1(stdin.encode()).hexdigest() if stdin is not None else None
    with KUBECONFORM_LOCK:
        if key is not None and key in _kubeconform_memo:
            ran = _kubeconform_memo[key]
        else:
            if stdin is not None:
                p = timed("kubeconform", subprocess.run, cmd + ["-"], input=stdin, capture_output=True, text=True)
            else:
                p = timed("kubeconform", run, cmd + [str(f) for f in files or []])
            try:
                res = json.loads(p.stdout or "{}")
            except json.JSONDecodeError:
                ran = (False, f"kubeconform produced no result: {p.stderr.strip()[-300:] or p.stdout[-300:]}", [], 0)
            else:
                summ = res.get("summary", {})
                bad = [r for r in res.get("resources", []) if r.get("status") not in ("statusValid", "statusSkipped")]
                lines = [f"{r.get('filename', '')} {r.get('kind', '')}/{r.get('name', '')}: {r.get('status')} {r.get('msg', '')[:300]}" for r in bad[:8]]
                ran = (not (bad or summ.get("invalid") or summ.get("errors")), "", lines, summ.get("valid", 0))
            if key is not None:
                _kubeconform_memo[key] = ran
    good, error, lines, valid = ran
    if error:
        if record:
            rep.fail(f"{label}: {error}")
        return False
    if not good:
        if record:
            for line in lines:
                rep.fail(f"{label}: {line}")
        return False
    if record:
        rep.ok(f"{label}: {valid} object(s) valid")
    return True


# --------------------------------------------------------------------------------------- formats
def check_kubernetes(rep: Report, root: Path, name: str) -> None:
    for cl in sorted(p for p in root.iterdir() if p.is_dir()):
        files = [f for f in cl.rglob("*.yaml") if f.name != "kustomization.yaml" and "prerequisites" not in f.parts]
        for f in cl.rglob("*.yaml"):
            try:
                load_docs(f.read_text())
            except yaml.YAMLError as e:
                rep.fail(f"{name}/kubernetes/{cl.name}/{f.name}: invalid YAML: {e}")
        if (cl / "kustomization.yaml").exists():
            p = timed("kubectl kustomize", run, ["kubectl", "kustomize", str(cl)])
            if p.returncode != 0:
                rep.fail(f"{name}/kubernetes/{cl.name}: kustomize build failed: {p.stderr.strip()[-400:]}")
            else:
                rep.ok(f"{name}/kubernetes/{cl.name}: kustomize builds ({len(load_docs(p.stdout))} objects)")
                kubeconform(rep, stdin=p.stdout, label=f"{name}/kubernetes/{cl.name} (built)")
        elif files:
            kubeconform(rep, files, label=f"{name}/kubernetes/{cl.name}")


def check_helm(rep: Report, root: Path, name: str, rendered: dict) -> None:
    """The releases of every helmfile, rendered earlier (`rendered`), checked against the schemas."""
    for cl in sorted(p for p in root.iterdir() if p.is_dir()):
        if not (cl / "helmfile.yaml").exists():
            continue
        label = f"{name}/helm/{cl.name}"
        for i, (rel, _) in enumerate(read_helmfile(cl)):
            res = rendered[(label, i)]
            if res is None:
                rep.fail(f"{label} release {rel['name']}: its chart could not be pulled (see above)")
            elif res.returncode != 0:
                rep.fail(f"{label} release {rel['name']}: helm template failed: {res.stderr.strip()[-600:]}")
            else:
                rep.ok(f"{label} release {rel['name']}: renders {len(split_docs(res.stdout))} object(s)")
                kubeconform(rep, stdin=without_crds(res.stdout), label=f"{label} release {rel['name']}")


_tf_seeds: dict[str, str] = {}  # what a stage asks of the providers -> the dependency lock file the first `init` with that request wrote
_tf_failed: dict[str, str] = {}  # the same key -> why the providers could not be installed


def provider_key(stage: Path) -> str | None:
    """What a stage asks of the providers (its `required_providers` blocks, whitespace aside); stages with the same text resolve to the same providers."""
    blocks = []
    for f in sorted(stage.glob("*.tf")):
        text = f.read_text()
        for m in re.finditer(r"required_providers\s*\{", text):
            depth, i = 1, m.end()
            while i < len(text) and depth:
                depth += {"{": 1, "}": -1}.get(text[i], 0)
                i += 1
            blocks.append(re.sub(r"\s+", " ", text[m.start():i]))
    return hashlib.sha1("\n".join(blocks).encode()).hexdigest() if blocks else None


def terraform_dirs(examples: list[Path]) -> list[tuple[Path, str]]:
    return [(stage, f"{ex.name}/terraform/{cl.name}/{stage.name}")
            for ex in examples if (ex / "terraform").is_dir()
            for cl in sorted(p for p in (ex / "terraform").iterdir() if p.is_dir())
            for stage in sorted(p for p in cl.iterdir() if p.is_dir())]


def terraform_init_command() -> list[str]:
    return ["terraform", "init", "-backend=false", "-input=false", "-no-color"]


def seed_terraform(examples: list[Path], rep: Report) -> None:
    """Install the providers, once for each distinct request, before anything runs in parallel. `terraform init` resolves the request against the registry and writes the
    provider into the plugin cache; a provider that is executed (`validate`) while another init rewrites its file in the cache fails with 'text file busy', so the writing is
    done here, one at a time, and the dependency lock file each of these inits wrote is kept. Every stage is then initialised with the lock of its request: it takes the
    same providers from the cache (1 s instead of 5 s in this sandbox) and writes nothing. Each distinct request is still resolved against the real registry, once."""
    rep.log("== Terraform providers: installed once for each distinct set of requirements (the stages with the same set reuse the lock file of the first)")
    env = dict(os.environ, TF_IN_AUTOMATION="1")
    Path(env["TF_PLUGIN_CACHE_DIR"]).mkdir(parents=True, exist_ok=True)
    for stage, label in terraform_dirs(examples):
        key = provider_key(stage)
        if key is None or key in _tf_seeds or key in _tf_failed:
            continue
        scratch = WORK / "tf-seed" / key
        shutil.copytree(stage, scratch)
        p = timed("terraform init", subprocess.run, terraform_init_command(), cwd=scratch, capture_output=True, text=True, env=env)
        lock = scratch / ".terraform.lock.hcl"
        if p.returncode == 0 and lock.is_file():
            _tf_seeds[key] = lock.read_text()
            rep.log(f"  providers of {label}: installed")
        else:
            _tf_failed[key] = f"{label}: {p.stderr.strip()[-500:]}"
            rep.fail(f"{label}: terraform init failed: {p.stderr.strip()[-500:]}")


def check_terraform(rep: Report, root: Path, name: str) -> None:
    for cl in sorted(p for p in root.iterdir() if p.is_dir()):
        for stage in sorted(p for p in cl.iterdir() if p.is_dir()):
            label = f"{name}/terraform/{cl.name}/{stage.name}"
            tmp = WORK / "tf" / name / cl.name / stage.name
            shutil.copytree(stage, tmp)
            env = dict(os.environ, TF_IN_AUTOMATION="1")
            Path(env["TF_PLUGIN_CACHE_DIR"]).mkdir(parents=True, exist_ok=True)
            key = provider_key(tmp)
            if key in _tf_failed:
                rep.fail(f"{label}: terraform init failed: the providers it asks for could not be installed (the first stage with the same requirements, {_tf_failed[key].splitlines()[0]})")
                continue
            if key in _tf_seeds:
                (tmp / ".terraform.lock.hcl").write_text(_tf_seeds[key])
                p = timed("terraform init", subprocess.run, terraform_init_command(), cwd=tmp, capture_output=True, text=True, env=env)
            else:  # no providers asked for, or `seed_terraform` did not run (a single stage checked by hand): the plugin cache is written by `init`, one at a time
                with TERRAFORM_INIT_LOCK:
                    p = timed("terraform init", subprocess.run, terraform_init_command(), cwd=tmp, capture_output=True, text=True, env=env)
            if p.returncode != 0:
                rep.fail(f"{label}: terraform init failed: {p.stderr.strip()[-500:]}")
                continue
            p = timed("terraform validate", subprocess.run, ["terraform", "validate", "-no-color"], cwd=tmp, capture_output=True, text=True, env=env)
            if p.returncode != 0:
                rep.fail(f"{label}: terraform validate failed: {(p.stdout + p.stderr).strip()[-800:]}")
            else:
                rep.ok(f"{label}: terraform validate")
            p = timed("terraform fmt", subprocess.run, ["terraform", "fmt", "-check", "-diff", "-no-color", "-recursive"], cwd=tmp, capture_output=True, text=True, env=env)
            if p.returncode != 0:
                rep.fail(f"{label}: not canonically formatted (terraform fmt):\n{p.stdout[-800:]}")
            else:
                rep.ok(f"{label}: terraform fmt")
            mf = tmp / "manifests"
            if mf.is_dir():
                kubeconform(rep, sorted(mf.glob("*.yaml")), label=f"{label} manifests")


def validate_example(ex: Path, rendered: dict) -> Report:
    rep = Report()
    rep.log(f"== {ex.name}")
    if (ex / "kubernetes").is_dir():
        check_kubernetes(rep, ex / "kubernetes", ex.name)
    if (ex / "helm").is_dir():
        check_helm(rep, ex / "helm", ex.name, rendered)
    if (ex / "terraform").is_dir():
        check_terraform(rep, ex / "terraform", ex.name)
    return rep


def self_test(rep: Report) -> None:
    """The validator must reject a misspelled field – otherwise a green run would prove nothing."""
    rep.log("== self-test: the validator rejects wrong objects")
    bad = WORK / "bad.yaml"
    bad.write_text("apiVersion: postgresql.cnpg.io/v1\nkind: Cluster\nmetadata:\n  name: x\nspec:\n  instances: 1\n  instancess: 2\n")
    if kubeconform(rep, [bad], label="self-test", record=False):
        rep.fail("self-test: kubeconform accepted a misspelled CRD field – the schemas are not strict")
    else:
        rep.ok("a misspelled CRD field is rejected")


def render_all(examples: list[Path], have: set[Chart]) -> dict:
    """Render every release of every helmfile (in parallel, each distinct one once), then take the CRDs of what was rendered into the schemas, in the order of the
    helmfiles. Returns {(label, index of the release in its helmfile): CompletedProcess, or None when its chart could not be pulled}."""
    plan = []  # (label, index, key of the render)
    jobs: dict[tuple, list[str]] = {}
    unavailable = set()
    for cl, label in helmfile_dirs(examples):
        for i, (rel, chart) in enumerate(read_helmfile(cl)):
            if chart is None:  # a local chart (a directory beside the helmfile) is rendered with its own defaults, as it always was
                args = ["helm", "template", rel["name"], str(cl / rel["chart"]), "--namespace", rel["namespace"], "--include-crds"]
                key = (tuple(args), ())
            else:
                values = [cl / v for v in rel.get("values", [])]
                args = template_args(rel["name"], chart, rel["namespace"], values, have)
                if args is None:
                    unavailable.add((label, i))
                    continue
                key = render_key(args, values)
            jobs.setdefault(key, args)
            plan.append((label, i, key))
    keys = list(jobs)
    with concurrent.futures.ThreadPoolExecutor(max_workers=JOBS) as pool:
        done = dict(zip(keys, pool.map(lambda k: timed("helm template", run, jobs[k]), keys)))
    rendered: dict = {k: None for k in unavailable}
    for label, i, key in plan:
        rendered[(label, i)] = done[key]
    for key in keys:  # the CRDs in a fixed order: the same schema file never depends on which render finished first
        if done[key].returncode == 0:
            add_crds(new_crds(done[key].stdout))
    return rendered


def main() -> int:
    if not OUT.is_dir() or not any(OUT.iterdir()):
        print(f"nothing to check in {OUT}", file=sys.stderr)
        return 2
    only = sys.argv[1:]
    examples = sorted(p for p in OUT.iterdir() if p.is_dir() and (not only or p.name in only))
    started = time.monotonic()
    CACHE.mkdir(parents=True, exist_ok=True)
    KUBECONFORM_CACHE.mkdir(parents=True, exist_ok=True)
    total = Total()

    def phase(title: str) -> None:
        print(f"-- {title} ({time.monotonic() - started:.0f}s since the start)", flush=True)

    charts = [operator_chart(op) for op in operators().values()]
    for cl, _ in helmfile_dirs(examples):
        charts += [c for _, c in read_helmfile(cl) if c is not None]
    rep = Report()
    have = pull_charts(charts, rep, keep_all=bool(only))
    total.add(rep)
    phase("charts are in the cache")

    rep = Report()
    collect_operator_crds(rep, have)
    self_test(rep)
    total.add(rep)

    rendered = render_all(examples, have)
    phase("every release is rendered, every CRD is known")

    rep = Report()
    seed_terraform(examples, rep)
    total.add(rep)
    phase("the Terraform providers are installed")

    with concurrent.futures.ThreadPoolExecutor(max_workers=JOBS) as pool:
        for rep in pool.map(lambda ex: validate_example(ex, rendered), examples):
            total.add(rep)

    print("", flush=True)
    print(f"-- done in {time.monotonic() - started:.0f}s; time in each tool, summed over the {JOBS} workers: " + ", ".join(f"{k} {v:.0f}s" for k, v in sorted(_spent.items(), key=lambda kv: -kv[1])), flush=True)
    print(f"{total.passed} checks passed, {len(total.failures)} failed", flush=True)
    for f in total.failures:
        print(f"  - {f.splitlines()[0]}", flush=True)
    return 1 if total.failures else 0


if __name__ == "__main__":
    sys.exit(main())
