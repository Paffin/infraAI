"""IaC templates for the new catalog kinds: an honest `target: none` with a reason that fits the function; traefik reuses the
verified ingress template of the generic L7 kind."""
import json
import os
import re

from arch import ENGINE as ENG

R = {
    "l7": ("Развёртывание {name} зависит от того, где он живёт: пара узлов с VRRP, ingress-контроллер в кластере или облачный сервис. Проверенного шаблона нет — добавьте свой.", "How {name} is deployed depends on where it lives: a VRRP pair, an ingress controller on the cluster or a cloud service. There is no verified template — add your own."),
    "l4": ("Развёртывание {name} зависит от сети площадки (VRRP, BGP, облачный балансировщик). Проверенного шаблона нет — добавьте свой.", "How {name} is deployed depends on the network of the site (VRRP, BGP, a cloud balancer). There is no verified template — add your own."),
    "proxy": ("Настройка {name} зависит от сети и политики доступа. Проверенного шаблона нет — добавьте свой.", "Setting up {name} depends on the network and the access policy. There is no verified template — add your own."),
    "http-cache": ("Правила кэширования {name} (VCL) пишутся под приложение: нейтрального шаблона нет. Добавьте свой.", "The caching rules of {name} (VCL) are written for the application: there is no neutral template. Add your own."),
    "firewall": ("Правила {name} зависят от вендора и сегментации сети: нейтрального шаблона нет. Добавьте шаблон под ваше оборудование.", "The rules of {name} depend on the vendor and the network segmentation: there is no neutral template. Add one for your equipment."),
    "network": ("Оборудование настраивается средствами вендора (Ansible, NAPALM, Terraform-провайдер производителя): нейтрального шаблона нет.", "Network devices are configured with the vendor’s tools (Ansible, NAPALM, a vendor Terraform provider): there is no neutral template."),
    "dns-server": ("Зоны и записи {name} зависят от вашей схемы имён: нейтрального шаблона нет. Добавьте свой.", "The zones and records of {name} depend on your naming scheme: there is no neutral template. Add your own."),
    "orchestrator": ("{name} ставится пакетами или своим сценарием; проверенного шаблона нет — добавьте свой.", "{name} is installed from packages or by your own playbook; there is no verified template — add your own."),
    "api-gateway": ("Маршруты и плагины {name} зависят от ваших API: нейтрального шаблона нет. Добавьте свой.", "The routes and plugins of {name} depend on your APIs: there is no neutral template. Add your own."),
    "data-processing": ("Задания и ресурсы {name} зависят от вашей обработки данных: нейтрального шаблона нет. Добавьте свой.", "The jobs and resources of {name} depend on your data processing: there is no neutral template. Add your own."),
    "database": ("Для {name} нужен оператор или сценарий развёртывания, зависящий от выбранного варианта; схемы ресурсов оператора не подтверждены, поэтому код не генерируется. Добавьте проверенный шаблон под ваш оператор.", "{name} needs an operator or a deployment playbook that depends on the chosen variant; the operator’s resource schemas have not been confirmed, so no code is generated. Add a verified template for your operator."),
    "mq": ("Для {name} нужен оператор или сценарий развёртывания, зависящий от выбранного варианта; схемы ресурсов оператора не подтверждены, поэтому код не генерируется. Добавьте проверенный шаблон.", "{name} needs an operator or a deployment playbook that depends on the chosen variant; the operator’s resource schemas have not been confirmed, so no code is generated. Add a verified template."),
    "storage": ("Хранилище {name} разворачивается оператором или средствами вендора, зависящими от оборудования: проверенного шаблона нет. Добавьте свой.", "The {name} storage is deployed by an operator or the vendor’s tools that depend on the hardware: there is no verified template. Add your own."),
    "platform": ("Значения установки {name} зависят от вашей среды (хранилище, домен, внешняя база): проверенного шаблона нет. Добавьте свой.", "The install values of {name} depend on your environment (storage, domain, an external database): there is no verified template. Add your own."),
}
BY_FN = {
    "l7": "l7", "l4": "l4", "proxy": "proxy", "http-cache": "http-cache", "firewall": "firewall", "network": "network", "dns-server": "dns-server",
    "orchestrator": "orchestrator", "api-gateway": "api-gateway", "data-processing": "data-processing",
    "rdbms": "database", "newsql": "database", "nosql-doc": "database", "graph": "database", "wide-column": "database", "kv": "database", "timeseries": "database", "cache": "database", "search": "database", "olap": "database",
    "mq": "mq", "sds": "storage",
}


def ensure(cat, write):
    """Every kind needs an IaC template (the engine's tests insist): a missing one becomes an honest `target: none` with a
    reason that fits the function – traefik reuses the verified ingress template. Existing files are never touched.
    Returns (ids without a template, whether the include list in defaults.rs is out of date); writes when `write`."""
    existing = {f[:-5] for f in os.listdir(f"{ENG}/defaults/iac")}
    missing = []
    for kd in cat["kinds"]:
        kid = kd["id"]
        if kid in existing:
            continue
        missing.append(kid)
        if not write:
            continue
        if kid == "traefik":
            t = json.load(open(f"{ENG}/defaults/iac/l7.json", encoding="utf-8"))
            t["id"] = "traefik"
            t["title"] = {"ru": "Traefik (Helm)", "en": "Traefik (Helm)"}
            for n in t.get("notes", []):
                n["text"] = {"ru": "Traefik установлен в кластер, хотя у узла указано размещение «vm».", "en": "Traefik is installed on the cluster although the node says hosting “vm”."}
        else:
            ru, en = R[BY_FN.get(kd.get("function"), "platform")]
            t = {"id": kid, "title": kd["name"], "enabled": True, "target": "none", "reason": {"ru": ru.format(name=kd["name"]["ru"]), "en": en.format(name=kd["name"]["en"])}}
        with open(f"{ENG}/defaults/iac/{kid}.json", "w", encoding="utf-8") as f:
            f.write(json.dumps(t, indent=1, ensure_ascii=False) + "\n")
    # the include list, in catalog order
    lines = "\n".join(f'    ("{kd["id"]}", include_str!("../defaults/iac/{kd["id"]}.json")),' for kd in cat["kinds"])
    p = f"{ENG}/src/defaults.rs"
    s = open(p, encoding="utf-8").read()
    marker = "pub const IAC_JSON: &[(&str, &str)] = &[\n"
    start = s.index(marker) + len(marker)
    end = s.index("];", start)
    stale = s[start:end] != lines + "\n"
    if stale and write:
        open(p, "w", encoding="utf-8").write(s[:start] + lines + "\n" + s[end:])
    return missing, stale
