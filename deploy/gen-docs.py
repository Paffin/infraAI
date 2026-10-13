#!/usr/bin/env python3
"""Regenerates the tables of docs/RULES.md, docs/CONFIGURATION.md, docs/IAC.md and docs/CATALOG.md from the JSON registries.

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


BLOCKS = {
    "rules": ("docs/RULES.md", rules_block),
    "settings": ("docs/CONFIGURATION.md", settings_block),
    "iac-kinds": ("docs/IAC.md", iac_kinds_block),
    "iac-operators": ("docs/IAC.md", iac_operators_block),
    "catalog": ("docs/CATALOG.md", catalog_block),
    "versions": ("docs/CATALOG.md", versions_block),
}


def render(path, name, body):
    text = path.read_text()
    pattern = re.compile(rf"(<!-- BEGIN GENERATED: {name} -->)\n.*?(<!-- END GENERATED: {name} -->)", re.S)
    if not pattern.search(text):
        sys.exit(f"{path}: markers for '{name}' not found")
    return pattern.sub(lambda m: f"{m.group(1)}\n{body}\n{m.group(2)}", text)


def main():
    check = "--check" in sys.argv
    stale = []
    for name, (rel, fn) in BLOCKS.items():
        path = ROOT / rel
        new = render(path, name, fn())
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
