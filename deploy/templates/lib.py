"""A small language for writing reference architectures: zones, nodes and links with the conventions of the model, and the same JSON the engine reads.

A template is a module `t_<id>.py` that builds an `Arch` and exposes it as `TEMPLATE`; `build.py` writes `backend/crates/engine/defaults/templates/<id>.json`
from it. What the model asks of a design (rules, hygiene of backups and links) is easier to keep right in code than in a thousand lines of JSON: the
helpers below carry the properties every replicated service needs.
"""
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE.parent.parent / "backend/crates/engine/defaults/templates"


class Arch:
    def __init__(self, id, name, description, badge, order, tags, requirements, title, about):
        """`name` / `description` are the pairs (ru, en) shown in the gallery; `title` / `about` are the Russian name and notes the design carries itself."""
        self.id, self.name, self.description = id, name, description
        self.badge, self.order, self.tags = badge, order, list(tags)
        self.requirements, self.title, self.about = requirements, title, about
        self.zones, self.nodes, self.edges = [], [], []
        self._ids = set()

    # ------------------------------------------------------------------------------------------------------------------ building
    def _unique(self, kind, key):
        if (kind, key) in self._ids:
            raise ValueError(f"{self.id}: duplicate {kind} {key!r}")
        self._ids.add((kind, key))

    def zone(self, id, name, type, parent=None, role=None, **props):
        self._unique("zone", id)
        self.zones.append({"id": id, "name": name, "type": type, "parentId": parent, "role": role, "props": props})
        return id

    def node(self, id, kind, name, zone=None, group=None, role=None, **props):
        self._unique("node", id)
        self.nodes.append({"id": id, "kind": kind, "name": name, "zoneId": zone, "group": group, "role": role, "props": props})
        return id

    def edge(self, source, target, type="traffic", label=None, **props):
        base = f"e-{source}--{target}"
        key, n = base, 1
        while ("edge", key) in self._ids:
            n += 1
            key = f"{base}-{n}"
        self._unique("edge", key)
        self.edges.append({"id": key, "source": source, "target": target, "type": type, "label": label, "props": props})
        return key

    # ------------------------------------------------------------------------------------------------------------------ links that carry what the rules look for
    def chain(self, *ids, **props):
        """Traffic along a line of nodes."""
        for a, b in zip(ids, ids[1:]):
            self.edge(a, b, "traffic", **props)

    def call(self, source, target, fanout=None, **props):
        """A request path to a dependency: `fanout` is how many calls one request makes (1 = every request, 0.3 = three in ten)."""
        if fanout is not None:
            props["fanout"] = fanout
        self.edge(source, target, "traffic", **props)

    def sync(self, primary, replica, ms=0.8, mbps=25000, paths=3, label=None):
        self.edge(primary, replica, "sync", label, latencyMs=ms, latencyMeasured=True, tls=True, bandwidthMbps=mbps, paths=paths)

    def async_(self, primary, replica, lag=1, ms=0.8, mbps=25000, paths=3, label=None, **extra):
        self.edge(primary, replica, "async", label, lagSeconds=lag, latencyMs=ms, latencyMeasured=True, tls=True, paths=paths, bandwidthMbps=mbps, **extra)

    def quorum(self, *ids, ms=0.8, paths=3):
        """Every voter sees every other: the links of a consensus group."""
        for i, a in enumerate(ids):
            for b in ids[i + 1:]:
                self.edge(a, b, "quorum", latencyMs=ms, tls=True, paths=paths)

    def backup(self, source, target, every=300, label=None):
        self.edge(source, target, "backup", label, intervalSeconds=every, tls=True)

    def control(self, source, target, tls=True):
        self.edge(source, target, "control", **({"tls": True} if tls else {}))

    # ------------------------------------------------------------------------------------------------------------------ output
    def document(self):
        return {
            "id": self.id,
            "name": {"ru": self.name[0], "en": self.name[1]},
            "description": {"ru": self.description[0], "en": self.description[1]},
            "badge": self.badge,
            "tags": self.tags,
            "enabled": True,
            "order": self.order,
            "architecture": {
                "schemaVersion": 1,
                "meta": {"name": self.title, "description": self.about},
                "requirements": self.requirements,
                "zones": self.zones,
                "nodes": self.nodes,
                "edges": self.edges,
            },
        }

    def text(self):
        return json.dumps(self.document(), indent=1, ensure_ascii=False) + "\n"


def requirements(rps, growth, rto, rpo, budget, tolerance, slo, **extra):
    r = {"rpsPeak": rps, "growthPercent": growth, "rtoSeconds": rto, "rpoSeconds": rpo, "budgetMonthly": budget, "failureTolerance": tolerance, "availabilitySlo": slo}
    r.update(extra)
    return r


# ---------------------------------------------------------------------------------------------------------------------- properties of the services every design has
def store(**p):
    """What a datastore carries so that the rules about encryption and pooling have an answer."""
    return {"encryptionAtRest": True, **p}


def backup_props(**p):
    """The storage of copies: immutable, kept long enough, restore tested, encrypted."""
    return {"immutable": True, "retentionDays": 30, "backupIntervalSeconds": 300, "restoreSeconds": 3600, "restoreTestedDays": 30, "encryptionAtRest": True, "diskGb": 10000, **p}
