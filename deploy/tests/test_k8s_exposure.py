"""deploy/check-k8s-exposure.py: the metrics port of deploy/k8s stays off every path the Ingress can take. Each test spoils the real manifests in one way."""
import copy
import importlib.util
import pathlib
import subprocess
import sys
import unittest

import yaml

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
K8S = DEPLOY / "k8s"
spec = importlib.util.spec_from_file_location("check_k8s_exposure", DEPLOY / "check-k8s-exposure.py")
exposure = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exposure)


def shipped():
    """The documents `kustomize build deploy/k8s` renders (the resources of kustomization.yaml; the namespace it adds changes nothing that is checked)."""
    resources = yaml.safe_load((K8S / "kustomization.yaml").read_text())["resources"]
    docs = []
    for name in resources:
        docs += [d for d in yaml.safe_load_all((K8S / name).read_text()) if d]
    return docs


def of(docs, kind):
    return next(d for d in docs if d["kind"] == kind)


def broken(change):
    docs = copy.deepcopy(shipped())
    change(docs)
    return exposure.check(docs)


def policy_rules(docs):
    return of(docs, "NetworkPolicy")["spec"]["ingress"]


class MetricsPortStaysPrivate(unittest.TestCase):
    def assert_flagged(self, problems, *words):
        text = "\n".join(problems)
        self.assertTrue(problems, "the manifests were spoiled and the check said nothing")
        for word in words:
            self.assertIn(word, text)

    def test_the_manifests_we_ship_pass(self):
        self.assertEqual(exposure.check(shipped()), [])

    def test_the_script_reads_a_file_and_stdin_and_exits_with_the_verdict(self):
        import tempfile

        run = lambda path, stdin=None: subprocess.run([sys.executable, str(DEPLOY / "check-k8s-exposure.py"), path], input=stdin, capture_output=True, text=True)
        good = yaml.safe_dump_all(shipped())
        with tempfile.TemporaryDirectory() as d:
            f = pathlib.Path(d) / "all.yaml"
            f.write_text(good)
            ok = run(str(f))
            self.assertEqual(ok.returncode, 0, ok.stdout + ok.stderr)
            self.assertIn("ok", ok.stdout)
        self.assertEqual(run("-", good).returncode, 0)
        docs = copy.deepcopy(shipped())
        of(docs, "Service")["spec"]["ports"].append({"name": "metrics", "port": 9090, "targetPort": "metrics"})
        bad = run("-", yaml.safe_dump_all(docs))
        self.assertEqual(bad.returncode, 1)
        self.assertIn("Service `infra-architect`", bad.stdout)

    def test_a_service_that_forwards_the_metrics_port_is_refused(self):
        # by the name of the container port, by its number, and without a targetPort (which then equals the port)
        for port in [{"name": "metrics", "port": 9090, "targetPort": "metrics"}, {"name": "m", "port": 9100, "targetPort": 9090}, {"name": "m", "port": 9090}]:
            problems = broken(lambda docs: of(docs, "Service")["spec"]["ports"].append(port))
            self.assert_flagged(problems, "Service `infra-architect`", "metrics port")

    def test_the_application_port_of_the_service_may_not_end_at_the_metrics_port(self):
        problems = broken(lambda docs: of(docs, "Service")["spec"]["ports"][0].update({"targetPort": "metrics"}))
        self.assert_flagged(problems, "Service `infra-architect`")
        # ... and the Ingress that sends to that port is flagged for leading there, whatever name the port has
        self.assert_flagged(problems, "Ingress `infra-architect`")

    def test_an_ingress_backend_on_the_metrics_port_is_refused(self):
        for port in [{"name": "metrics"}, {"number": 9090}]:
            def change(docs, port=port):
                of(docs, "Ingress")["spec"]["rules"][0]["http"]["paths"][0]["backend"]["service"]["port"] = port

            self.assert_flagged(broken(change), "Ingress `infra-architect`", "metrics port")

    def test_a_default_backend_is_looked_at_too(self):
        def change(docs):
            of(docs, "Ingress")["spec"]["defaultBackend"] = {"service": {"name": "infra-architect", "port": {"name": "metrics"}}}

        self.assert_flagged(broken(change), "Ingress `infra-architect`")

    def test_a_host_port_publishes_the_metrics_port_on_the_node(self):
        def change(docs):
            [p for p in of(docs, "Deployment")["spec"]["template"]["spec"]["containers"][0]["ports"] if p["name"] == "metrics"][0]["hostPort"] = 9090

        self.assert_flagged(broken(change), "hostPort")

    def test_the_network_policy_old_shape_that_admits_monitoring_on_the_application_port_is_refused(self):
        def change(docs):
            rules = policy_rules(docs)
            rules[0]["from"].append(rules[1]["from"][0])  # monitoring may now also reach 8080

        self.assert_flagged(broken(change), "both the application's port 8080 and the metrics port 9090")

    def test_the_ingress_controller_on_the_metrics_port_is_refused(self):
        def change(docs):
            rules = policy_rules(docs)
            rules[1]["from"].append(rules[0]["from"][0])

        self.assert_flagged(broken(change), "both the application's port")

    def test_no_rule_for_the_metrics_port_means_nobody_can_scrape(self):
        self.assert_flagged(broken(lambda docs: policy_rules(docs).pop()), "no NetworkPolicy rule admits the metrics port 9090")

    def test_a_rule_for_everybody_is_refused(self):
        for who in [None, [{"namespaceSelector": {}}], [{"ipBlock": {"cidr": "0.0.0.0/0"}}]]:
            def change(docs, who=who):
                rule = policy_rules(docs)[1]
                if who is None:
                    del rule["from"]
                else:
                    rule["from"] = who

            self.assert_flagged(broken(change), "everybody")

    def test_a_rule_without_ports_admits_every_port_to_its_callers(self):
        def change(docs):
            del policy_rules(docs)[0]["ports"]  # the ingress controller may now reach every port, 9090 too

        self.assert_flagged(broken(change), "both the application's port")

    def test_no_network_policy_at_all_is_refused(self):
        self.assert_flagged(broken(lambda docs: docs.remove(of(docs, "NetworkPolicy"))), "no NetworkPolicy")

    def test_metrics_port_unset_or_not_a_port_in_the_config_map_is_refused(self):
        for value in [None, "", "90a0", "0", "70000"]:
            def change(docs, value=value):
                data = of(docs, "ConfigMap")["data"]
                if value is None:
                    del data["METRICS_PORT"]
                else:
                    data["METRICS_PORT"] = value

            self.assert_flagged(broken(change), "METRICS_PORT")

    def test_the_container_port_and_the_scrape_annotation_follow_the_config_map(self):
        def other_number(docs):
            of(docs, "ConfigMap")["data"]["METRICS_PORT"] = "9191"

        self.assert_flagged(broken(other_number), "no container port named `metrics`")

        def annotation(docs):
            of(docs, "Deployment")["spec"]["template"]["metadata"]["annotations"]["prometheus.io/port"] = "8080"

        self.assert_flagged(broken(annotation), "prometheus.io/port")

    def test_the_example_files_are_not_rendered_so_they_are_not_judged(self):
        resources = yaml.safe_load((K8S / "kustomization.yaml").read_text())["resources"]
        self.assertNotIn("podmonitor.example.yaml", resources)
        # the PodMonitor example scrapes the port by name, which the container port has
        monitor = yaml.safe_load((K8S / "podmonitor.example.yaml").read_text())
        self.assertEqual(monitor["spec"]["podMetricsEndpoints"][0]["port"], "metrics")


if __name__ == "__main__":
    unittest.main()
