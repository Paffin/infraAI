#!/usr/bin/env python3
"""The rules the workflows of this repository keep, as a check that fails the `docs` job when one is broken.

    python3 deploy/check-workflows.py [root]        exit 1 and `file:line: [job] what` for each break

A workflow is code that runs with the repository's token and, sometimes, its secrets, on every push and every pull request; a mistake in it is found by a run that
is red for twenty minutes (twice, on main, it was). So what can be stated as a rule is one:

 pins         every `uses:` is `owner/repo@<40 hex> # vX.Y.Z` (a tag can be moved), or a local `./` action (deploy/pin-actions.py writes them)
 timeouts     every job has `timeout-minutes` (the default is six hours of a hung browser or a lost download); a job that calls a reusable workflow cannot have one: the
              jobs of the called file do
 permissions  every workflow says `permissions:` at its top and none of it is write; a job may ask for write only where ALLOWED_WRITERS lists it (publishing the image,
              the release tables' commit, the issue the daily security run opens), and a job with `contents: write` runs no package manager, no build tool, no repository
              script and no action of anyone but GitHub's own: whatever it runs holds the token that can push to main
 checkout     every `actions/checkout` has `persist-credentials: false` (the token must not stay in .git/config for the steps after it), except in the jobs that push
 pip          every `pip install` is `-r <file>` or `pkg==version`
 injection    no `${{ github.event... }}`, `github.head_ref` or `inputs.` inside `run:` (a branch name is a command there); put it in `env:`
 concurrency  `cancel-in-progress` is never the literal `true`: a push to main must not cancel the run of the commit before it (that commit is tested and may be published)
 graph        in the workflow that has `ci-ok` (the one check to require in the repository settings): it waits for EVERY other job (a job it does not wait for gates nothing),
              `publish` waits for `ci-ok`, and the jobs listed in INDEPENDENT_OF do not wait for the jobs they are listed with (the image needs no IaC validation to be built)
 toolchain    the Rust version of the Dockerfile (ARG RUST_VERSION) is the default of both services of docker-compose.test.yml that use it; there is no rust-toolchain.toml
              under backend/ (rustup 1.28 does not install what a toolchain file names, so it breaks every build that has only `stable`; CI installs the Dockerfile's version)
"""
import pathlib
import re
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
# (workflow file, job) -> the permission scopes it may have as write
ALLOWED_WRITERS = {
    ("ci.yml", "publish"): {"packages"},
    ("ci.yml", "security"): {"issues"},  # the caller of security.yml grants what the called job `report` asks for; the job never runs from there
    ("security.yml", "report"): {"issues"},
    ("versions.yml", "commit"): {"contents"},
}
# jobs that push, and so keep the token in .git/config
KEEPS_CREDENTIALS = {("versions.yml", "commit")}
# a job with contents: write may use only these actions and run none of these words
WRITER_ACTIONS = ("actions/checkout", "actions/download-artifact")
WRITER_FORBIDDEN = re.compile(r"(?<![\w-])(cargo|rustup|rustc|npm|npx|pnpm|yarn|node|pip3?|pipx|make|docker|go|ruby|gem|bundle|curl|wget|sudo|bash\s+-c|sh\s+-c)(?![\w-])|python3?\s+(?!deploy/release-patch\.py\b)\S")
# job -> jobs it must not wait for
INDEPENDENT_OF = {"build": {"iac"}, "image-checks": {"iac"}, "e2e": {"iac"}}
PINNED = re.compile(r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")
VERSION_COMMENT = re.compile(r"#\s*v\d+\.\d+\.\d+\s*$")
UNTRUSTED = re.compile(r"\$\{\{[^}]*(github\.event\.|github\.head_ref|inputs\.)[^}]*\}\}")


class Problem:
    def __init__(self, file, line, job, what):
        self.file, self.line, self.job, self.what = file, line, job, what

    def __str__(self):
        return f"{self.file}:{self.line}: " + (f"[{self.job}] " if self.job else "") + self.what

    def key(self):
        return (self.file, self.line, self.job or "", self.what)


def key_of(mapping, name):
    """The value node of a key of a mapping node, and the line (from 1) the key is on."""
    if isinstance(mapping, yaml.MappingNode):
        for k, v in mapping.value:
            if k.value == name:
                return v, k.start_mark.line + 1
    return None, 0


def items(node):
    return node.value if isinstance(node, yaml.SequenceNode) else []


def write_scopes(perms):
    """The permission scopes that are write: {'contents'}; `write-all` is every one of them."""
    if perms == "write-all":
        return {"*"}
    if isinstance(perms, dict):
        return {k for k, v in perms.items() if v == "write"}
    return set()


def check_text(name, text, problems, composite=False):
    """The rules that one file keeps by itself."""
    try:
        data = yaml.safe_load(text)
        node = yaml.compose(text)
    except yaml.YAMLError as e:
        problems.append(Problem(name, getattr(getattr(e, "problem_mark", None), "line", 0) + 1, None, f"not valid YAML: {e}"))
        return {}
    if not isinstance(data, dict):
        problems.append(Problem(name, 1, None, "not a mapping"))
        return {}
    lines = text.splitlines()
    base = pathlib.PurePosixPath(name).name

    def check_steps(steps_node, steps, job, job_perms_write):
        for step_node, step in zip(items(steps_node), steps or []):
            if not isinstance(step, dict):
                continue
            uses = step.get("uses")
            if uses is not None:
                unode, uline = key_of(step_node, "uses")
                comment_ok = VERSION_COMMENT.search(lines[uline - 1]) if 0 < uline <= len(lines) else None
                if str(uses).startswith("docker://"):
                    if not re.search(r"@sha256:[0-9a-f]{64}$", str(uses)):
                        problems.append(Problem(name, uline, job, f"`uses: {uses}`: a docker image is pinned by its digest (@sha256:…), a tag moves"))
                elif not str(uses).startswith("./"):
                    if not PINNED.match(str(uses)):
                        problems.append(Problem(name, uline, job, f"`uses: {uses}` is pinned by a tag or a branch, not by a commit (python3 deploy/pin-actions.py)"))
                    elif not comment_ok:
                        problems.append(Problem(name, uline, job, f"`uses: {uses}` has no `# vX.Y.Z` comment, which is what Dependabot moves together with the commit"))
                if str(uses).startswith("actions/checkout@"):
                    persist = (step.get("with") or {}).get("persist-credentials")
                    if persist is not False and (base, job) not in KEEPS_CREDENTIALS:
                        problems.append(Problem(name, uline, job, "`actions/checkout` without `persist-credentials: false`: the token stays in .git/config for every later step"))
                if job_perms_write and not str(uses).startswith(WRITER_ACTIONS) and not str(uses).startswith("./"):
                    problems.append(Problem(name, uline, job, f"a job with `contents: write` uses `{str(uses).split('@')[0]}`, which is not GitHub's own: it would run with the token that can push"))
            run = step.get("run")
            if run is not None:
                rnode, rline = key_of(step_node, "run")
                for m in re.finditer(r"(?m)^.*(?:pip3?|python3?\s+-m\s+pip)\s+install\b.*$", str(run)):
                    cmd = m.group(0)
                    if not re.search(r"\s-r\s|\s--requirement\b|\S==\S", cmd + " "):
                        problems.append(Problem(name, rline, job, f"`{cmd.strip()}`: a pip install without `-r <file>` or `pkg==version` takes whatever is newest today"))
                if UNTRUSTED.search(str(run)):
                    problems.append(Problem(name, rline, job, "a value an outsider writes (`github.event…`, `github.head_ref`, `inputs.`) is inside `run:`: it is a shell command there; pass it through `env:`"))
                if job_perms_write:
                    for m in WRITER_FORBIDDEN.finditer(str(run)):
                        problems.append(Problem(name, rline, job, f"a job with `contents: write` runs `{m.group(0).strip()}`: whatever it runs holds the token that can push; do that in a job that only reads"))

    if composite:
        runs, _ = key_of(node, "runs")
        steps_node, _ = key_of(runs, "steps")
        check_steps(steps_node, (data.get("runs") or {}).get("steps"), "(composite action)", False)
        return {}

    # workflow level
    perms_node, pline = key_of(node, "permissions")
    if "permissions" not in data:
        problems.append(Problem(name, 1, None, "no `permissions:` at the top: the token gets whatever the repository setting says (write, unless it was changed)"))
    elif write_scopes(data["permissions"]):
        problems.append(Problem(name, pline, None, f"workflow-level `permissions` has write ({', '.join(sorted(write_scopes(data['permissions'])))}): every job would hold it; give it to the one job that needs it"))
    conc = data.get("concurrency")
    if isinstance(conc, dict) and conc.get("cancel-in-progress") is True:
        _, cline = key_of(key_of(node, "concurrency")[0], "cancel-in-progress")
        problems.append(Problem(name, cline, None, "`cancel-in-progress: true`: a push to main would cancel the run of the commit before it (use `${{ github.event_name == 'pull_request' }}`)"))
    jobs_node, _ = key_of(node, "jobs")
    jobs = data.get("jobs") or {}
    for jkey, jval in (jobs_node.value if jobs_node else []):
        job = jkey.value
        j = jobs.get(job) or {}
        jline = jkey.start_mark.line + 1
        calls = "uses" in j
        if calls:
            if not str(j["uses"]).startswith("./"):
                problems.append(Problem(name, jline, job, f"calls `{j['uses']}`: a reusable workflow of another repository is code that runs with this one's token; keep it in this repository"))
        elif "timeout-minutes" not in j:
            problems.append(Problem(name, jline, job, "no `timeout-minutes`: a hung step holds the runner for six hours"))
        pw = write_scopes(j.get("permissions"))
        if pw:
            allowed = ALLOWED_WRITERS.get((base, job), set())
            extra = pw - allowed
            if extra:
                _, wline = key_of(jval, "permissions")
                problems.append(Problem(name, wline or jline, job, f"write permission ({', '.join(sorted(extra))}) on a job that is not in ALLOWED_WRITERS of deploy/check-workflows.py"))
        contents_write = "contents" in pw or "*" in pw
        steps_node, _ = key_of(jval, "steps")
        check_steps(steps_node, j.get("steps"), job, contents_write)

    # the graph
    needs = {}
    for job, j in jobs.items():
        n = (j or {}).get("needs") or []
        needs[job] = {n} if isinstance(n, str) else set(n)
    for job, banned in INDEPENDENT_OF.items():
        if job in jobs and needs[job] & banned:
            node_, line_ = key_of(key_of(jobs_node, job)[0], "needs")
            problems.append(Problem(name, line_, job, f"waits for {', '.join(sorted(needs[job] & banned))}: it needs nothing of it, and the wait is minutes on the critical path"))
    if "ci-ok" in jobs:
        after = {"ci-ok"}  # the jobs that come after the verdict (publish) cannot be waited for by it
        while True:
            more = {job for job in jobs if needs[job] & after} - after
            if not more:
                break
            after |= more
        for job in jobs:
            if job not in after and job not in needs["ci-ok"]:
                problems.append(Problem(name, key_of(key_of(jobs_node, "ci-ok")[0], "needs")[1] or 1, "ci-ok", f"does not wait for `{job}`: a job that `ci-ok` does not wait for gates nothing"))
        if "publish" in jobs and "ci-ok" not in needs["publish"]:
            problems.append(Problem(name, key_of(key_of(jobs_node, "publish")[0], "needs")[1] or 1, "publish", "does not wait for `ci-ok`: it could publish an image that a job it does not list rejected"))
    return jobs


def check_toolchain(root, problems):
    dockerfile = root / "Dockerfile"
    compose = root / "docker-compose.test.yml"
    toml = root / "backend" / "rust-toolchain.toml"
    if toml.exists() or (root / "backend" / "rust-toolchain").exists():
        problems.append(Problem("backend/rust-toolchain.toml", 1, None, "a toolchain file under backend/: rustup 1.28 does not install the toolchain it names, so docs (cargo metadata), the Docker build and docker-compose.test.yml fail wherever only `stable` is installed. The version is the Dockerfile's `ARG RUST_VERSION`; CI installs it by name"))
    if not dockerfile.exists() or not compose.exists():
        return
    m = re.search(r"(?m)^ARG RUST_VERSION=(\S+)\s*$", dockerfile.read_text())
    if not m:
        problems.append(Problem("Dockerfile", 1, None, "no `ARG RUST_VERSION=` line: it is where CI reads the Rust version from"))
        return
    for n, line in enumerate(compose.read_text().splitlines(), 1):
        for found in re.findall(r"\$\{RUST_VERSION:-([^}]*)\}", line):
            if found != m.group(1):
                problems.append(Problem("docker-compose.test.yml", n, None, f"RUST_VERSION defaults to {found}, the Dockerfile's is {m.group(1)}: `make test` would test with another compiler than the one that builds the image"))


def check(root=ROOT):
    root = pathlib.Path(root)
    problems = []
    files = sorted((root / ".github" / "workflows").glob("*.y*ml"))
    for f in files:
        check_text(f"{f.relative_to(root).as_posix()}", f.read_text(), problems)
    for f in sorted((root / ".github" / "actions").glob("*/action.y*ml")):
        check_text(f.relative_to(root).as_posix(), f.read_text(), problems, composite=True)
    check_toolchain(root, problems)
    return sorted(problems, key=Problem.key), len(files)


def main(argv):
    root = pathlib.Path(argv[0]) if argv else ROOT
    problems, count = check(root)
    for p in problems:
        print(p)
    print(f"{count} workflow file(s), {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
