"""deploy/iac-check/check.py: the parts of the IaC check that do not need Helm, Terraform or a network - which charts are pulled and from where, what is rendered from the
cache, how a CRD becomes a strict schema. (That the check validates the generated code against the real upstream artifacts is what the `iac` job of ci.yml and
`make iac-check` run, with the real tools.) Run: python3 -m unittest discover -s deploy/tests"""
import contextlib
import importlib.util
import json
import os
import pathlib
import io
import tempfile
import textwrap
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent

_cache = tempfile.TemporaryDirectory()
os.environ["IAC_CACHE"] = _cache.name
os.environ["IAC_OUT"] = _cache.name
os.environ["DEFAULTS"] = str(DEPLOY.parent / "backend" / "crates" / "engine" / "defaults")
spec = importlib.util.spec_from_file_location("iac_check", DEPLOY / "iac-check" / "check.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

CRD = textwrap.dedent("""\
    apiVersion: apiextensions.k8s.io/v1
    kind: CustomResourceDefinition
    metadata:
      name: widgets.example.com
    spec:
      group: example.com
      names:
        kind: Widget
      versions:
        - name: v1
          schema:
            openAPIV3Schema:
              type: object
              properties:
                spec:
                  type: object
                  properties:
                    size:
                      x-kubernetes-int-or-string: true
                    name:
                      type: string
                      default: x
""")


class Charts(unittest.TestCase):
    def test_a_chart_has_one_file_in_the_cache_per_repository_name_and_version(self):
        a = check.Chart("https://charts.example.com/", "web", "1.2.3")
        b = check.Chart("https://other.example.com", "web", "1.2.3")
        self.assertEqual(a.file(), check.CHARTS / "charts.example.com" / "web-1.2.3.tgz")
        self.assertNotEqual(a.file(), b.file())
        self.assertEqual(check.Chart("https://x.io", "cert-manager", "v1.21.2").file().name, "cert-manager-v1.21.2.tgz")

    def test_an_unpinned_chart_has_no_file(self):
        self.assertIsNone(check.Chart("https://x.io", "web", None).file())

    def test_a_repository_and_a_registry_are_named_differently_to_helm(self):
        self.assertEqual(check.Chart("https://x.io/charts", "web", "1").ref(), ["web", "--repo", "https://x.io/charts"])
        self.assertEqual(check.Chart("oci://ghcr.io/org/charts/", "web", "1").ref(), ["oci://ghcr.io/org/charts/web"])

    def test_the_slug_of_a_url_is_a_safe_directory_name(self):
        slug = check.Chart("https://example.com:8443/a b/c?x=1", "web", "1").slug()
        self.assertRegex(slug, r"^[A-Za-z0-9._-]+$")

    def test_a_chart_in_the_cache_is_not_pulled_again(self):
        c = check.Chart("https://x.io", "web", "1.0.0")
        c.file().parent.mkdir(parents=True, exist_ok=True)
        c.file().write_bytes(b"chart")
        self.assertEqual(check.pull_chart(c), (True, "cached"))
        self.assertEqual(c.file().read_bytes(), b"chart")

    def test_what_is_no_longer_pinned_is_removed_from_the_cache_and_what_is_pinned_stays(self):
        keep = check.Chart("https://x.io", "web", "2.0.0")
        old = check.Chart("https://x.io", "web", "1.0.0")
        other = check.Chart("https://gone.io", "db", "3.0.0")
        for c in (keep, old, other):
            c.file().parent.mkdir(parents=True, exist_ok=True)
            c.file().write_bytes(b"x")
        removed = check.prune_charts({keep.file()})
        self.assertEqual(sorted(p.name for p in removed), ["db-3.0.0.tgz", "web-1.0.0.tgz"])
        self.assertTrue(keep.file().exists())
        self.assertFalse(old.file().exists())
        self.assertFalse(other.file().parent.exists(), "the directory of the removed repository stays empty")


class Templates(unittest.TestCase):
    def test_a_pulled_chart_is_rendered_from_its_file(self):
        c = check.Chart("https://x.io", "web", "1.0.0")
        args = check.template_args("rel", c, "ns", [pathlib.Path("/v/a.yaml"), pathlib.Path("/v/b.yaml")], {c})
        self.assertEqual(args, ["helm", "template", "rel", str(c.file()), "--namespace", "ns", "--include-crds", "-f", "/v/a.yaml", "-f", "/v/b.yaml"])
        self.assertNotIn("--repo", args)

    def test_a_pinned_chart_that_could_not_be_pulled_is_not_fetched_again_for_every_release(self):
        c = check.Chart("https://x.io", "web", "1.0.0")
        self.assertIsNone(check.template_args("rel", c, "ns", [], set()))

    def test_an_unpinned_chart_is_rendered_from_where_it_lives(self):
        c = check.Chart("https://x.io", "web", None)
        self.assertEqual(check.template_args("rel", c, "ns", [], set()), ["helm", "template", "rel", "web", "--repo", "https://x.io", "--namespace", "ns", "--include-crds"])

    def test_the_same_release_with_the_same_values_is_rendered_once(self):
        with tempfile.TemporaryDirectory() as d:
            d = pathlib.Path(d)
            (d / "a.yaml").write_text("x: 1\n")
            (d / "b.yaml").write_text("x: 1\n")
            (d / "c.yaml").write_text("x: 2\n")
            c = check.Chart("https://x.io", "web", "1.0.0")
            args = lambda f: check.template_args("rel", c, "ns", [d / f], {c})
            key = lambda f: check.render_key(args(f), [d / f])
            self.assertEqual(key("a.yaml"), key("b.yaml"))
            self.assertNotEqual(key("a.yaml"), key("c.yaml"))


class Helmfiles(unittest.TestCase):
    def test_releases_are_read_with_their_chart(self):
        with tempfile.TemporaryDirectory() as d:
            d = pathlib.Path(d)
            (d / "helmfile.yaml").write_text(textwrap.dedent("""\
                repositories:
                  - name: repo1
                    url: https://argoproj.github.io/argo-helm
                  - name: repo2
                    url: ghcr.io/org/charts
                    oci: true
                releases:
                  - name: local
                    namespace: default
                    chart: ./charts/local
                  - name: gitops
                    namespace: platform
                    chart: repo1/argo-cd
                    version: "10.10.3"
                  - name: registry
                    namespace: platform
                    chart: repo2/thing
                    version: "0.1.0"
                """))
            found = check.read_helmfile(d)
        self.assertEqual([r["name"] for r, _ in found], ["local", "gitops", "registry"])
        self.assertIsNone(found[0][1])
        self.assertEqual(found[1][1], check.Chart("https://argoproj.github.io/argo-helm", "argo-cd", "10.10.3"))
        self.assertEqual(found[2][1], check.Chart("oci://ghcr.io/org/charts", "thing", "0.1.0"))

    def test_the_operators_of_the_engine_are_charts_that_can_be_pulled(self):
        charts = [check.operator_chart(op) for op in check.operators().values()]
        self.assertGreaterEqual(len(charts), 5)
        self.assertTrue(all(c.version and c.file() for c in charts), "an operator that is not pinned is rendered from the repository every time")


class Schemas(unittest.TestCase):
    def test_a_crd_becomes_a_strict_schema_and_is_written_whole(self):
        docs = check.load_docs(CRD)
        self.assertEqual(check.add_crds(docs), 1)
        target = check.SCHEMAS / "example.com" / "widget_v1.json"
        schema = json.loads(target.read_text())
        self.assertFalse(schema["properties"]["spec"]["additionalProperties"], "an unknown field must be an error")
        self.assertNotIn("default", schema["properties"]["spec"]["properties"]["name"])
        self.assertEqual(schema["properties"]["spec"]["properties"]["size"]["oneOf"], [{"type": "integer"}, {"type": "string"}])
        self.assertEqual([p.name for p in target.parent.iterdir()], ["widget_v1.json"], "no temporary file is left beside the schema")

    def test_only_what_can_be_a_crd_is_parsed(self):
        text = "kind: ConfigMap\ndata: {a: [unclosed\n---\n" + CRD
        self.assertEqual(len(check.load_docs(text, tolerant=True, only_crds=True)), 1)
        with self.assertRaises(Exception):
            check.load_docs(text, tolerant=False)  # without the filter the broken document is parsed, and refused

    def test_crds_are_taken_out_of_what_kubeconform_sees(self):
        out = check.without_crds("kind: ConfigMap\n---\n" + CRD + "---\nkind: Service\n")
        self.assertIn("kind: ConfigMap", out)
        self.assertIn("kind: Service", out)
        self.assertNotIn("CustomResourceDefinition", out)


class Reports(unittest.TestCase):
    def test_reports_are_merged_in_the_order_they_are_added(self):
        a, b = check.Report(), check.Report()
        a.log("== a")
        a.ok("one")
        b.log("== b")
        b.fail("two broke\nmore")
        total = check.Total()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            for r in (b, a):  # whatever finished first, the log reads in the order they are added
                total.add(r)
        self.assertEqual(out.getvalue().splitlines(), ["== b", "  FAIL two broke", "more", "== a", "  ok   one"])
        self.assertEqual((total.passed, total.failures), (1, ["two broke\nmore"]))


if __name__ == "__main__":
    unittest.main()
