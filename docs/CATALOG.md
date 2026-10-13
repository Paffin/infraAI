# Каталог продуктов

Каталог — это **данные** (`catalog.json`, элементы `catalog.kind` в таблице `config_items`), а не код. Всё, что редактор показывает в библиотеке и
инспекторе, что движок знает о поведении продукта при отказе и что ИИ видит в промпте, берётся отсюда. Новый продукт, новый вариант развёртывания
или новая версия — это правка в админ-панели (**Администрирование → Каталог**), перезапуск не нужен.

## Как устроен каталог

```
категория (data, platform, network…)
 └─ функция (rdbms, cache, mq, l7, secrets…)         — «какую работу выполняет»; продукты внутри взаимозаменяемы
     └─ вид = продукт (postgresql, mysql, cockroachdb…)
         ├─ вариант (patroni, repmgr, rds, aurora…)  — «как развёрнут»: топология HA или управляемый сервис
         └─ версия (16, 17, 8.0…)                    — линия выпуска из таблицы жизненного цикла
```

| Понятие | Что это | Где задаётся |
|---|---|---|
| **Категория** | раздел библиотеки (`data`, `network`, `platform`…) | `catalog.category`; **функции** объявлены в категории (`categories[].functions`) |
| **Функция** | работа, которую делает слой схемы: «реляционная БД», «кэш», «очередь», «L7-балансировщик». Продукты одной функции можно подменять друг другом | `KindDef.function` |
| **Вид (продукт)** | PostgreSQL, Redis, NGINX, Kafka… Поведение при отказе (`stateless` / `primary_replica` / `consensus` / `quorum_log`), признаки (`traits`), роли, свойства, ёмкость, цена | `catalog.kind` |
| **Вариант** | способ развёртывания продукта: Patroni, repmgr, Amazon RDS, Aurora… Меняет то, как движок видит узел | `KindDef.variants[]` |
| **Версия** | линия выпуска (`16`, `1.31`). Срок поддержки проверяется по таблице выпусков | `catalog.versions` |

Узел на схеме хранит выбор в своих свойствах: `props.variant` (пусто — вариант по умолчанию, помеченный `default`) и `props.version`.
Неизвестный вариант движок отклоняет при валидации (`unknown_variant`) — ни ручная правка, ни предложение ИИ не создадут узел с вариантом, которого нет в каталоге.

## Варианты

Вариант — это не подпись, а **изменение модели**. Всё, что он задаёт, переопределяет вид:

| Поле варианта | Что меняет |
|---|---|
| `behavior` | поведение при отказе: у Vault со встроенным Raft вместо `primary_replica` — `consensus` (голосующие, а не primary) |
| `addTraits` / `removeTraits` | признаки узла: `managed` (провайдер отвечает за переключение), `self-quorum` (кворум внутри продукта), `platform-quorum` (кворум платформы), … |
| `defaults` | значения свойств по умолчанию: автопереключение за 30 с, `hosting: managed`, число голосов |
| `roles` | роли узла в этом варианте (у Raft — `voter` и `witness`) |
| `capacity`, `cost` | ёмкость на экземпляр и цена (`multiplier` к прайсу, `monthlyFixed`): управляемый сервис дороже, но без эксплуатации |
| `versions` | **своя** таблица выпусков (Amazon RDS живёт по датам AWS, а не сообщества PostgreSQL). Пустая строка — таблицы нет, срок поддержки не проверяется |
| `default` | вариант, который предполагается, пока узел не выбрал другой |

Свойство вида может показываться только для части вариантов (`PropDef.variants`): например, `dcs` (хранилище кворума) нужен Patroni и не нужен RDS.
Инспектор показывает только свойства выбранного варианта. Значение, оставшееся в узле от другого варианта, не удаляется и движком читается как заданное — после смены варианта лишнее лучше очистить.

**Пример — PostgreSQL.** Восемь вариантов в одном виде:

| Вариант | Что это | Как видит движок |
|---|---|---|
| `patroni` ★ | Patroni + etcd/Consul | автопереключение, нужен DCS (`dcs-capable`) |
| `repmgr` | repmgr + witness | автопереключение по голосам узлов и witness, медленнее |
| `streaming` | потоковая репликация | переключение вручную (`failover: manual`) |
| `cloudnativepg` | оператор в Kubernetes | `platform-quorum`: лидера выбирает API Kubernetes |
| `rds`, `aurora`, `azure`, `cloud` | управляемые сервисы | `managed`: отказоустойчивость на стороне провайдера, цена ×1,6–1,9; у RDS, Aurora и Azure своя таблица версий, у прочих облачных её нет |

## Кворум и арбитры: кто решает, кому быть лидером

Автоматическое переключение без арбитра небезопасно: при разрыве сети обе стороны назначат себе лидера (**split-brain**). Движок различает четыре
вида «власти» над переключением (`graph.rs::Authority`, настройка `engine.authority` — какие признаки что значат):

| Власть | Откуда | Что делает движок |
|---|---|---|
| **Внешняя** (`External`) | свойство `dcs` указывает на кластер консенсуса (etcd, Consul, ZooKeeper) | кворум этого кластера; в `dcs` можно сослаться только на кластер, вид которого имеет признак `dcs-capable` (`PropDef.refTrait`) |
| **Собственная** (`SelfVotes`) | в юните есть witness или признак `self-quorum` (Redis Sentinel/Cluster, встроенный Raft) | голоса самого юнита; кворум считается по членам и witness |
| **Провайдер** (`Provider`) | все узлы с признаком `managed` или `platform-quorum` | провайдер решает сам: считается, что он остаётся на большей стороне разделения, изолированный primary останавливается (fencing); split-brain не создаётся |
| **Нет** | ничего из перечисленного | автопереключение небезопасно (`HA-DB-02`), либо только ручное |

Поэтому правило `HA-DB-02` не требует от Amazon RDS отдельного etcd, а `HA-QRM-*` учитывает голоса юнитов с собственным кворумом.
Список признаков, которые считаются «собственным кворумом» и «провайдером», редактируется в `engine.json` (`authority.selfQuorumTraits`, `authority.providerTraits`).

## Версии и жизненный цикл

Для каждого продукта (и для варианта со своей таблицей) есть **таблица выпусков** — элемент `catalog.versions`, `id` которого совпадает с `KindDef.versions`
(по умолчанию — `id` вида):

```json
{ "id": "postgresql", "name": "PostgreSQL", "source": "https://endoflife.date/api/postgresql.json", "asOf": "2026-10-13",
  "cycles": [
    { "cycle": "18", "releaseDate": "2025-09-25", "eol": "2030-11-14", "latest": "18.6", "latestReleaseDate": "2026-08-11" },
    { "cycle": "17", "releaseDate": "2024-09-26", "eol": "2029-11-08", "latest": "17.11", "latestReleaseDate": "2026-08-10" } ] }
```

Дата в `eol` — конец исправлений безопасности; `support` (есть не у всех продуктов) — конец активной поддержки, после которой идут только исправления безопасности; `lts` — линия с долгой поддержкой. Вместо даты источник может сказать `true` («закончилось, дата
неизвестна») или `false` («не закончилось / не известно»). Фаза линии на дату:

| Фаза | Значение |
|---|---|
| `supported` | активная поддержка |
| `maintenance` | активная поддержка кончилась, исправления безопасности ещё выходят |
| `eol` | исправлений безопасности больше нет |
| `unknown` | таблица не знает дат |

* **Движок не знает времени.** Дата — аргумент (`EngineConfig.today`): сервер подставляет сегодняшнюю (UTC), тесты — фиксированную. Поэтому отчёт воспроизводим.
* **Судится только заявленное.** Версию проверяет правило, только если она **указана в узле**: версия «по умолчанию» из каталога — допущение, а не факт о вашей системе.
  Не указана — `VER-01` молчит (`onMissing: skip`, по умолчанию) или просит заполнить (`onMissing: no_data`, кнопка ведёт к полю). Версия, которой нет в таблице, — `no_data` (`onUnknown`).
* **Рекомендуемая линия** — среди линий, которые ещё получают исправления безопасности, самая новая из тех, что вышли не менее `matureDays` назад (свежий `x.0` не рекомендация),
  с предпочтением LTS (`preferLts`). Если созревших нет — самая новая из поддерживаемых. Редактор показывает её рядом с версией, а исправление `set_version` ставит в узел.
* **Устаревшая таблица** — не повод молчать. Если таблица снята раньше, чем `staleDays` дней назад, `VER-01` честно говорит «данные устарели» (`no_data`),
  а не выдаёт зелёное по старым датам.

### Правила

| Правило | Проверка | Что ловит |
|---|---|---|
| `VER-01` | `version_lifecycle` | версия вышла из поддержки (критично); поддержка закончится в ближайшие `warnDays` дней (риск); версия неизвестна таблице или таблица устарела (`нет данных`). Параметры правила: `warnDays`, `onMissing`, `onUnknown` — они важнее настроек движка |
| `VER-02` | `version_skew` | члены одного кластера работают на разных линиях выпуска (риск при обновлении и репликации) |

Свойства **ОС и среды выполнения** тоже проверяются: у `PropDef` есть `lifecycle` — id таблицы (`ubuntu`) или ссылка `@key`: таблицу называет **значение другого свойства того же узла**
(`osVersion` у пула ВМ с `lifecycle: "@os"` судится по таблице той ОС, что выбрана в `os`; `runtimeVersion` у функций — по `@runtime`).

### Настройки

| Где | Ключ | Что |
|---|---|---|
| `engine.json` → `lifecycle` | `warnDays` (180) | за сколько дней до конца поддержки предупреждать |
| | `matureDays` (90) | сколько дней новая линия считается «ещё не созревшей» для рекомендации |
| | `preferLts` (true) | предпочитать LTS |
| | `staleDays` (120) | через сколько дней таблица считается устаревшей |
| Настройки (группа «Версии и жизненный цикл») | `versions.autoSyncDays` (7) | обновлять таблицы каждые N дней (0 — вручную) |
| | `versions.autoSyncCheckMinutes` (60) | как часто проверять, пора ли |
| | `versions.sourceBases` | **разрешённые источники** (по умолчанию `https://endoflife.date/api/`) |
| | `versions.syncTimeoutSeconds` (20), `versions.syncParallel` (6) | ожидание ответа и число параллельных запросов |
| | `versions.maxAgeYears` (8) | сколько лет хранить уже закончившиеся линии |

### Обновление таблиц

Таблицы поставляются снятыми с [endoflife.date](https://endoflife.date) на день сборки образа. Дальше их можно обновлять тремя способами:

1. **Кнопка** — Администрирование → Каталог → **Версии** → «Обновить все» или «Обновить эту таблицу». Сверху: сколько таблиц устарело, когда и кем (вручную / по расписанию) было последнее обновление и что не удалось.
   Обновление **заменяет линии выпуска** таблицы, поэтому ручные правки таблицы с источником будут перезаписаны (панель просит подтверждение); таблицу, которую вы ведёте руками, пометьте `source: manual`.
2. **По расписанию** — раз в `versions.autoSyncDays` дней. Реплик может быть много: «кто обновляет» решается условной записью времени в таблицу `meta` (одна реплика забирает слот, остальные видят
   его занятым), поэтому внешний сервис не получает N одинаковых запросов. Итог последнего обновления (кто, когда, что не удалось) виден в админ-панели и в журнале аудита (`versions.sync`).
3. **Из командной строки** — `python3 deploy/versions-sync.py` обновляет `versions.json` в репозитории (так делает плановая задача CI, см. [DEVELOPMENT.md](DEVELOPMENT.md)).

Сервер обновляет только таблицы с адресом `http…` в `source`; таблица с `source: manual` ведётся руками и не трогается. После успешного обновления у таблицы новые `cycles` и `asOf`, ревизия конфигурации растёт — все реплики видят её за секунды.

Защиты:

* **SSRF.** Сервер ходит только по адресам, начинающимся с одного из `versions.sourceBases` (совпадают схема, хост и порт; логин и пароль в адресе запрещены). Администратор может
  отредактировать `source` таблицы, но не заставит сервер обратиться к внутреннему сервису. В закрытой сети в `sourceBases` указывают внутреннее зеркало.
* **Плохой ответ не опустошает таблицу.** Нет линий, не JSON, слишком большой ответ (> 4 МБ), код ≠ 2xx — таблица остаётся прежней, ошибка попадает в итог.
* **Свежее не затирается старым.** Обновлённая таблица становится значением по умолчанию этой установки; при обновлении приложения поставляемая таблица заменяет хранимую,
  только если она **новее** (`asOf`). Вручную изменённая таблица остаётся вашей, а в списке появляется метка «доступна новая версия по умолчанию».
* **Без сети всё работает.** Нет доступа к источнику — используются поставленные таблицы, а `VER-01` по `staleDays` честно предупредит, что данные устарели.

## Что видит пользователь

* **Библиотека** сгруппирована по функциям («Реляционные БД», «Кэши»…): продукты одной работы лежат рядом, в подсказке — число вариантов и их названия. Поиск находит продукт и по названию
  варианта («Aurora» → PostgreSQL): плитка подписана найденным вариантом, и клик добавляет узел сразу с ним.
* **Инспектор** — раздел «Продукт»: выбор варианта (описание; метка «управляемый сервис», когда провайдер отвечает за переключение) и версии. Версия — список линий выпуска с датами конца поддержки
  (до даты, «только исправления безопасности», «поддержка закончилась»), отметкой LTS и подсказкой «Рекомендуется …» с кнопкой «Выбрать». Если таблица старше `staleDays`, под полем стоит предупреждение.
  Значения по умолчанию варианта подставляются в **пустые** свойства: заданное руками смена варианта не затирает. Смена продукта убирает `variant` и `version` (они значат что-то только для того продукта, для которого выбраны) и сообщает, какие свойства исчезли.
* **Находки `VER-01` / `VER-02`** выглядят как любые другие: подсвечивают узлы, ведут к полю версии, предлагают исправление «поставить рекомендуемую линию».
* **Мастер размещения** ([PLACEMENT.md](PLACEMENT.md)) предлагает для каждого слоя все продукты его функции и варианты каждого.
* **ИИ** получает компактный «цифровой» каталог (общие свойства один раз, у вида — только своё): функция, поведение, признаки и строки вариантов `id: Название [default] [+managed]`.
  Модель выбирает продукт и ставит `props.variant`; движок проверяет результат так же, как правку человека.

## Как добавить…

**Продукт** — Администрирование → Каталог → «Новый элемент» (или «Копия» похожего): `function`, `behavior`, `traits`, `roles`, `props`, `capacity`, `cost`. Если функция новая, объявите её в категории.
Продукт без таблицы выпусков допустим: `versions: ""` — срок поддержки не проверяется.

**Вариант** — в `variants` вида: `id`, `name`, `description`, при необходимости `behavior`, `addTraits`, `defaults`, `versions`. Один вариант — `default: true`.

**Таблицу версий** — Каталог → Версии → «Новый элемент»: `id` (равный `versions` вида или варианта), `source` (адрес в `versions.sourceBases` или `manual`) и линии. Для «ручной» таблицы
срок поддержки задаёте вы; для таблицы с источником — кнопка «Обновить».

**Встроенное значение по умолчанию** (чтобы продукт был в чистой установке) — `deploy/catalog/`: каталог собирается из исходников на Python (`part_*.py`), так сотня продуктов остаётся согласованной.
Правило: правите исходник → `python3 deploy/catalog/build.py` → `--check` в CI убеждается, что `catalog.json` и названия таблиц актуальны. Подробности — [README каталога](../deploy/catalog/README.md).

## Каталог по умолчанию

Таблица ниже генерируется из `catalog.json` и `versions.json` (`python3 deploy/gen-docs.py`). ★ — вариант по умолчанию.

<!-- BEGIN GENERATED: catalog -->
Видов: **105**, вариантов: **207**, функций: **49**. «Версии» — таблица выпусков, по которой проверяется срок поддержки (у варианта она может быть своей; прочерк — таблицы нет).

### Сеть и вход

**Пользователи и клиенты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `users` — Пользователи | `stateless` | — | — |

**Глобальная балансировка (GSLB)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `gslb` — GSLB / DNS | `stateless` | Свои DNS-серверы (PowerDNS / BIND + проверки) ★, Amazon Route 53, Cloudflare Load Balancing, Другой облачный DNS / Traffic Manager | — |

**Серверы DNS**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `dns_server` — DNS-сервер | `stateless` | BIND 9 ★, PowerDNS, CoreDNS, Облачная зона (Route 53, Cloud DNS…) | BIND 9 |

**CDN**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cdn` — CDN | `stateless` | Cloudflare ★, Amazon CloudFront, Akamai, Fastly, Региональная CDN (Gcore, Yandex Cloud CDN, Selectel…), Свои кэширующие узлы (Varnish / NGINX) | — |

**HTTP-кэш**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `varnish` — Varnish Cache | `stateless` | — | Varnish Cache |

**WAF**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `waf` — WAF | `stateless` | Облачный WAF (Cloudflare, AWS WAF, Imperva…) ★, ModSecurity / Coraza + OWASP CRS, NGINX App Protect, Аппаратный / виртуальный (F5, FortiWeb, Imperva) | — |

**Балансировщики L7 и ingress**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `l7` — L7 / Ingress | `stateless` | На своих серверах (VRRP / keepalived) ★, Ingress-контроллер в Kubernetes, Облачный L7 (ALB, Application Gateway, Cloud Load Balancing), Аппаратный / виртуальный (F5, Citrix ADC, A10) | — |
| `nginx` — NGINX | `stateless` | На своих серверах (VRRP / keepalived) ★, Ingress-контроллер в Kubernetes | NGINX |
| `haproxy` — HAProxy | `stateless` | На своих серверах (VRRP / keepalived) ★, Ingress-контроллер в Kubernetes | HAProxy |
| `envoy` — Envoy | `stateless` | Граничный прокси на виртуальных машинах ★, Envoy Gateway (Gateway API в Kubernetes) | Envoy |
| `traefik` — Traefik | `stateless` | Ingress-контроллер в Kubernetes ★, Отдельные узлы | Traefik |
| `caddy` — Caddy | `stateless` | — | Caddy |
| `apache_httpd` — Apache HTTP Server | `stateless` | — | Apache HTTP Server |

**Балансировщики L4**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `l4` — LB / VRRP | `stateless` | VRRP + LVS / HAProxy ★, Anycast + BGP (ECMP), Облачный L4 (NLB, Load Balancer) | — |
| `metallb` — MetalLB | `stateless` | Режим L2 (ARP / NDP) ★, Режим BGP (ECMP) | MetalLB |

**Прокси**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `squid` — Squid | `stateless` | — | Squid |

**Межсетевые экраны**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `firewall` — Межсетевой экран | `stateless` | OPNsense ★, Fortinet FortiGate, Palo Alto Networks (PAN-OS), pfSense, Cisco Secure Firewall (ASA / FTD), Облачный межсетевой экран (AWS Network Firewall, Azure Firewall) | OPNsense |

**Сетевое оборудование**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `network_device` — Маршрутизатор / коммутатор | `stateless` | Cisco IOS XE ★, Arista EOS, Juniper Junos, SONiC / Cumulus (open networking) | Cisco IOS XE |

**VPN и каналы между площадками**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vpn_dci` — VPN / DCI | `stateless` | IPsec site-to-site ★, WireGuard, OpenVPN, Облачный VPN (AWS Site-to-Site, Cloud VPN, VPN Gateway), Выделенный канал (Direct Connect, ExpressRoute, оптика, L2), MPLS / SD-WAN | — |

### Вычисления

**Kubernetes**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kubernetes` — Kubernetes | `stateless` | Свой кластер (kubeadm / RKE2 / k0s) ★, Red Hat OpenShift, Rancher (RKE2), Amazon EKS, Google GKE, Azure AKS | Kubernetes |

**Оркестраторы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `nomad` — HashiCorp Nomad | `stateless` | — | HashiCorp Nomad |

**Виртуальные машины**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vm_pool` — Пул ВМ | `stateless` | VMware vSphere (ESXi) ★, Proxmox VE, XCP-ng, oVirt / RHV, Nutanix AHV, KVM / libvirt, Облачные виртуальные машины (EC2, Compute Engine, Yandex Compute…) | VMware ESXi |

**Функции (serverless)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `functions` — Functions | `stateless` | Облачные функции (Lambda, Cloud Functions, Yandex Cloud Functions) ★, Knative, OpenFaaS | — |

**API-шлюзы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `api_gateway` — API Gateway | `stateless` | Свой шлюз (Envoy, Tyk, KrakenD…) ★, Облачный шлюз (API Gateway, Azure APIM, Apigee) | — |
| `kong` — Kong Gateway | `stateless` | DB-less (конфигурация из Git) ★, С базой данных PostgreSQL, Hybrid (control plane + data plane) | Kong Gateway |
| `apisix` — Apache APISIX | `stateless` | Конфигурация в etcd ★, Автономный режим (файл) | Apache APISIX |

**Обработка данных**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `airflow` — Apache Airflow | `stateless` | — | Apache Airflow |
| `spark` — Apache Spark | `stateless` | — | Apache Spark |
| `flink` — Apache Flink | `stateless` | — | Apache Flink |
| `nifi` — Apache NiFi | `stateless` | — | Apache NiFi |

### Данные

**Реляционные СУБД**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `postgresql` — PostgreSQL | `primary_replica` | Patroni + etcd/Consul ★, repmgr + witness, Потоковая репликация, ручное переключение, CloudNativePG в Kubernetes, Amazon RDS Multi-AZ, Amazon Aurora PostgreSQL, Azure Database for PostgreSQL, Другой облачный PostgreSQL | PostgreSQL |
| `mysql` — MySQL | `primary_replica` | InnoDB Cluster (Group Replication + MySQL Router) ★, Асинхронная репликация + Orchestrator, Полусинхронная репликация, ручное переключение, Percona XtraDB Cluster (Galera), Amazon RDS for MySQL (Multi-AZ), Amazon Aurora MySQL, Azure Database for MySQL, Другой облачный MySQL | MySQL |
| `mariadb` — MariaDB | `primary_replica` | MariaDB Galera Cluster ★, Репликация + MaxScale, Полусинхронная репликация, ручное переключение, Amazon RDS for MariaDB (Multi-AZ), Другой облачный MariaDB | MariaDB |
| `mssql` — Microsoft SQL Server | `primary_replica` | Always On группы доступности (WSFC) ★, Кластер экземпляров (общее хранилище), Доставка журналов, ручное переключение, Облачный SQL Server (Azure SQL MI, Amazon RDS) | Microsoft SQL Server |
| `oracle` — Oracle Database | `primary_replica` | Data Guard + Fast-Start Failover ★, Data Guard, ручное переключение, Real Application Clusters (RAC), Облачный Oracle (OCI, Amazon RDS for Oracle) | Oracle Database |

**Распределённые SQL**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cockroachdb` — CockroachDB | `quorum_log` | Свой кластер ★, CockroachDB Cloud | CockroachDB |
| `yugabytedb` — YugabyteDB | `quorum_log` | Свой кластер ★, YugabyteDB Aeon | — |
| `tidb` — TiDB | `quorum_log` | Свой кластер (TiUP / оператор) ★, TiDB Cloud | — |
| `vitess` — Vitess | `primary_replica` | — | Vitess |

**Документные базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `mongodb` — MongoDB | `primary_replica` | Реплика-сет ★, Шардированный кластер, MongoDB Atlas, Amazon DocumentDB, Другой облачный MongoDB | MongoDB |
| `couchbase` — Couchbase | `quorum_log` | — | Couchbase Server |
| `couchdb` — Apache CouchDB | `quorum_log` | — | Apache CouchDB |
| `arangodb` — ArangoDB | `quorum_log` | — | ArangoDB |

**Графовые базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `neo4j` — Neo4j | `quorum_log` | — | Neo4j |

**Колоночные базы (wide-column)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cassandra` — Apache Cassandra | `quorum_log` | Свой кластер ★, K8ssandra (оператор в Kubernetes), Облачный Cassandra (Amazon Keyspaces, Astra DB) | Apache Cassandra |
| `scylladb` — ScyllaDB | `quorum_log` | Свой кластер ★, ScyllaDB Cloud | — |
| `hbase` — Apache HBase | `quorum_log` | — | Apache HBase |

**Ключ-значение**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `foundationdb` — FoundationDB | `quorum_log` | — | FoundationDB |
| `tarantool` — Tarantool | `primary_replica` | Встроенный Raft (election_mode) ★, Master-replica, ручное переключение | Tarantool |

**Временные ряды**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `influxdb` — InfluxDB | `primary_replica` | Core (один узел + копии) ★, Enterprise (кластер) | InfluxDB |

**Кэши**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `redis` — Redis | `primary_replica` | Один узел ★, Sentinel (master + реплики), Redis Cluster (шарды), Amazon ElastiCache, Другой облачный Redis | Redis |
| `valkey` — Valkey | `primary_replica` | Один узел ★, Sentinel (master + реплики), Кластер (шарды), Amazon ElastiCache for Valkey, Другой облачный Valkey / Redis | Valkey |
| `memcached` — Memcached | `stateless` | Свои узлы ★, Amazon ElastiCache for Memcached | Memcached |

**Очереди и потоки**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kafka` — Kafka | `quorum_log` | KRaft (без ZooKeeper) ★, С ZooKeeper (устаревает), Amazon MSK, Confluent Cloud / другой облачный Kafka | Apache Kafka |
| `rabbitmq` — RabbitMQ | `quorum_log` | Кластер с quorum-очередями ★, Amazon MQ for RabbitMQ, Облачный RabbitMQ (CloudAMQP и др.) | RabbitMQ |
| `pulsar` — Apache Pulsar | `quorum_log` | Свой кластер ★, StreamNative / облачный Pulsar | Apache Pulsar |
| `activemq` — Apache ActiveMQ | `primary_replica` | Classic, общее хранилище (блокировка) ★, Artemis, общее хранилище, Artemis, репликация + кворум, Amazon MQ for ActiveMQ | Apache ActiveMQ Classic |
| `ibm_mq` — IBM MQ | `primary_replica` | Native HA (Raft, 3 реплики) ★, RDQM (DRBD + Pacemaker), Multi-instance (общий диск), Облачный IBM MQ | IBM MQ |
| `nats` — NATS | `quorum_log` | — | — |
| `redpanda` — Redpanda | `quorum_log` | Свой кластер ★, Redpanda Cloud | — |

**Поиск**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `opensearch` — OpenSearch | `quorum_log` | Выделенные мастер-узлы ★, Совмещённые роли, Amazon OpenSearch Service | OpenSearch |
| `elasticsearch` — Elasticsearch | `quorum_log` | Выделенные мастер-узлы ★, Совмещённые роли, Elastic Cloud | Elasticsearch |
| `solr` — Apache Solr | `quorum_log` | — | Apache Solr |
| `meilisearch` — Meilisearch | `primary_replica` | — | Meilisearch |

**Аналитические базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `clickhouse` — ClickHouse | `quorum_log` | Реплики (ClickHouse Keeper) ★, Шарды + реплики, ClickHouse Cloud / другой управляемый | ClickHouse |

**Объектные хранилища**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `object_storage` — Объектное хранилище | `stateless` | MinIO (distributed, erasure coding) ★, Ceph RGW, Облачное S3 (AWS S3, Yandex Object Storage…) | — |

**Хранилища (SDS, СХД)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `ceph` — Ceph | `quorum_log` | Развёртывание cephadm ★, Rook в Kubernetes | Ceph |
| `longhorn` — Longhorn | `stateless` | — | Longhorn |
| `nas_san` — Дисковый массив / NAS | `primary_replica` | NetApp ONTAP MetroCluster ★, NetApp ONTAP SnapMirror, TrueNAS, Другой массив (Dell, HPE, Huawei…) | NetApp ONTAP |

**Резервное копирование**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `backup_storage` — Бэкапы (offsite) | `stateless` | Облачное хранилище с Object Lock ★, Veeam Backup & Replication, Proxmox Backup Server, Commvault, restic / Borg + хранилище, Ленточная библиотека | — |

### Платформа

**Консенсус и координация**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `etcd` — etcd | `consensus` | Выделенный кластер ★, На узлах control-plane Kubernetes | etcd |
| `consul` — Consul | `consensus` | — | HashiCorp Consul |
| `zookeeper` — Apache ZooKeeper | `consensus` | — | Apache ZooKeeper |

**Секреты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vault` — Vault | `primary_replica` | Репликация (DR / performance) ★, Встроенный Raft (HA-кластер), HCP Vault / облачный сервис секретов | HashiCorp Vault |
| `openbao` — OpenBao | `consensus` | — | OpenBao |

**Мониторинг**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `monitoring` — Мониторинг | `stateless` | Prometheus HA (пара) ★, VictoriaMetrics (кластер), Облачный мониторинг (Managed Prometheus, Grafana Cloud) | Prometheus |
| `zabbix` — Zabbix | `primary_replica` | Кластер active/standby ★, Один сервер | Zabbix |
| `icinga` — Icinga | `stateless` | — | Icinga |
| `checkmk` — Checkmk | `stateless` | — | Checkmk |

**Сбор телеметрии**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `telegraf` — Telegraf | `stateless` | — | Telegraf |
| `fluent_bit` — Fluent Bit | `stateless` | — | Fluent Bit |
| `otel_collector` — OpenTelemetry Collector | `stateless` | — | — |

**Логи**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `loki` — Grafana Loki | `stateless` | — | Grafana Loki |
| `graylog` — Graylog | `stateless` | — | Graylog |
| `logstash` — Logstash | `stateless` | — | Logstash |
| `kibana` — Kibana | `stateless` | — | Kibana |

**Трассировка**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `jaeger` — Jaeger | `stateless` | — | Jaeger |

**Дашборды**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `grafana` — Grafana | `stateless` | Свой Grafana ★, Grafana Cloud | Grafana |

**GitOps**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `gitops` — GitOps | `stateless` | Argo CD ★, Flux | Argo CD |

**CI/CD и Git-хостинг**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `jenkins` — Jenkins | `stateless` | — | Jenkins |
| `gitlab` — GitLab | `stateless` | Свой GitLab ★, GitLab.com / GitLab Dedicated | GitLab |
| `forgejo` — Forgejo | `stateless` | — | Forgejo |

**Качество кода**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `sonarqube` — SonarQube | `stateless` | — | SonarQube |

**Реестры артефактов**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `artifactory` — JFrog Artifactory | `stateless` | — | JFrog Artifactory |
| `nexus` — Sonatype Nexus Repository | `stateless` | — | Sonatype Nexus Repository |
| `harbor` — Harbor | `stateless` | Свой Harbor ★ | Harbor |

**Идентификация**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `keycloak` — Keycloak | `stateless` | Кластер на одной площадке ★, Несколько площадок (active/active) | Keycloak |
| `authentik` — authentik | `stateless` | — | authentik |

**Доступ к инфраструктуре**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `teleport` — Teleport | `stateless` | Свой кластер ★, Teleport Enterprise Cloud | Teleport |
| `boundary` — HashiCorp Boundary | `stateless` | — | HashiCorp Boundary |

**Сертификаты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cert_manager` — cert-manager | `stateless` | — | cert-manager |

**Политики**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kyverno` — Kyverno | `stateless` | — | Kyverno |
| `gatekeeper` — OPA Gatekeeper | `stateless` | — | OPA Gatekeeper |

**Сеть кластера**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cni` — Сеть кластера (CNI) | `stateless` | Cilium (eBPF) ★, Calico, Flannel, CNI провайдера (VPC CNI, Azure CNI) | Cilium |

**Сервисная сетка**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `service_mesh` — Сервисная сетка | `stateless` | Istio ★, Linkerd, Cilium Service Mesh, Consul Connect | Istio |
<!-- END GENERATED: catalog -->

## Таблицы выпусков по умолчанию

<!-- BEGIN GENERATED: versions -->
Таблиц: **139**, данные на **2026-10-13** (самая старая — 2026-10-13). «LTS» — есть линия с долгой поддержкой.

| Таблица | Продукт | Линий | Новейшая линия | LTS |
|---|---|---|---|---|
| `activemq` | Apache ActiveMQ Classic | 8 | `6.3` | — |
| `airflow` | Apache Airflow | 5 | `3.3` | — |
| `almalinux` | AlmaLinux | 3 | `10` | — |
| `alpine` | Alpine Linux | 16 | `3.24` | — |
| `amazon-aurora-mysql` | Amazon Aurora MySQL | 2 | `8.4` | — |
| `amazon-aurora-postgresql` | Amazon Aurora PostgreSQL | 8 | `18` | — |
| `amazon-documentdb` | Amazon DocumentDB | 4 | `8.0` | да |
| `amazon-eks` | Amazon EKS | 20 | `1.37` | — |
| `amazon-elasticache-redis` | Amazon ElastiCache for Redis | 3 | `7` | — |
| `amazon-linux` | Amazon Linux | 1 | `2023` | — |
| `amazon-mq-activemq` | Amazon MQ for ActiveMQ | 4 | `5.19` | — |
| `amazon-mq-rabbitmq` | Amazon MQ for RabbitMQ | 4 | `4.3` | — |
| `amazon-msk` | Amazon MSK | 19 | `4.2` | — |
| `amazon-opensearch` | Amazon OpenSearch Service | 16 | `3.5` | — |
| `amazon-rds-mariadb` | Amazon RDS for MariaDB | 8 | `12.3` | — |
| `amazon-rds-mysql` | Amazon RDS for MySQL | 16 | `8.4.9` | — |
| `amazon-rds-postgresql` | Amazon RDS for PostgreSQL | 8 | `18` | — |
| `apache` | Apache HTTP Server | 1 | `2.4` | — |
| `apisix` | Apache APISIX | 26 | `3.19` | да |
| `arangodb` | ArangoDB | 9 | `3.12` | — |
| `argo_cd` | Argo CD | 30 | `3.5` | — |
| `artemis` | Apache ActiveMQ Artemis | 46 | `2.57` | — |
| `artifactory` | JFrog Artifactory | 30 | `7.161` | — |
| `authentik` | authentik | 12 | `2026.8` | — |
| `azure-database-for-mysql` | Azure Database for MySQL | 2 | `8.4` | — |
| `azure-database-for-postgresql` | Azure Database for PostgreSQL | 8 | `18` | — |
| `azure-kubernetes-service` | Azure Kubernetes Service | 16 | `1.36` | — |
| `bind` | BIND 9 | 4 | `9.21` | да |
| `boundary` | HashiCorp Boundary | 6 | `0.21` | — |
| `caddy` | Caddy | 2 | `2` | — |
| `calico` | Calico | 9 | `3.33` | — |
| `cassandra` | Apache Cassandra | 3 | `5.0` | — |
| `ceph` | Ceph | 7 | `20` | — |
| `cert_manager` | cert-manager | 12 | `1.21` | да |
| `checkmk` | Checkmk | 6 | `2.5` | — |
| `cilium` | Cilium | 8 | `1.20` | — |
| `cisco_ios_xe` | Cisco IOS XE | 20 | `26.1` | да |
| `clickhouse` | ClickHouse | 21 | `26.9` | да |
| `cockroachdb` | CockroachDB | 21 | `26.3` | да |
| `commvault` | Commvault | 7 | `11.46` | да |
| `consul` | HashiCorp Consul | 18 | `2.0` | да |
| `couchbase` | Couchbase Server | 8 | `8.0` | — |
| `couchdb` | Apache CouchDB | 4 | `3.5` | — |
| `debian` | Debian | 4 | `13` | — |
| `dotnet` | .NET | 9 | `10` | да |
| `eclipse-temurin` | Eclipse Temurin (Java) | 14 | `27` | да |
| `elasticsearch` | Elasticsearch | 11 | `9.5` | — |
| `envoy` | Envoy | 31 | `1.39` | — |
| `esxi` | VMware ESXi | 4 | `9.1` | — |
| `etcd` | etcd | 4 | `3.7` | — |
| `flink` | Apache Flink | 8 | `2.3` | да |
| `fluent_bit` | Fluent Bit | 21 | `5.1` | — |
| `flux` | Flux | 11 | `2.9` | — |
| `forgejo` | Forgejo | 10 | `16.0` | да |
| `fortigate` | FortiOS (FortiGate) | 7 | `8.0` | — |
| `foundationdb` | FoundationDB | 3 | `7.3` | — |
| `gatekeeper` | OPA Gatekeeper | 13 | `3.23` | — |
| `gitlab` | GitLab | 67 | `19.4` | — |
| `go` | Go | 16 | `1.27` | — |
| `google-kubernetes-engine` | Google Kubernetes Engine | 20 | `1.36` | — |
| `grafana` | Grafana | 29 | `13.2` | — |
| `graylog` | Graylog | 18 | `7.1` | — |
| `haproxy` | HAProxy | 16 | `3.4` | да |
| `harbor` | Harbor | 18 | `2.15` | — |
| `hbase` | Apache HBase | 7 | `3.0` | — |
| `ibm_mq` | IBM MQ | 4 | `10.0` | — |
| `icinga` | Icinga | 6 | `2.16` | — |
| `influxdb` | InfluxDB | 15 | `3.12` | — |
| `istio` | Istio | 25 | `1.31` | — |
| `jaeger` | Jaeger | 1 | `2` | — |
| `jenkins` | Jenkins | 19 | `2.568` | да |
| `kafka` | Apache Kafka | 22 | `4.3` | — |
| `keycloak` | Keycloak | 28 | `26.8` | — |
| `kibana` | Kibana | 13 | `9.5` | — |
| `kong_gateway` | Kong Gateway | 11 | `3.9` | да |
| `kubernetes` | Kubernetes | 22 | `1.37` | — |
| `kyverno` | Kyverno | 12 | `1.19` | — |
| `logstash` | Logstash | 10 | `9.5` | — |
| `loki` | Grafana Loki | 25 | `3.7` | — |
| `longhorn` | Longhorn | 13 | `1.13` | — |
| `mariadb` | MariaDB | 22 | `13.0` | да |
| `meilisearch` | Meilisearch | 55 | `1.54` | — |
| `memcached` | Memcached | 1 | `1.6` | — |
| `metallb` | MetalLB | 4 | `0.16` | — |
| `mongodb` | MongoDB | 19 | `9.0` | — |
| `mssqlserver` | Microsoft SQL Server | 7 | `17.0` | — |
| `mysql` | MySQL | 12 | `9.7` | да |
| `neo4j` | Neo4j | 53 | `2026.09` | да |
| `netapp-ontap` | NetApp ONTAP | 15 | `9.19.1` | — |
| `nexus` | Sonatype Nexus Repository | 68 | `3.96` | — |
| `nginx` | NGINX | 15 | `1.31` | — |
| `nifi` | Apache NiFi | 23 | `2.12` | — |
| `nodejs` | Node.js | 16 | `26` | да |
| `nomad` | HashiCorp Nomad | 14 | `2.0` | да |
| `nutanix-aos` | Nutanix AOS | 19 | `7.6` | да |
| `openbao` | OpenBao | 8 | `2.7` | — |
| `opensearch` | OpenSearch | 3 | `3` | — |
| `openshift` | Red Hat OpenShift | 22 | `4.22` | — |
| `openvpn` | OpenVPN | 3 | `2.7` | — |
| `opnsense` | OPNsense | 16 | `26.7` | — |
| `oracle-linux` | Oracle Linux | 3 | `10` | — |
| `oracle_database` | Oracle Database | 3 | `23` | да |
| `ovirt` | oVirt | 3 | `4.5` | — |
| `palo_alto` | PAN-OS (Palo Alto Networks) | 11 | `12.2` | — |
| `php` | PHP | 8 | `8.5` | — |
| `postgresql` | PostgreSQL | 8 | `18` | — |
| `prometheus` | Prometheus | 36 | `3.15` | да |
| `proxmox_backup` | Proxmox Backup Server | 4 | `4` | — |
| `proxmox_ve` | Proxmox VE | 4 | `9` | — |
| `pulsar` | Apache Pulsar | 15 | `5.0` | да |
| `python` | Python | 8 | `3.15` | — |
| `rabbitmq` | RabbitMQ | 10 | `4.3` | — |
| `rancher` | Rancher | 14 | `2.15` | — |
| `redis` | Redis | 12 | `8.10` | — |
| `rhel` | Red Hat Enterprise Linux | 3 | `10` | да |
| `rocky-linux` | Rocky Linux | 3 | `10` | — |
| `ruby` | Ruby | 8 | `4.0` | — |
| `sles` | SUSE Linux Enterprise | 10 | `16.0` | — |
| `solr` | Apache Solr | 3 | `10` | — |
| `sonarqube` | SonarQube | 14 | `2026.5` | да |
| `spark` | Apache Spark | 10 | `4.2` | да |
| `spring-boot` | Spring Boot | 15 | `4.1` | — |
| `squid` | Squid | 3 | `7` | — |
| `tarantool` | Tarantool | 18 | `3.8` | да |
| `telegraf` | Telegraf | 32 | `1.40` | — |
| `teleport` | Teleport | 3 | `18` | — |
| `tomcat` | Apache Tomcat | 4 | `11.0` | — |
| `traefik` | Traefik | 20 | `3.7` | — |
| `truenas` | TrueNAS | 5 | `25.10` | — |
| `ubuntu` | Ubuntu | 16 | `26.04` | да |
| `valkey` | Valkey | 5 | `9.1` | — |
| `varnish` | Varnish Cache | 12 | `9.1` | да |
| `vault` | HashiCorp Vault | 15 | `2.1` | да |
| `veeam_backup` | Veeam Backup & Replication | 4 | `13` | — |
| `vitess` | Vitess | 12 | `24` | — |
| `windows-server` | Windows Server | 10 | `2025` | да |
| `xcp_ng` | XCP-ng | 5 | `8.3` | да |
| `zabbix` | Zabbix | 8 | `7.4` | да |
| `zookeeper` | Apache ZooKeeper | 5 | `3.9` | — |
<!-- END GENERATED: versions -->

## Подводные камни

| Камень | Решение |
|---|---|
| Версия «16» из каталога давно закончила поддержку | дата — аргумент движка; таблицы обновляются кнопкой, расписанием или из CI; устаревшая таблица сама превращает вердикт в «нет данных» |
| Таблицы не получилось скачать (закрытая сеть, источник упал) | поставляемые таблицы работают офлайн; `sourceBases` указывает на внутреннее зеркало; неудача одной таблицы не трогает остальные |
| Админ подменил `source` на внутренний адрес (SSRF) | только префиксы из `versions.sourceBases`, без логина и пароля в адресе |
| Реплики обновляют таблицы одновременно и спамят источник | слот обновления захватывается записью в `meta`; свежая таблица не перезаписывается старой при посеве |
| Управляемый сервис наследовал «сообщественные» даты PostgreSQL | у управляемого варианта своя таблица (RDS, Aurora, Azure) или пустая (`""`): чужие даты не подставляются |
| RDS или MSK помечены «split-brain: нет арбитра» | признак `managed` → власть `Provider`: провайдер сам держит кворум, изолированный primary останавливается |
| Redis Sentinel / Cluster требует etcd | признак `self-quorum`: голоса считаются внутри юнита |
| Выбор варианта затирает ваши настройки | значения по умолчанию варианта действуют только там, где свойство не задано; заданное вручную остаётся |
| ИИ придумал вариант, которого нет | `unknown_variant` при валидации; в промпте перечислены только существующие `id` |
| Новая версия продукта вышла вчера и ещё сырая | рекомендуется линия, которой больше `matureDays` дней; LTS предпочитается |
| Продукту нечего показывать в таблице версий | `versions: ""` явно отключает проверку; `VER-01` не создаёт шум там, где данных нет |
