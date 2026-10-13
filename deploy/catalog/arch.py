"""Archetypes for catalog authoring: prop packs, recommended versions, kind builders."""
import json
import os
from lib import *

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
ENGINE = f"{REPO}/backend/crates/engine"
VERSIONS_PATH = f"{ENGINE}/defaults/versions.json"
VERS = {s["id"]: s for s in json.load(open(VERSIONS_PATH, encoding="utf-8"))}
# "today" for the recommended lines is the day the release tables were taken: the build does not depend on the clock
TODAY = max(s["asOf"] for s in VERS.values())
USED_TABLES = set()


def _alive(c):
    e = c.get("eol")
    if e is True:
        return False
    if isinstance(e, str) and e <= TODAY:
        return False
    return True


def _days(a, b):
    import datetime as dt
    return (dt.date.fromisoformat(a) - dt.date.fromisoformat(b)).days


def rec(table, lts=True, mature=90):
    """The line the engine recommends (mirror of VersionSet::recommended): newest alive and out for `mature` days, long-term support first."""
    USED_TABLES.add(table)
    s = VERS.get(table)
    if not s:
        raise SystemExit(f"no version table {table!r}")
    cands = [c for c in s["cycles"] if _alive(c)]
    cands.sort(key=lambda c: (c.get("releaseDate", ""), c["cycle"]), reverse=True)
    old = [c for c in cands if not c.get("releaseDate") or _days(TODAY, c["releaseDate"]) >= mature]
    pool = old or cands
    if lts:
        for c in pool:
            if c.get("lts"):
                return c["cycle"]
    return pool[0]["cycle"] if pool else None


# ---- prop packs -----------------------------------------------------------------------------------------------------
def pv(table, ru="Версия", en="Version", default=None, help=None):
    d = default if default is not None else (rec(table) if table else None)
    return string("version", ru, en, default=d, group="platform", help=help)


def pr(default, ru="Экземпляры", en="Instances", maxv=None):
    return num("replicas", ru, en, min=1, max=maxv, default=default, group="capacity")


def res(vcpu, ram, disk=None):
    """vCPU / RAM / disk of one instance; a zero or missing size leaves the common property without a default."""
    out = []
    if vcpu:
        out.append(num("vcpu", "vCPU на экземпляр", "vCPU per instance", unit_="vCPU", min=0, group="capacity", default=vcpu))
    if ram:
        out.append(num("ramGb", "RAM на экземпляр", "RAM per instance", unit_="GB", min=0, group="capacity", default=ram))
    if disk:
        out.append(num("diskGb", "Диск на экземпляр", "Disk per instance", unit_="GB", min=0, group="capacity", default=disk))
    return out


def lb_props(replicas=2, vcpu=2, ram=4, version=None, host=None):
    out = [pr(replicas)]
    out += p_lb()
    out += res(vcpu, ram)
    return out


def sub_rep(extra="{version}"):
    return f"{{replicas}}× · {extra}" if extra else "{replicas}×"


def failover_enum(default="auto"):
    return enum("failover", "Переключение (failover)", "Failover", [("auto", "Автоматическое", "Automatic"), ("manual", "Ручное", "Manual"), ("none", "Нет", "None")], default=default, group="failover")


def failover_secs(default=30):
    return num("failoverSeconds", "Время автопереключения", "Automatic failover time", unit_="s", min=0, default=default, group="failover")


def dcs_prop(ru_help=None, en_help=None, variants=None):
    return prop("dcs", "unit_ref", "Хранилище кворума (DCS)", "Quorum store (DCS)", refBehavior="consensus", group="failover", variants=variants,
                help=(ru_help or "Кластер консенсуса (etcd, Consul, ZooKeeper), по которому выбирается лидер. Без DCS или witness автопереключение небезопасно (split-brain).",
                      en_help or "A consensus cluster (etcd, Consul, ZooKeeper) used for leader election. Without a DCS or witness automatic failover is unsafe (split-brain)."))


def votes_prop():
    return num("votes", "Голоса", "Votes", min=0, default=1, group="failover", advanced=True)


def db_props(table, vcpu, ram, disk, failover="auto", secs=30, dcs=True, votes=True, pooler=None, enc=True, version_default=None, extra=()):
    out = [pv(table, default=version_default), failover_enum(failover), failover_secs(secs)]
    if dcs:
        out.append(dcs_prop())
    if votes:
        out.append(votes_prop())
    if pooler:
        out.append(boolean("connectionPooler", f"Пулер соединений ({pooler})", f"Connection pooler ({pooler})", group="performance"))
    out += list(extra)
    if enc:
        out.append(p_enc())
    out += res(vcpu, ram, disk)
    return out


def cluster_props(table, replicas, rf, min_insync, vcpu, ram, disk, rf_ru="Replication factor", rf_en="Replication factor", mi_ru="min.insync.replicas", mi_en="min.insync.replicas", rep_ru="Узлы", rep_en="Nodes", enc=True, extra=(), version_default=None):
    out = [pv(table, default=version_default), pr(replicas, rep_ru, rep_en)]
    if rf:
        out.append(num("replicationFactor", rf_ru, rf_en, min=1, default=rf, group="durability"))
    if min_insync:
        out.append(num("minInsync", mi_ru, mi_en, min=1, default=min_insync, group="durability"))
    out += list(extra)
    if enc:
        out.append(p_enc())
    out += res(vcpu, ram, disk)
    return out


def app_props(table, replicas, vcpu, ram, disk=None, rep_ru="Экземпляры", rep_en="Instances", extra=(), version_default=None, with_version=True):
    out = []
    if with_version:
        out.append(pv(table, default=version_default))
    out.append(pr(replicas, rep_ru, rep_en))
    out += list(extra)
    out += res(vcpu, ram, disk)
    return out


def hosting_default(default):
    return enum("hosting", "Размещение", "Hosting", [
        ("auto", "Авто — в кластере Kubernetes зоны, если он есть", "Auto – on the zone’s Kubernetes cluster if there is one"),
        ("kubernetes", "Kubernetes", "Kubernetes"), ("vm", "Виртуальные машины", "Virtual machines"), ("managed", "Управляемый сервис облака", "Managed cloud service")],
        default=default, group="platform", advanced=True,
        help=("Определяет, создаются ли для узла манифесты Kubernetes при экспорте IaC.", "Decides whether Kubernetes manifests are generated for the node when exporting IaC."))


R_VOTER = [{"id": "voter", "name": L("Участник кворума", "Voter")}, {"id": "witness", "name": L("Witness", "Witness")}]


def R_PR(primary="Primary", replica=("Реплика", "Replica"), witness=("Witness (арбитр)", "Witness (arbiter)")):
    return roles_pr(primary, replica, witness)


def rb(failover=None, restore=None, verify=None):
    d = {}
    if failover:
        d["failover"] = [L(*x) for x in failover]
    if restore:
        d["restore"] = [L(*x) for x in restore]
    if verify:
        d["verify"] = [L(*x) for x in verify]
    return d or None


def k(id, cat, fn, abbr, color, ru, en, beh, traits, props, desc, sub=None, cap=None, roles=None, variants=None, versions=None, runbook=None):
    """kind() with the description given as one (ru, en) pair and the capacity as a plain rps number."""
    capacity = {"rpsPerReplica": cap} if cap else None
    subtitle = sub
    if isinstance(sub, tuple):
        subtitle = L(*sub)
    return kind(id, cat, fn, abbr, color, ru, en, beh, traits, props, desc=desc, capacity=capacity, roles=roles, subtitle=subtitle, variants=variants, versions=versions, runbook=runbook)
