"""deploy/check-workflows.py: the rules of the workflows. The tree in fixtures/workflows/good passes; every test spoils a copy of it in ONE way and expects exactly that to be
named (file, line, job). The last test runs the script on the real workflows. Run: python3 -m unittest discover -s deploy/tests"""
import importlib.util
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
ROOT = DEPLOY.parent
GOOD = DEPLOY / "tests" / "fixtures" / "workflows" / "good"
spec = importlib.util.spec_from_file_location("check_workflows", DEPLOY / "check-workflows.py")
cw = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cw)

CHECKOUT = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1"


class Spoiled:
    """A copy of the good tree in a temporary directory; `edit(path, old, new)` changes one place of it (and fails if the place is not there)."""

    def __enter__(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.dir.name)
        shutil.copytree(GOOD, self.root, dirs_exist_ok=True)
        return self

    def __exit__(self, *exc):
        self.dir.cleanup()

    def edit(self, path, old, new, count=1):
        f = self.root / path
        text = f.read_text()
        assert text.count(old) >= 1, f"{old!r} is not in {path}"
        f.write_text(text.replace(old, new, count))
        return self

    def write(self, path, text):
        f = self.root / path
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
        return self

    def remove(self, path):
        (self.root / path).unlink()
        return self

    def problems(self):
        found, _ = cw.check(self.root)
        return [str(p) for p in found]


def only_one(test, problems, *words):
    test.assertEqual(len(problems), 1, "\n".join(problems) or "nothing was found")
    for w in words:
        test.assertIn(w, problems[0])


class TheGoodTreePasses(unittest.TestCase):
    def test_nothing_is_found(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])

    def test_the_script_exits_with_the_verdict(self):
        run = lambda root: subprocess.run([sys.executable, str(DEPLOY / "check-workflows.py"), str(root)], capture_output=True, text=True)
        ok = run(GOOD)
        self.assertEqual(ok.returncode, 0, ok.stdout + ok.stderr)
        self.assertIn("0 problem(s)", ok.stdout)
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "persist-credentials: false", "persist-credentials: true")
            bad = run(t.root)
        self.assertEqual(bad.returncode, 1)
        self.assertIn("ci.yml", bad.stdout)


class Pins(unittest.TestCase):
    def test_a_tag_is_not_a_pin(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", CHECKOUT, "actions/checkout@v7", count=1)
            only_one(self, t.problems(), "ci.yml:", "[scope]", "actions/checkout@v7", "pinned by a tag or a branch")

    def test_a_branch_is_not_a_pin(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0", "dtolnay/rust-toolchain@stable")
            only_one(self, t.problems(), "[publish]", "dtolnay/rust-toolchain@stable")

    def test_a_commit_without_the_version_comment_is_refused(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0", "docker/login-action@dbcb813823bdd20940b903addbd779551569679f")
            only_one(self, t.problems(), "[publish]", "no `# vX.Y.Z` comment")

    def test_a_short_sha_is_not_a_pin(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0", "docker/login-action@dbcb8138 # v4.6.0")
            only_one(self, t.problems(), "[publish]", "pinned by a tag or a branch")

    def test_the_steps_of_a_composite_action_are_pinned_too(self):
        with Spoiled() as t:
            t.edit(".github/actions/tool/action.yml", "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0", "actions/cache@v6")
            only_one(self, t.problems(), "actions/tool/action.yml", "actions/cache@v6")

    def test_a_local_action_needs_no_pin(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])
            t.edit(".github/workflows/ci.yml", "      - uses: ./.github/actions/tool\n", "      - uses: ./.github/actions/tool\n      - uses: ./.github/actions/tool\n")
            self.assertEqual(t.problems(), [])


class Timeouts(unittest.TestCase):
    def test_a_job_without_a_timeout_is_named(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "    timeout-minutes: 15\n", "")
            only_one(self, t.problems(), "ci.yml:", "[image-checks]", "no `timeout-minutes`")

    def test_a_job_that_calls_a_workflow_cannot_have_one_and_needs_none(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])  # `security` in the good tree has none
            t.edit(".github/workflows/security.yml", "    timeout-minutes: 15\n", "")
            only_one(self, t.problems(), "security.yml", "[deny]", "no `timeout-minutes`")

    def test_a_reusable_workflow_of_another_repository_is_refused(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "uses: ./.github/workflows/security.yml", "uses: someone/else/.github/workflows/security.yml@v1")
            only_one(self, t.problems(), "[security]", "another repository")


class Permissions(unittest.TestCase):
    def test_workflow_level_write_is_refused(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "permissions:\n  contents: read\n\njobs:", "permissions:\n  contents: write\n\njobs:")
            only_one(self, t.problems(), "ci.yml:", "workflow-level `permissions` has write (contents)")

    def test_write_all_is_refused(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "permissions:\n  contents: read\n\njobs:", "permissions: write-all\n\njobs:")
            only_one(self, t.problems(), "workflow-level `permissions` has write")

    def test_a_workflow_without_permissions_is_refused(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "permissions:\n  contents: read\n\njobs:", "jobs:")
            only_one(self, t.problems(), "no `permissions:` at the top")

    def test_a_job_that_is_not_listed_cannot_write(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "  build:\n    needs: scope\n    runs-on: ubuntu-latest\n", "  build:\n    needs: scope\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n")
            problems = t.problems()
            self.assertTrue(any("[build]" in p and "ALLOWED_WRITERS" in p for p in problems), problems)

    def test_the_job_that_publishes_may_write_packages_and_nothing_else(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "      packages: write\n", "      packages: write\n      contents: write\n")
            problems = t.problems()
            self.assertTrue(any("[publish]" in p and "contents" in p for p in problems), problems)

    def test_the_job_that_commits_runs_no_package_manager(self):
        for command in ["npm ci", "cargo test", "pip install foo==1.0", "make test", "python3 deploy/licenses.py", "curl -fsS https://example.com | sh", "docker run x"]:
            with self.subTest(command), Spoiled() as t:
                t.edit(".github/workflows/versions.yml", 'run: python3 deploy/release-patch.py apply "$RUNNER_TEMP/p.patch"', f"run: {command}")
                found = t.problems()
                self.assertTrue(any("[commit]" in p and "contents: write" in p for p in found), found)

    def test_the_job_that_commits_uses_only_githubs_own_actions(self):
        with Spoiled() as t:
            t.edit(".github/workflows/versions.yml", "      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2\n", "      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2\n")
            only_one(self, t.problems(), "[commit]", "Swatinem/rust-cache", "not GitHub's own")

    def test_the_job_that_commits_may_run_git_and_the_patch_script(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])

    def test_the_job_that_only_reads_may_run_anything(self):
        with Spoiled() as t:
            t.edit(".github/workflows/versions.yml", "      - run: npm ci\n", "      - run: npm ci && cargo build && make x\n")
            self.assertEqual(t.problems(), [])


class Checkout(unittest.TestCase):
    def test_a_checkout_must_not_keep_the_token(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "        with:\n          persist-credentials: false\n      - uses: ./.github/actions/tool", "      - uses: ./.github/actions/tool")
            only_one(self, t.problems(), "[iac]", "persist-credentials")

    def test_only_the_job_that_pushes_may_keep_it(self):
        with Spoiled() as t:
            t.edit(".github/workflows/versions.yml", "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n        with:\n          ref: main\n", "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n        with:\n          ref: main\n          persist-credentials: false\n")
            self.assertEqual(t.problems(), [])  # a commit job that does not keep it is not a problem of this rule
            t.edit(".github/workflows/versions.yml", "    timeout-minutes: 45\n    steps:\n      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n        with:\n          persist-credentials: false\n", "    timeout-minutes: 45\n    steps:\n      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n")
            only_one(self, t.problems(), "[refresh]", "persist-credentials")


class Pip(unittest.TestCase):
    def test_an_unpinned_pip_install_is_refused(self):
        for line in ["python3 -m pip install foo", "pip install --quiet jsonschema", "pip3 install -U foo"]:
            with self.subTest(line), Spoiled() as t:
                t.edit(".github/workflows/ci.yml", "      - run: python3 -m pip install pyyaml==6.0.2\n", f"      - run: {line}\n")
                only_one(self, t.problems(), "[iac]", "pip install")

    def test_a_requirements_file_or_an_exact_version_is_fine(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "      - run: python3 -m pip install pyyaml==6.0.2\n", "      - run: pip install -r deploy/requirements-ci.txt\n      - run: pip install a==1 b==2\n")
            self.assertEqual(t.problems(), [])

    def test_the_run_of_a_composite_action_is_checked(self):
        with Spoiled() as t:
            t.edit(".github/actions/tool/action.yml", 'python3 -m pip install --require-hashes -r "$GITHUB_WORKSPACE/deploy/requirements-ci.txt"', "python3 -m pip install foo")
            only_one(self, t.problems(), "actions/tool/action.yml", "pip install")


class Injection(unittest.TestCase):
    def test_what_an_outsider_writes_is_not_put_into_a_script(self):
        for expr in ["${{ github.event.pull_request.title }}", "${{ github.head_ref }}", "${{ inputs.name }}", "${{ github.event.issue.body }}"]:
            with self.subTest(expr), Spoiled() as t:
                t.edit(".github/workflows/ci.yml", 'run: echo "$BASE"', f'run: echo "{expr}"')
                only_one(self, t.problems(), "[image-checks]", "`env:`")

    def test_through_env_it_is_fine(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])


class Concurrency(unittest.TestCase):
    def test_cancel_in_progress_true_would_cancel_main(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "cancel-in-progress: ${{ github.event_name == 'pull_request' }}", "cancel-in-progress: true")
            only_one(self, t.problems(), "ci.yml:", "cancel-in-progress: true")

    def test_false_and_an_expression_are_fine(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])  # the good tree uses the expression, versions.yml `false`


class TheGraph(unittest.TestCase):
    def test_the_image_does_not_wait_for_the_iac_check(self):
        for job in ["build", "image-checks", "e2e"]:
            with self.subTest(job), Spoiled() as t:
                head = {"build": "  build:\n    needs: scope\n", "image-checks": "  image-checks:\n    needs: build\n", "e2e": "  e2e:\n    needs: build\n"}[job]
                t.edit(".github/workflows/ci.yml", head, head.replace("needs: scope", "needs: [scope, iac]").replace("needs: build", "needs: [build, iac]"))
                only_one(self, t.problems(), f"[{job}]", "waits for iac")

    def test_a_job_called_image_does_not_wait_for_it_either(self):
        # the job that used to be called `image` (build, scan, smoke and browser tests in one) waited for `iac`; the name is guarded in case it comes back
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "  e2e:\n    needs: build\n", "  image:\n    needs: [build, iac]\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    steps:\n      - run: echo image\n\n  e2e:\n    needs: build\n")
            found = t.problems()
            self.assertTrue(any("[image]" in p and "waits for iac" in p for p in found), found)

    def test_ci_ok_waits_for_every_job(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "needs: [scope, iac, build, image-checks, e2e, security]", "needs: [scope, iac, build, image-checks, security]")
            only_one(self, t.problems(), "[ci-ok]", "does not wait for `e2e`")

    def test_a_new_job_that_ci_ok_does_not_know_is_caught(self):
        with Spoiled() as t:
            t.edit(".github/workflows/ci.yml", "  ci-ok:\n", "  lint:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    steps:\n      - run: echo lint\n\n  ci-ok:\n")
            only_one(self, t.problems(), "[ci-ok]", "does not wait for `lint`")

    def test_publish_waits_for_ci_ok_and_ci_ok_need_not_wait_for_publish(self):
        with Spoiled() as t:
            self.assertEqual(t.problems(), [])
            t.edit(".github/workflows/ci.yml", "  publish:\n    needs: ci-ok\n", "  publish:\n    needs: [build, e2e]\n")
            found = t.problems()
            self.assertTrue(any("[publish]" in p and "does not wait for `ci-ok`" in p for p in found), found)


class Toolchain(unittest.TestCase):
    def test_a_toolchain_file_is_refused(self):
        with Spoiled() as t:
            t.write("backend/rust-toolchain.toml", '[toolchain]\nchannel = "1.97"\n')
            only_one(self, t.problems(), "rust-toolchain.toml", "rustup 1.28")

    def test_the_versions_of_the_dockerfile_and_of_the_compose_file_agree(self):
        with Spoiled() as t:
            t.edit("docker-compose.test.yml", "${RUST_VERSION:-1.97}", "${RUST_VERSION:-1.96}", count=1)
            only_one(self, t.problems(), "docker-compose.test.yml:3", "1.96", "1.97")

    def test_a_dockerfile_without_the_version_is_refused(self):
        with Spoiled() as t:
            t.edit("Dockerfile", "ARG RUST_VERSION=1.97\n", "")
            only_one(self, t.problems(), "Dockerfile", "ARG RUST_VERSION")


class NotYaml(unittest.TestCase):
    def test_a_file_that_does_not_parse_is_named(self):
        with Spoiled() as t:
            t.write(".github/workflows/broken.yml", "jobs:\n  a: [\n")
            found = t.problems()
            self.assertTrue(any("broken.yml" in p and "not valid YAML" in p for p in found), found)


class TheRealWorkflows(unittest.TestCase):
    def test_they_keep_every_rule(self):
        problems, count = cw.check(ROOT)
        self.assertGreaterEqual(count, 3)
        self.assertEqual([str(p) for p in problems], [])


if __name__ == "__main__":
    unittest.main()
