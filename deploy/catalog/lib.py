"""Authoring helpers for catalog.json: prop builders, archetypes, variants. Output matches the hand-written kinds."""
import json

L = lambda ru, en: {"ru": ru, "en": en}


def prop(key, typ, ru, en, **kw):
    d = {"key": key, "type": typ, "label": L(ru, en)}
    for k, v in kw.items():
        if v is None:
            continue
        if k == "help":
            v = L(*v)
        if k == "unit_":
            k = "unit"
        d[k] = v
    return d


def num(key, ru, en, **kw):
    return prop(key, "number", ru, en, **kw)


def enum(key, ru, en, options, **kw):
    opts = [{"value": v, "label": L(r, e)} for v, r, e in options]
    return prop(key, "enum", ru, en, options=opts, **kw)


def boolean(key, ru, en, **kw):
    return prop(key, "boolean", ru, en, **kw)


def string(key, ru, en, **kw):
    return prop(key, "string", ru, en, **kw)


# ---- shared props -------------------------------------------------------------------------------------------
def p_version(default):
    return string("version", "Версия", "Version", default=default, group="platform")


def p_resources(vcpu, ram, disk=None, replicas=None):
    out = []
    if replicas is not None:
        out.append(num("replicas", "Экземпляры", "Instances", min=1, default=replicas, group="capacity"))
    out.append(num("vcpu", "vCPU на экземпляр", "vCPU per instance", unit_="vCPU", min=0, group="capacity", default=vcpu))
    out.append(num("ramGb", "RAM на экземпляр", "RAM per instance", unit_="GB", min=0, group="capacity", default=ram))
    if disk is not None:
        out.append(num("diskGb", "Диск на экземпляр", "Disk per instance", unit_="GB", min=0, group="capacity", default=disk))
    return out


def p_failover(default="auto", secs=30, dcs=True, votes=True, dcs_help=None):
    out = [
        enum("failover", "Переключение (failover)", "Failover", [("auto", "Автоматическое", "Automatic"), ("manual", "Ручное", "Manual"), ("none", "Нет", "None")], default=default, group="failover"),
        num("failoverSeconds", "Время автопереключения", "Automatic failover time", unit_="s", min=0, default=secs, group="failover"),
    ]
    if dcs:
        out.append(prop("dcs", "unit_ref", "Хранилище кворума (DCS)", "Quorum store (DCS)", refBehavior="consensus", group="failover",
                        help=dcs_help or ("Кластер консенсуса (etcd/Consul/ZooKeeper), по которому выбирается лидер. Без DCS или witness автопереключение небезопасно (split-brain).",
                                          "Consensus cluster (etcd/Consul/ZooKeeper) used for leader election. Without a DCS or witness automatic failover is unsafe (split-brain).")))
    if votes:
        out.append(num("votes", "Голоса", "Votes", min=0, default=1, group="failover", advanced=True))
    return out


def p_enc():
    return boolean("encryptionAtRest", "Шифрование на диске", "Encryption at rest", group="security")


def p_hosting(default="auto"):
    return enum("hosting", "Размещение", "Hosting", [
        ("auto", "Авто — в кластере Kubernetes зоны, если он есть", "Auto – on the zone’s Kubernetes cluster if there is one"),
        ("kubernetes", "Kubernetes", "Kubernetes"), ("vm", "Виртуальные машины", "Virtual machines"), ("managed", "Управляемый сервис облака", "Managed cloud service")],
        default=default, group="platform", advanced=True,
        help=("Определяет, создаются ли для узла манифесты Kubernetes при экспорте IaC.", "Decides whether Kubernetes manifests are generated for the node when exporting IaC."))


def p_lb():
    return [num("healthCheckSeconds", "Интервал health-check", "Health-check interval", unit_="s", min=1, default=3, group="failover"),
            num("failThreshold", "Порог отказов", "Failure threshold", min=1, default=2, group="failover")]


ROLES_PR = [{"id": "primary", "name": L("Primary", "Primary")}, {"id": "replica", "name": L("Реплика", "Replica")}, {"id": "witness", "name": L("Witness (арбитр)", "Witness (arbiter)")}]
ROLES_CONSENSUS = [{"id": "voter", "name": L("Голосующий", "Voter")}, {"id": "witness", "name": L("Witness (арбитр)", "Witness (arbiter)")}]


def roles_pr(primary="Primary", replica=("Реплика", "Replica"), witness=("Witness (арбитр)", "Witness (arbiter)")):
    return [{"id": "primary", "name": L(primary, primary)}, {"id": "replica", "name": L(*replica)}, {"id": "witness", "name": L(*witness)}]


def runbook(failover, restore, verify):
    return {"failover": [L(*x) for x in failover], "restore": [L(*x) for x in restore], "verify": [L(*x) for x in verify]}


def variant(id, ru, en, desc_ru="", desc_en="", default=False, behavior=None, add_traits=None, remove_traits=None, defaults=None, capacity=None, mult=None, fixed=None, versions=None, roles=None):
    v = {"id": id, "name": L(ru, en)}
    if desc_ru or desc_en:
        v["description"] = L(desc_ru, desc_en)
    if default:
        v["default"] = True
    if behavior:
        v["behavior"] = behavior
    if roles:
        v["roles"] = roles
    if add_traits:
        v["addTraits"] = add_traits
    if remove_traits:
        v["removeTraits"] = remove_traits
    if defaults:
        v["defaults"] = defaults
    if capacity:
        v["capacity"] = capacity
    if mult is not None or fixed is not None:
        v["cost"] = {"multiplier": mult if mult is not None else 1.0, "monthlyFixed": fixed or 0}
    if versions is not None:
        v["versions"] = versions
    return v


def managed(id="managed", ru="Управляемый сервис облака", en="Managed cloud service", versions=None, secs=60, mult=1.6, extra=None, desc_ru=None, desc_en=None):
    d = {"hosting": "managed"}
    if secs is not None:
        d.update({"failover": "auto", "failoverSeconds": secs})
    if extra:
        d.update(extra)
    # a managed offering lives by the provider's dates, not by the community's: without a table of its own it has no lifecycle
    return variant(id, ru, en,
                   desc_ru or "Отказоустойчивость, обновления и резервные копии обеспечивает провайдер: выше цена, меньше контроля над топологией.",
                   desc_en or "The provider runs failover, upgrades and backups: higher price, less control over the topology.",
                   add_traits=["managed"], defaults=d, mult=mult, versions=versions if versions is not None else "")


def kind(id, category, function, abbr, color, ru, en, behavior, traits, props, desc=None, capacity=None, roles=None, subtitle=None, variants=None, versions=None, runbook=None):
    k = {"id": id, "category": category, "abbr": abbr, "color": color, "behavior": behavior, "traits": traits, "name": L(ru, en)}
    if desc:
        k["description"] = L(*desc)
    if subtitle:
        k["subtitle"] = subtitle if isinstance(subtitle, str) else L(*subtitle)
    if capacity is not None:
        k["capacity"] = capacity
    if roles:
        k["roles"] = roles
    k["props"] = props
    if runbook:
        k["runbook"] = runbook
    k["function"] = function
    if variants:
        k["variants"] = variants
    if versions is not None:
        k["versions"] = versions
    return k
