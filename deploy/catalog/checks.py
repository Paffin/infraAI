"""The checks on the sources of the catalog: build.py runs them over everything, lint.py over a module while it is being written."""
import glob
import importlib
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
CATALOG_JSON = f"{REPO}/backend/crates/engine/defaults/catalog.json"
ICONS_TS = f"{REPO}/frontend/src/lib/icons.ts"

BEHAVIORS = {"stateless", "primary_replica", "consensus", "quorum_log"}
CATEGORIES = {"edge", "compute", "data", "platform", "security", "apps", "ai"}
PROP_TYPES = {"number", "string", "boolean", "enum", "unit_ref"}
# traits the rules, the simulator and the cost model read (README.md): a kind that says one of these must also carry what the rule needs
MODEL_TRAITS = {"workload", "datastore", "balancer"}
BUILTIN_SUBTITLE_KEYS = {"variantName"}
CYR = re.compile("[А-Яа-яЁё]")


RULES_JSON = f"{REPO}/backend/crates/engine/defaults/rules.json"


def rule_traits():
    """Every trait the rules of the engine name (`trait`, `anyTrait`, `excludeTraits`, `ignoreTraits`, in any nesting): a kind that says one is read."""
    found = set()

    def walk(v, key=""):
        if isinstance(v, dict):
            for k_, x in v.items():
                walk(x, k_)
        elif isinstance(v, list):
            for x in v:
                walk(x, key)
        elif isinstance(v, str) and "trait" in key.lower():
            found.add(v)

    walk(json.load(open(RULES_JSON, encoding="utf-8")))
    return found


def icon_names():
    """The names of the icon registry of the UI (frontend/src/lib/icons.ts)."""
    src = open(ICONS_TS, encoding="utf-8").read()
    body = src.split("export const ICONS", 1)[1].split("= {", 1)[1].split("\n}\n", 1)[0]
    return {a or b for a, b in re.findall(r"'([a-z0-9-]+)'\s*:\s*[A-Z]|\b([a-z][a-z0-9]*)\s*:\s*[A-Z]", body)}


def modules():
    """The kinds_*.py modules in the order they are built, as [(name, module)]; one that cannot be imported is reported by the caller."""
    out = []
    sys.path.insert(0, HERE)
    for path in sorted(glob.glob(f"{HERE}/kinds_*.py")):
        name = os.path.basename(path)[:-3]
        try:
            out.append((name, importlib.import_module(name), None))
        except BaseException as e:  # noqa: BLE001 - a half-written module must not stop the others from being checked
            out.append((name, None, f"{type(e).__name__}: {e}"))
    return out


def text_ok(v, lang):
    return isinstance(v, dict) and isinstance(v.get(lang), str) and v[lang].strip() != ""


def check_kind(kd, fns_of, errs, warns, tables, known_traits, icons):
    kid = kd.get("id", "?")
    e = lambda msg: errs.append(f"{kid}: {msg}")  # noqa: E731
    w = lambda msg: warns.append(f"{kid}: {msg}")  # noqa: E731
    if not re.fullmatch(r"[a-z][a-z0-9_]*", str(kid)):
        e("the id is lower snake_case: letters, digits and underscores, starting with a letter")
    if kd.get("category") not in CATEGORIES:
        e(f"category {kd.get('category')!r} is not one of {sorted(CATEGORIES)}")
    fn = kd.get("function")
    if kd.get("category") in fns_of and fn not in fns_of[kd["category"]]:
        e(f"function {fn!r} is not declared in category {kd.get('category')!r} (declare it in FUNCTIONS of the module)")
    if not 1 <= len(str(kd.get("abbr", ""))) <= 4:
        e(f"abbr {kd.get('abbr')!r} must be 1..4 characters")
    if not re.fullmatch(r"#[0-9a-fA-F]{6}", str(kd.get("color", ""))):
        e(f"color {kd.get('color')!r} is #rrggbb")
    if kd.get("behavior") not in BEHAVIORS:
        e(f"behavior {kd.get('behavior')!r} is one of {sorted(BEHAVIORS)}")
    for lang in ("ru", "en"):
        if not text_ok(kd.get("name"), lang):
            e(f"name.{lang} is missing")
        d = kd.get("description")
        if not text_ok(d, lang):
            e(f"description.{lang} is missing")
        elif len(d[lang].strip()) < 40:
            e(f"description.{lang} is too short to say what the product is and how it fails ({len(d[lang].strip())} characters)")
    d = kd.get("description") or {}
    if text_ok(d, "ru") and not CYR.search(d["ru"]):
        e("description.ru has no Russian in it")
    if text_ok(d, "en") and CYR.search(d["en"]):
        e("description.en has Russian in it")
    if d.get("ru") and d.get("ru") == d.get("en"):
        e("description.ru and description.en are the same text")
    traits = kd.get("traits")
    if not isinstance(traits, list) or not traits or not all(isinstance(t, str) for t in traits):
        e("traits is a non-empty list of strings")
        traits = []
    for t in traits:
        if t not in known_traits:
            w(f"trait {t!r} is new: nothing in the engine reads it (fine for a description, a typo otherwise)")
    props = kd.get("props") or []
    keys = [p.get("key") for p in props]
    if len(keys) != len(set(keys)):
        e(f"duplicate props {sorted({x for x in keys if keys.count(x) > 1})}")
    for p in props:
        if p.get("type") not in PROP_TYPES:
            e(f"prop {p.get('key')}: type {p.get('type')!r}")
        if p.get("type") == "enum" and not p.get("options"):
            e(f"prop {p.get('key')}: an enum without options")
        if not text_ok(p.get("label"), "ru") or not text_ok(p.get("label"), "en"):
            e(f"prop {p.get('key')}: label in ru and en")
    sub = kd.get("subtitle")
    if sub:
        for text in ([sub] if isinstance(sub, str) else [sub.get("ru", ""), sub.get("en", "")]):
            for ph in re.findall(r"\{(\w+)\}", text):
                if ph not in keys and ph not in BUILTIN_SUBTITLE_KEYS:
                    e(f"subtitle says {{{ph}}}, which is not a property of the kind")
    cap = (kd.get("capacity") or {}).get("rpsPerReplica")
    # a service the provider sizes (`provider-ha`) has no capacity of its own to declare: the engine asks the node for one only when it matters
    is_model = bool(MODEL_TRAITS & set(traits)) and "provider-ha" not in traits
    if is_model and cap is None and "storage" not in traits:
        e(f"traits {sorted(MODEL_TRAITS & set(traits))} take part in the capacity model: give `cap` (requests per second one instance sustains)")
    if ("workload" in traits or "balancer" in traits) and "managed" not in traits and "replicas" not in keys:
        e("an application tier or a balancer has instances: a `replicas` property")
    if "datastore" in traits and "cache" not in traits and "encryptionAtRest" not in keys:
        e("a datastore says whether it encrypts at rest (SEC-ENC-01 asks): an `encryptionAtRest` property (db_props / cluster_props have it)")
    if "backup-target" in traits and not {"immutable", "retentionDays", "restoreTestedDays"} <= set(keys):
        e("a backup target carries immutable / retentionDays / restoreTestedDays (DR-BKP-03..05): use backup_props()")
    if kd.get("behavior") == "primary_replica" and not kd.get("roles"):
        w("a primary/replica kind names its roles (roles=R_PR())")
    vt = kd.get("versions")
    if vt not in (None, "") and vt not in tables:
        e(f"release table {vt!r} does not exist (a table of versions.json, or versions=\"\")")
    seen, defaults = set(), 0
    for v in kd.get("variants") or []:
        vid = v.get("id")
        if vid in seen:
            e(f"variant {vid!r} twice")
        seen.add(vid)
        defaults += 1 if v.get("default") else 0
        if not text_ok(v.get("name"), "ru") or not text_ok(v.get("name"), "en"):
            e(f"variant {vid}: name in ru and en")
        if not text_ok(v.get("description"), "ru") or not text_ok(v.get("description"), "en"):
            e(f"variant {vid}: description in ru and en")
        if v.get("behavior") is not None and v["behavior"] not in BEHAVIORS:
            e(f"variant {vid}: behavior {v['behavior']!r}")
        if v.get("versions") not in (None, "") and v["versions"] not in tables:
            e(f"variant {vid}: release table {v['versions']!r} does not exist")
    if defaults > 1:
        e("more than one default variant")
    ic = kd.get("icon")
    if ic and ic not in icons:
        e(f"icon {ic!r} is not in the registry of the UI (frontend/src/lib/icons.ts)")


def check_all(kinds, fns_of, existing_names=None, tables=None, known_traits=None, icons=None, only=None):
    """Errors and warnings of the kinds `kinds` (a list of kind dicts; `only` limits the report to some ids); uniqueness is checked over all of them."""
    errs, warns = [], []
    ids = [k_["id"] for k_ in kinds]
    for i in sorted({x for x in ids if ids.count(x) > 1}):
        if only is None or i in only:
            errs.append(f"{i}: the id is used twice")
    names = {}
    for k_ in kinds:
        for lang in ("ru", "en"):
            n = ((k_.get("name") or {}).get(lang) or "").strip().lower()
            if n:
                names.setdefault((lang, n), []).append(k_["id"])
    for (lang, n), who in names.items():
        if len(who) > 1 and (only is None or set(who) & only):
            errs.append(f"{', '.join(who)}: the same name ({lang}: {n!r}) – one product listed twice?")
    for kd in kinds:
        if only is not None and kd["id"] not in only:
            continue
        check_kind(kd, fns_of, errs, warns, tables, known_traits, icons)
    return errs, warns
