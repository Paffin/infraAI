#!/usr/bin/env python3
"""The metrics port of the Kubernetes manifests stays off the public path.

    kustomize build deploy/k8s | python3 deploy/check-k8s-exposure.py -        # or a file instead of -

`/metrics` has a port of its own (`METRICS_PORT` in the ConfigMap, a container port named `metrics`) so that what forwards the application's port, the Service
and through it the Ingress, cannot forward it: a NetworkPolicy tells ports apart, but not two paths on one port. This reads the rendered manifests (the
output of `kustomize build`, several YAML documents) and exits 1, with what is wrong, when any of that is undone:

  * the ConfigMap does not set METRICS_PORT, or the Deployment has no container port `metrics` with that number, or the scrape annotation names another port;
  * a Service forwards to the metrics port (by number or by name, also through a `targetPort` that is the name of the port), or an Ingress backend leads to it
    (by the Service's port name or number, resolved to the container port it ends at), or the container port is published on the node (`hostPort`);
  * the NetworkPolicy has no rule for the metrics port, admits it to anyone the application's port is also admitted to, or to "everybody" (a rule without
    `from`, an empty `namespaceSelector`, an `ipBlock`).

It is the CI's test of the manifests (kubeconform checks the schema, and nothing else): the job `manifests` of .github/workflows/ci.yml, and
deploy/tests/test_k8s_exposure.py, which gives it a manifest that does each of these things. Needs PyYAML (`pip install pyyaml`).
"""
import sys

try:
    import yaml
except ImportError:  # pragma: no cover
    sys.exit("PyYAML is needed: pip install pyyaml")

APP_PORT_NAME = "http"
METRICS_PORT_NAME = "metrics"


def by_kind(docs, kind):
    return [d for d in docs if isinstance(d, dict) and d.get("kind") == kind]


def containers(deployment):
    return deployment.get("spec", {}).get("template", {}).get("spec", {}).get("containers", [])


def sources_of(rule):
    """Who a NetworkPolicy ingress rule admits, as comparable strings; `*` stands for everybody."""
    sources = rule.get("from") or []
    if not sources:
        return {"*"}
    out = set()
    for source in sources:
        parts = []
        if "namespaceSelector" in source:
            labels = (source["namespaceSelector"] or {}).get("matchLabels") or {}
            expressions = (source["namespaceSelector"] or {}).get("matchExpressions") or []
            if not labels and not expressions:
                out.add("*")
                continue
            parts.append("ns:" + ",".join(f"{k}={v}" for k, v in sorted(labels.items())) + ("+expr" if expressions else ""))
        if "podSelector" in source:
            labels = (source["podSelector"] or {}).get("matchLabels") or {}
            parts.append("pod:" + ",".join(f"{k}={v}" for k, v in sorted(labels.items())))
        if "ipBlock" in source:
            cidr = source["ipBlock"].get("cidr", "")
            parts.append("ip:" + cidr)
            if cidr in ("0.0.0.0/0", "::/0"):
                out.add("*")
        out.add("&".join(parts) if parts else "*")
    return out


def check(docs):
    """What is wrong with the rendered manifests, as lines to print."""
    problems = []
    configmaps = [c for c in by_kind(docs, "ConfigMap") if c["metadata"]["name"] == "infra-architect"]
    deployments = by_kind(docs, "Deployment")
    if not configmaps or not deployments:
        return ["the ConfigMap or the Deployment `infra-architect` is not in the manifests (is this the output of `kustomize build deploy/k8s`?)"]
    raw = str((configmaps[0].get("data") or {}).get("METRICS_PORT", "")).strip()
    if not raw.isdigit() or not 0 < int(raw) < 65536:
        return [f"the ConfigMap sets METRICS_PORT to {raw!r}: unset or not a port, /metrics is then served on the application's own port, which the Ingress forwards"]
    metrics_port = int(raw)

    # the Deployment: the container port, and where the scrape is pointed
    ports = [p for c in containers(deployments[0]) for p in c.get("ports", [])]
    named = {p.get("name"): p.get("containerPort") for p in ports if p.get("name")}
    if named.get(METRICS_PORT_NAME) != metrics_port:
        problems.append(f"the Deployment has no container port named `{METRICS_PORT_NAME}` with the number METRICS_PORT sets ({metrics_port}); it has {named}")
    app_port = named.get(APP_PORT_NAME)
    if app_port is None:
        problems.append(f"the Deployment has no container port named `{APP_PORT_NAME}`")
    if app_port == metrics_port:
        problems.append(f"the metrics port {metrics_port} is the application's port")
    for p in ports:
        if p.get("containerPort") == metrics_port and p.get("hostPort"):
            problems.append(f"the metrics port is published on the node (hostPort {p['hostPort']})")
    annotations = deployments[0].get("spec", {}).get("template", {}).get("metadata", {}).get("annotations", {}) or {}
    if str(annotations.get("prometheus.io/scrape")).lower() == "true" and str(annotations.get("prometheus.io/port")) != str(metrics_port):
        problems.append(f"prometheus.io/port is {annotations.get('prometheus.io/port')!r}, the metrics are on {metrics_port}")
    metrics_names = {name for name, number in named.items() if number == metrics_port}

    def container_port_of(target):
        """The number a Service's targetPort ends at: a number is itself, a name is a container port's number."""
        if isinstance(target, int):
            return target
        if isinstance(target, str) and target.isdigit():
            return int(target)
        return named.get(target)

    services = {s["metadata"]["name"]: s for s in by_kind(docs, "Service")}
    for name, service in services.items():
        for port in service.get("spec", {}).get("ports", []):
            target = container_port_of(port.get("targetPort", port.get("port")))
            if target == metrics_port:
                problems.append(f"Service `{name}` forwards port {port.get('port')} ({port.get('name', 'unnamed')}) to the metrics port {metrics_port}: whatever reaches the Service reaches /metrics")

    # an Ingress backend names a Service and one of its ports (by name or number)
    def backend_ports(ingress):
        spec = ingress.get("spec", {})
        backends = [spec["defaultBackend"]] if spec.get("defaultBackend") else []
        for rule in spec.get("rules", []):
            for path in (rule.get("http") or {}).get("paths", []):
                backends.append(path.get("backend", {}))
        return backends

    def service_port(service, wanted):
        """The entry of the Service's `ports` that an Ingress backend's port (a name or a number) selects."""
        for p in (service or {}).get("spec", {}).get("ports", []):
            if wanted.get("name") and p.get("name") == wanted["name"]:
                return p
            if wanted.get("number") and p.get("port") == wanted["number"]:
                return p
        return None

    for ingress in by_kind(docs, "Ingress"):
        for backend in backend_ports(ingress):
            service = backend.get("service", {})
            wanted = service.get("port", {})
            where = f"Ingress `{ingress['metadata']['name']}` -> Service `{service.get('name')}` port {wanted}"
            if wanted.get("name") in metrics_names or wanted.get("number") == metrics_port:
                problems.append(f"{where}: that is the metrics port")
                continue
            port = service_port(services.get(service.get("name")), wanted)
            if port is not None and container_port_of(port.get("targetPort", port.get("port"))) == metrics_port:
                problems.append(f"{where} ends at the metrics port {metrics_port}")

    # the NetworkPolicy: the metrics port is admitted to its scraper and to nobody the application's port is admitted to
    policies = by_kind(docs, "NetworkPolicy")
    admitted = {}  # container port number -> who
    for policy in policies:
        if "Ingress" not in (policy.get("spec", {}).get("policyTypes") or []):
            continue
        for rule in policy.get("spec", {}).get("ingress", []):
            rule_ports = [p.get("port") for p in rule.get("ports", [])] or ["*"]
            for p in rule_ports:
                numbers = list(named.values()) if p == "*" else [container_port_of(p)]
                for number in numbers:
                    admitted.setdefault(number, set()).update(sources_of(rule))
    if not policies:
        problems.append("there is no NetworkPolicy: every pod in the cluster can reach the metrics port")
    elif metrics_port not in admitted:
        problems.append(f"no NetworkPolicy rule admits the metrics port {metrics_port}: Prometheus cannot scrape it")
    else:
        who = admitted[metrics_port]
        if "*" in who:
            problems.append(f"the NetworkPolicy admits the metrics port {metrics_port} to everybody ({sorted(who)})")
        shared = who & admitted.get(app_port, set()) if app_port else set()
        if shared:
            problems.append(f"the NetworkPolicy admits {sorted(shared)} to both the application's port {app_port} and the metrics port {metrics_port}: the two ports have different callers (ingress controller, monitoring)")
        if "*" in admitted.get(app_port, set()):
            problems.append(f"the NetworkPolicy admits everybody to the application's port {app_port}")
    return problems


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__)
    text = sys.stdin.read() if argv[0] == "-" else open(argv[0], encoding="utf-8").read()
    docs = [d for d in yaml.safe_load_all(text) if d]
    problems = check(docs)
    for line in problems:
        print(f"check-k8s-exposure: {line}")
    if problems:
        return 1
    print("check-k8s-exposure: ok: the metrics port is not published by any Service or Ingress, and the NetworkPolicy gives it a caller of its own")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
