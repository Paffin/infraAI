#!/usr/bin/env python3
"""Regenerates the tables of docs/RULES.md, docs/CONFIGURATION.md, docs/IAC.md, docs/CATALOG.md, docs/TEMPLATES.md and docs/FEATURES.md, and the numbers of README.md and README.en.md,
from the JSON registries (and the icon set of the UI, the tag names of the gallery, the tests in the repository).

    python3 deploy/gen-docs.py            # rewrite the generated blocks
    python3 deploy/gen-docs.py --check    # exit 1 if the docs are out of date (used in CI)

Only the text between `<!-- BEGIN GENERATED: name -->` and `<!-- END GENERATED: name -->` is replaced.
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ENGINE = ROOT / "backend/crates/engine/defaults"
SERVER = ROOT / "backend/crates/server/defaults"

CATEGORIES = {"ha": "Отказоустойчивость", "dr": "Катастрофоустойчивость", "net": "Сеть", "sec": "Безопасность", "cap": "Ёмкость",
              "cost": "Стоимость", "gov": "Управление и согласованность", "obs": "Наблюдаемость", "lifecycle": "Жизненный цикл продуктов"}
GROUPS = {"general": "Общие", "auth": "Аутентификация", "llm": "ИИ / OpenRouter", "jobs": "Задачи ИИ", "limits": "Лимиты схемы",
          "retention": "Хранение данных", "observability": "Наблюдаемость", "lifecycle": "Жизненный цикл"}
SEVERITY = {"critical": "критично", "risk": "риск"}


def ru(x):
    if x is None:
        return ""
    if isinstance(x, str):
        return x
    return x.get("ru") or x.get("en") or next(iter(x.values()), "")


def cell(s):
    return str(s).replace("|", "\\|").replace("\n", " ").strip()


def code(v):
    if isinstance(v, bool):
        return "`true`" if v else "`false`"
    if v is None or v == "" or v == [] or v == {}:
        return "—"
    if isinstance(v, (list, dict)):
        text = json.dumps(v, ensure_ascii=False)
        return "`" + (text if len(text) <= 60 else text[:57] + "…") + "`"
    return f"`{v}`"


def rules_block():
    rules = json.loads((ENGINE / "rules.json").read_text())
    out = [f"Всего правил: **{len(rules)}**. Серьёзность, вес, параметры и тексты каждого правила редактируются в админ-панели; правило можно отключить.", ""]
    by_cat = {}
    for r in rules:
        by_cat.setdefault(r["category"], []).append(r)
    for cat in [c for c in CATEGORIES if c in by_cat] + [c for c in by_cat if c not in CATEGORIES]:
        out += [f"### {CATEGORIES.get(cat, cat)}", "", "| Правило | Проверка | При нарушении | Название |", "|---|---|---|---|"]
        for r in by_cat[cat]:
            sev = SEVERITY.get(r.get("severity", "risk"), r.get("severity"))
            out.append(f"| `{r['id']}` | `{r['check']}` | {sev} | {cell(ru(r['title']))} |")
        out.append("")
    return "\n".join(out).rstrip()


def settings_block():
    settings = json.loads((SERVER / "settings.json").read_text())
    out = [f"Всего настроек: **{len(settings)}**.", ""]
    by_group = {}
    for s in settings:
        by_group.setdefault(s["group"], []).append(s)
    for g in [x for x in GROUPS if x in by_group] + [x for x in by_group if x not in GROUPS]:
        out += [f"### {GROUPS.get(g, g)}", "", "| Ключ | Тип | По умолчанию | Описание |", "|---|---|---|---|"]
        for s in by_group[g]:
            ty = s["type"] + (f" ({', '.join(s['options'])})" if s.get("options") else "")
            if s.get("format"):
                ty += f", формат `{s['format']}`"
            if s.get("min") is not None or s.get("max") is not None:
                ty += f" [{s.get('min', '')}…{s.get('max', '')}]"
            default = "—" if s["type"] == "secret" else code(s.get("default"))
            desc = ru(s.get("label"))
            if s.get("help"):
                desc += ". " + ru(s["help"])
            out.append(f"| `{s['key']}` | {cell(ty)} | {default} | {cell(desc)} |")
        out.append("")
    return "\n".join(out).rstrip()


def catalog_block():
    cat = json.loads((ENGINE / "catalog.json").read_text())
    tables = {t["id"]: t for t in json.loads((ENGINE / "versions.json").read_text())}
    kinds = cat["kinds"]
    out = [f"Видов: **{len(kinds)}**, вариантов: **{sum(len(k.get('variants', [])) for k in kinds)}**, функций: **{sum(len(c.get('functions', [])) for c in cat['categories'])}**. "
           "«Версии» — таблица выпусков, по которой проверяется срок поддержки (у варианта она может быть своей; прочерк — таблицы нет).", ""]
    for c in sorted(cat["categories"], key=lambda c: c.get("order", 0)):
        out += [f"### {ru(c['name'])}", ""]
        for fn in c.get("functions", []):
            ks = [k for k in kinds if k["category"] == c["id"] and k.get("function") == fn["id"]]
            if not ks:
                continue
            out += [f"**{ru(fn['name'])}**", "", "| Вид | Поведение | Варианты | Версии |", "|---|---|---|---|"]
            for k in ks:
                vs = ", ".join(ru(v["name"]) + (" ★" if v.get("default") else "") for v in k.get("variants", [])) or "—"
                default = next((v for v in k.get("variants", []) if v.get("default")), None)
                table = (default or {}).get("versions", k.get("versions", k["id"]))
                shown = ru(tables[table].get("name") or table) if table in tables else "—"
                out.append(f"| `{k['id']}` — {cell(ru(k['name']))} | `{k['behavior']}` | {cell(vs)} | {cell(shown)} |")
            out.append("")
    return "\n".join(out).rstrip()


def icons_block():
    """The icon names the UI knows (frontend/src/lib/icons.ts) and which of them the shipped catalog uses."""
    src = (ROOT / "frontend/src/lib/icons.ts").read_text()
    body = src.split("export const ICONS", 1)[1].split("= {", 1)[1].split("\n}\n", 1)[0]
    known = [a or b for a, b in re.findall(r"'([a-z0-9-]+)'\s*:\s*[A-Z]|\b([a-z][a-z0-9]*)\s*:\s*[A-Z]", body)]
    cat = json.loads((ENGINE / "catalog.json").read_text())
    used = {x["icon"] for group in (cat["kinds"], cat["categories"], cat["zoneTypes"]) for x in group if x.get("icon")}
    unknown = sorted(used - set(known))
    if unknown:
        sys.exit("catalog.json names icons the UI does not know (frontend/src/lib/icons.ts): " + ", ".join(unknown))
    by_use = [n for n in known if n in used]
    spare = [n for n in known if n not in used]
    return "\n".join([f"Интерфейс знает **{len(known)}** имён, каталог по умолчанию использует **{len(by_use)}**.", "",
                      "| Значки | Имена |", "|---|---|",
                      f"| Используются в каталоге по умолчанию | {', '.join(f'`{n}`' for n in by_use)} |",
                      f"| Про запас, для ваших видов | {', '.join(f'`{n}`' for n in spare)} |"])


def versions_block():
    tables = json.loads((ENGINE / "versions.json").read_text())
    as_of = sorted({t["asOf"] for t in tables if t.get("asOf")})
    out = [f"Таблиц: **{len(tables)}**, данные на **{as_of[-1]}** (самая старая — {as_of[0]}). «LTS» — есть линия с долгой поддержкой.", "",
           "| Таблица | Продукт | Линий | Новейшая линия | LTS |", "|---|---|---|---|---|"]
    for t in tables:
        newest = t["cycles"][0]["cycle"] if t["cycles"] else "—"
        lts = "да" if any(c.get("lts") for c in t["cycles"]) else "—"
        out.append(f"| `{t['id']}` | {cell(t.get('name') or t['id'])} | {len(t['cycles'])} | `{newest}` | {lts} |")
    return "\n".join(out)


def iac_kinds_block():
    d = ENGINE / "iac"
    catalog = {k["id"]: k for k in json.loads((ENGINE / "catalog.json").read_text())["kinds"]}
    items = [json.loads(p.read_text()) for p in sorted(d.glob("*.json"))]
    order = {k: i for i, k in enumerate(catalog)}
    items.sort(key=lambda t: order.get(t["id"], 999))
    done = [t for t in items if t.get("target") == "kubernetes"]
    out = [f"Генерируются **{len(done)}** из {len(items)} видов компонентов; для остальных в отчёте экспорта указана причина.", "",
           "### Что генерируется", "", "| Вид | Результат | Требует | Нужны данные |", "|---|---|---|---|"]
    for t in done:
        objs = [f"`{o['body']['kind']}`" for o in t.get("objects", [])]
        rels = [f"Helm `{r['chart']}` {r.get('version', '')}".strip() for r in t.get("releases", [])]
        req = [r if isinstance(r, str) else r["id"] + " (по условию)" for r in t.get("requires", [])]
        needs = [f"`{n['prop']}`" for n in t.get("needs", [])]
        out.append(f"| `{t['id']}` — {cell(ru(t.get('title')))} | {cell(', '.join(objs + rels))} | {cell(', '.join(req)) or '—'} | {', '.join(needs) or '—'} |")
    out += ["", "### Что не генерируется — и почему", "", "| Вид | Причина |", "|---|---|"]
    for t in items:
        if t.get("target") != "kubernetes":
            out.append(f"| `{t['id']}` | {cell(ru(t.get('reason')))} |")
    return "\n".join(out)


def iac_operators_block():
    ops = json.loads((ENGINE / "engine.json").read_text())["export"]["k8s"]["operators"]
    out = ["| Идентификатор | Что | Чарт | Версия | Сначала |", "|---|---|---|---|---|"]
    for oid, o in ops.items():
        out.append(f"| `{oid}` | {cell(ru(o.get('title')))} | `{o['repo']}/{o['chart']}` | `{o['version']}` | {', '.join(f'`{a}`' for a in o.get('after', [])) or '—'} |")
    return "\n".join(out)


UNITS = {"ru": (("дн", 86400), ("ч", 3600), ("мин", 60), "с"), "en": (("d", 86400), ("h", 3600), ("min", 60), "s")}


def seconds_text(v, lang="ru"):
    """0 → `0`; 35 → `35 с`; 300 → `5 мин`; 14400 → `4 ч` (in English: `35 s`, `5 min`, `4 h`)."""
    if v is None:
        return "—"
    if v == 0:
        return "0"
    v = int(v) if float(v).is_integer() else v   # a template may carry 300.0 for 300
    *units, second = UNITS[lang]
    for unit, size in units:
        if v >= size and v % size == 0:
            return f"{v // size} {unit}"
    return f"{v:g} {second}"


def templates_block():
    tolerance = {l["id"]: ru(l["name"]).split(" (")[0].lower() for l in json.loads((ENGINE / "engine.json").read_text())["tolerance"]["levels"]}
    ts = sorted((json.loads(p.read_text()) for p in (ENGINE / "templates").glob("*.json")), key=lambda t: t.get("order", 0))
    out = [f"Эталонных архитектур: **{len(ts)}**. Каждая проходит правила без критичных находок, держит заявленную доступность и собственный шлюз `infra-architect check` (проверяют тесты).", "",
           "| Архитектура | Теги | Узлов | SLO | RPO | RTO | Переживает |", "|---|---|---|---|---|---|---|"]
    for t in ts:
        r = t["architecture"]["requirements"]
        name = f"**{cell(ru(t['name']))}** `{t['badge']}`<br>{cell(ru(t['description']))}"
        out.append(f"| {name} | {', '.join(f'`{x}`' for x in t.get('tags', []))} | {len(t['architecture']['nodes'])} | {r.get('availabilitySlo', '—')} % | {seconds_text(r.get('rpoSeconds'))} | {seconds_text(r.get('rtoSeconds'))} | {tolerance.get(r.get('failureTolerance'), r.get('failureTolerance', '—'))} |")
    return "\n".join(out)


def stats():
    """The numbers the README is proud of, counted from what ships: nothing here is typed in."""
    catalog = json.loads((ENGINE / "catalog.json").read_text())
    rules = json.loads((ENGINE / "rules.json").read_text())
    kinds = catalog["kinds"]
    tests = (sum(len(re.findall(r"#\[(?:tokio::)?test", p.read_text())) for p in (ROOT / "backend/crates").rglob("*.rs"))
             + sum(len(re.findall(r"^\s*(?:it|test)(?:\.each\([^)]*\))?\(", p.read_text(), re.M)) for p in (ROOT / "frontend/tests").glob("*.test.ts"))
             + sum(len(re.findall(r"^\s*test\(", p.read_text(), re.M)) for p in (ROOT / "frontend/e2e").glob("*.spec.ts")))
    return {"kinds": len(kinds), "variants": sum(len(k.get("variants", [])) for k in kinds), "functions": sum(len(c.get("functions", [])) for c in catalog["categories"]),
            "rules": len(rules), "checks": len({r["check"] for r in rules}), "templates": len(list((ENGINE / "templates").glob("*.json"))),
            "settings": len(json.loads((SERVER / "settings.json").read_text())), "versions": len(json.loads((ENGINE / "versions.json").read_text())),
            "tests": tests // 50 * 50}  # rounded down: a count that changes with every test would make the README stale with every commit


def plural(n, forms, lang):
    """`forms` is (one, few, many) in Russian and (one, many) in English."""
    if lang == "en":
        return forms[0] if n == 1 else forms[-1]
    n10, n100 = n % 10, n % 100
    return forms[0] if n10 == 1 and n100 != 11 else forms[1] if 2 <= n10 <= 4 and not 12 <= n100 <= 14 else forms[2]


def readme_numbers(lang):
    s = stats()
    cells = [
        (str(s["kinds"]), {"ru": ("продукт в каталоге", "продукта в каталоге", "продуктов в каталоге"), "en": ("product in the catalog", "products in the catalog")}),
        (str(s["variants"]), {"ru": ("вариант развёртывания", "варианта развёртывания", "вариантов развёртывания"), "en": ("deployment variant", "deployment variants")}),
        (str(s["rules"]), {"ru": ("правило проверки", "правила проверки", "правил проверки"), "en": ("verification rule", "verification rules")}),
        (str(s["templates"]), {"ru": ("эталонная архитектура", "эталонные архитектуры", "эталонных архитектур"), "en": ("reference architecture", "reference architectures")}),
        (str(s["settings"]), {"ru": ("настройка без пересборки", "настройки без пересборки", "настроек без пересборки"), "en": ("setting, no rebuild", "settings, no rebuild")}),
        (f"{s['tests']}+", {"ru": ("автотест без моков", "автотеста без моков", "автотестов без моков"), "en": ("test, no mocks", "tests, no mocks")}),
    ]
    out = ['<table align="center">', "<tr>"]
    for number, forms in cells:
        n = int(number.rstrip("+"))
        out.append(f'<td align="center"><b>{number}</b><br><sub>{plural(n, forms[lang], lang)}</sub></td>')
    out += ["</tr>", "</table>"]
    return "\n".join(out)


def tag_names(lang):
    """How the gallery of the interface calls a tag of a reference architecture (frontend/src/i18n)."""
    src = (ROOT / f"frontend/src/i18n/{lang}.ts").read_text()
    return dict(re.findall(r"'tpl\.tag\.([a-z0-9-]+)':\s*'([^']*)'", src))


def readme_templates(lang):
    tolerance = {l["id"]: l["name"][lang].split(" (")[0].lower() for l in json.loads((ENGINE / "engine.json").read_text())["tolerance"]["levels"]}
    names = tag_names(lang)
    ts = sorted((json.loads(p.read_text()) for p in (ENGINE / "templates").glob("*.json")), key=lambda t: t.get("order", 0))
    head = {"ru": ("Архитектура", "Теги", "SLO", "RPO", "RTO", "Переживает"), "en": ("Architecture", "Tags", "SLO", "RPO", "RTO", "Survives")}[lang]
    title = {"ru": "Все эталонные архитектуры", "en": "All reference architectures"}[lang]
    out = [f"<details>", f"<summary><b>{title}</b> · {len(ts)}</summary>", "", "| " + " | ".join(head) + " |", "|---|---|---|---|---|---|"]
    for t in ts:
        r = t["architecture"]["requirements"]
        tags = " · ".join(names.get(x, x) for x in t.get("tags", []))
        out.append(f"| **{cell(t['name'][lang])}** | {cell(tags)} | {r.get('availabilitySlo', '—')} % | {seconds_text(r.get('rpoSeconds'), lang)} | {seconds_text(r.get('rtoSeconds'), lang)} | "
                   f"{tolerance.get(r.get('failureTolerance'), r.get('failureTolerance', '—'))} |")
    out += ["", "</details>"]
    return "\n".join(out)


def features_facts():
    s = stats()
    return (f"Сейчас в поставке: **{s['kinds']}** продуктов в **{s['functions']}** функциях ({s['variants']} вариантов развёртывания, {s['versions']} таблиц версий), **{s['rules']}** правил "
            f"({s['checks']} типов проверок), **{s['templates']}** эталонных архитектур, **{s['settings']}** настроек.")


BLOCKS = [  # (marker name, file, generator)
    ("templates", "docs/TEMPLATES.md", templates_block),
    ("rules", "docs/RULES.md", rules_block),
    ("settings", "docs/CONFIGURATION.md", settings_block),
    ("iac-kinds", "docs/IAC.md", iac_kinds_block),
    ("iac-operators", "docs/IAC.md", iac_operators_block),
    ("catalog", "docs/CATALOG.md", catalog_block),
    ("versions", "docs/CATALOG.md", versions_block),
    ("icons", "docs/CATALOG.md", icons_block),
    ("features-facts", "docs/FEATURES.md", features_facts),
    ("readme-numbers", "README.md", lambda: readme_numbers("ru")),
    ("readme-templates", "README.md", lambda: readme_templates("ru")),
    ("readme-numbers", "README.en.md", lambda: readme_numbers("en")),
    ("readme-templates", "README.en.md", lambda: readme_templates("en")),
]


def render_text(text, rel, name, body):
    pattern = re.compile(rf"(<!-- BEGIN GENERATED: {name} -->)\n.*?(<!-- END GENERATED: {name} -->)", re.S)
    if not pattern.search(text):
        sys.exit(f"{rel}: markers for '{name}' not found")
    return pattern.sub(lambda m: f"{m.group(1)}\n{body}\n{m.group(2)}", text)


def render(path, name, body):
    return render_text(path.read_text(), path.relative_to(ROOT), name, body)


def main():
    check = "--check" in sys.argv
    texts = {}
    for name, rel, fn in BLOCKS:
        path = ROOT / rel
        texts[rel] = render(path, name, fn()) if rel not in texts else render_text(texts[rel], rel, name, fn())
    stale = []
    for rel, new in texts.items():
        path = ROOT / rel
        if new != path.read_text():
            if check:
                stale.append(rel)
            else:
                path.write_text(new)
                print(f"updated {rel}")
    if stale:
        sys.exit("out of date: " + ", ".join(stale) + " – run python3 deploy/gen-docs.py")


if __name__ == "__main__":
    main()
