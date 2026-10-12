#!/usr/bin/env python3
"""Regenerates the tables of docs/RULES.md and docs/CONFIGURATION.md from the JSON registries.

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
              "cost": "Стоимость", "gov": "Управление и согласованность", "obs": "Наблюдаемость"}
GROUPS = {"general": "Общие", "auth": "Аутентификация", "llm": "ИИ / OpenRouter", "jobs": "Задачи ИИ", "limits": "Лимиты схемы",
          "retention": "Хранение данных", "observability": "Наблюдаемость"}
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


BLOCKS = {"rules": ("docs/RULES.md", rules_block), "settings": ("docs/CONFIGURATION.md", settings_block)}


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
