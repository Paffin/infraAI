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
| Настройки (группа «Жизненный цикл») | `versions.autoSyncDays` (7) | обновлять таблицы каждые N дней (0 — вручную) |
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

* **SSRF.** Сервер ходит только по адресам, начинающимся с одного из `versions.sourceBases` (совпадают схема, хост и порт; логин и пароль в адресе запрещены; адрес — каталог: `…/api` не пропускает `…/api-evil/`). Администратор может
  отредактировать `source` таблицы, но не заставит сервер обратиться к внутреннему сервису. В закрытой сети в `sourceBases` указывают внутреннее зеркало.
* **Перенаправление проверяется на каждом шаге.** Клиент обновления сам за `Location` не идёт: `versions::fetch` берёт адрес из заголовка, склеивает его с текущим (относительный `Location` тоже) и сверяет с `sourceBases` заново, не более 5 переходов.
  Разрешённый хост, который ответил `302` на внутренний сервис, получает отказ «redirected … outside the allowed addresses», а внутренний сервис — ни одного запроса (раньше клиент по умолчанию шёл за перенаправлением, а проверялся только первый адрес).
* **Плохой ответ не опустошает таблицу.** Нет линий, не JSON, слишком большой ответ (> `outbound.maxAnswerKb`, 4 МБ по умолчанию; тело считается по мере поступления, поэтому ответ без `Content-Length` — chunked, без конца — обрезается тоже), код ≠ 2xx — таблица остаётся прежней, ошибка попадает в итог.
* **Свежее не затирается старым.** Обновлённая таблица становится значением по умолчанию этой установки; при обновлении приложения поставляемая таблица заменяет хранимую,
  только если она **новее** (`asOf`). Вручную изменённая таблица остаётся вашей, а в списке появляется метка «доступна новая версия по умолчанию».
* **Без сети всё работает.** Нет доступа к источнику — используются поставленные таблицы, а `VER-01` по `staleDays` честно предупредит, что данные устарели.

## Что видит пользователь

* **Библиотека** сгруппирована по категориям и функциям («Реляционные БД», «Кэши»…): продукты одной работы лежат рядом, **основные** (признак `core`) — первыми, в подсказке — число вариантов и их названия.
  Категория сворачивается целиком (выбор запоминается; так же сворачиваются «Эталонные архитектуры»), у каждой в заголовке — число продуктов. Плитка занимает ширину панели
  (в узкой панели — одна колонка, в широкой — больше), длинное название переносится на вторую строку: «Jira Service Management» и «Jira Software» не сливаются в «Jira Servi…». Поиск находит продукт и по названию варианта («Aurora» → PostgreSQL): плитка подписана найденным вариантом,
  и клик добавляет узел сразу с ним. Когда в ответ на запрос находятся сотни продуктов, библиотека показывает первые (`editor.librarySearchShown`), остальное открывается кнопкой «Показать ещё».
  Списки в админ-панели ведут себя так же (`editor.adminListShown`), а текст для поиска по записи строится один раз.
* **Инспектор** — раздел «Продукт»: выбор варианта (описание; метка «управляемый сервис», когда провайдер отвечает за переключение) и версии. Версия — список линий выпуска с датами конца поддержки
  (до даты, «только исправления безопасности», «поддержка закончилась»), отметкой LTS и подсказкой «Рекомендуется …» с кнопкой «Выбрать». Если таблица старше `staleDays`, под полем стоит предупреждение.
  Значения по умолчанию варианта подставляются в **пустые** свойства: заданное руками смена варианта не затирает. Смена продукта убирает `variant` и `version` (они значат что-то только для того продукта, для которого выбраны) и сообщает, какие свойства исчезли.
* **Обозначения** (кнопка над холстом) объясняют каждый тип связи каталога — как он нарисован и что значит (`edgeTypes[].description`; у типа без описания говорится, что значит его `semantics`), а также карточки, зоны и то, что рисует отказ: частицы, потери, объезды. Описание типа связи — данные, как и всё остальное: правится в админ-панели.
* **Находки `VER-01` / `VER-02`** выглядят как любые другие: подсвечивают узлы, ведут к полю версии, предлагают исправление «поставить рекомендуемую линию».
* **Мастер размещения** ([PLACEMENT.md](PLACEMENT.md)) предлагает для каждого слоя все продукты его функции и варианты каждого.
* **ИИ** получает каталог в два яруса, чтобы подсказка оставалась подсказкой при любом числе продуктов. **`index`** — все продукты по функциям, одними идентификаторами (первый в функции — «обычный выбор»,
  когда пользователь назвал только работу). **`kinds`** — полное описание (функция, поведение, признаки, свои свойства, строки вариантов `id: Название [default] [+managed]`) для основных продуктов,
  для тех, что уже есть на схеме, и для тех, на которые указывают слова запроса (`llm.catalogExtraKinds`; слово, которое несёт двадцатая часть библиотеки — «cloud», «server», «aws», — ничего не называет;
  чем реже слова, общие с запросом, тем лучше совпадение). Если ответ модели не прошёл проверку, в сообщении о правке к ошибкам прикладывается полное описание продуктов, которыми она воспользовалась,
  не зная их свойств. Модель выбирает продукт и ставит `props.variant`; движок проверяет результат так же, как правку человека.

## Значки

У вида, типа зоны и категории есть необязательное поле `icon` — имя значка из набора интерфейса ([Lucide](https://lucide.dev), в виде `kebab-case`: `database-zap`).
Значок стоит на карточке схемы, в плитке библиотеки, в строке палитры команд (Ctrl+K) и на зоне. Вид по умолчанию носит значок своей **функции**: все реляционные БД —
`database`, все очереди — `messages-square`, все балансировщики L4 — `git-fork`; свой значок виду можно задать отдельно.

Дырки на карточке не бывает: имя, которого интерфейс не знает (опечатка в виде, который вы написали), заменяется значком категории, а он — нейтральной коробкой `box`.
Значок — это векторный компонент интерфейса, а не картинка из базы, поэтому **набор имён ограничен** реестром `frontend/src/lib/icons.ts`: новое имя нужно добавить в него и пересобрать
интерфейс. Список ниже генерируется из реестра, а `gen-docs.py` отказывается работать, если каталог по умолчанию называет имя, которого в нём нет.

<!-- BEGIN GENERATED: icons -->
Интерфейс знает **216** имён, каталог по умолчанию использует **111**.

| Значки | Имена |
|---|---|
| Используются в каталоге по умолчанию | `activity`, `archive-restore`, `arrow-right-left`, `badge-check`, `blocks`, `box`, `boxes`, `brick-wall`, `building-2`, `cable`, `chart-column`, `chart-line`, `cloud`, `cloud-download`, `columns-3`, `cpu`, `database`, `database-zap`, `file-braces`, `fingerprint-pattern`, `git-branch`, `git-fork`, `git-pull-request`, `globe`, `hard-drive`, `hash`, `id-card`, `key-round`, `layers`, `layout-dashboard`, `log-in`, `messages-square`, `network`, `package`, `plug`, `radar`, `route`, `router`, `scale`, `scroll-text`, `search`, `server`, `share-2`, `shield-check`, `signpost`, `split`, `square-function`, `users`, `vote`, `waypoints`, `workflow`, `zap`, `bell`, `brain`, `container`, `file-text`, `inbox`, `mail`, `monitor-cog`, `radio`, `server-cog`, `shield`, `antenna`, `radio-tower`, `ethernet-port`, `shield-alert`, `shield-ban`, `lock-keyhole`, `microchip`, `rocket`, `cloud-cog`, `table-2`, `file-spreadsheet`, `chart-pie`, `folder-sync`, `files`, `file-code`, `bug`, `flask-conical`, `flag`, `book-open`, `clipboard-check`, `refresh-cw`, `siren`, `scan-search`, `key-square`, `user-check`, `bug-off`, `door-closed`, `file-lock`, `shield-half`, `mailbox`, `send`, `phone`, `phone-call`, `video`, `message-circle`, `ticket`, `briefcase`, `shopping-cart`, `graduation-cap`, `newspaper`, `book-text`, `app-window`, `layout-grid`, `headphones`, `calendar`, `brain-circuit`, `sparkles`, `tags`, `notebook-pen` |
| Про запас, для ваших видов | `bot`, `clock`, `cog`, `fingerprint`, `folder`, `gauge`, `hard-drive-download`, `layers-2`, `link`, `lock`, `repeat`, `rss`, `settings-2`, `smartphone`, `terminal`, `wifi`, `satellite-dish`, `signal`, `plug-zap`, `globe-lock`, `funnel`, `merge`, `timer`, `map-pin`, `memory-stick`, `server-crash`, `package-open`, `component`, `puzzle`, `cloud-upload`, `cloud-lightning`, `database-backup`, `table`, `sheet`, `chart-area`, `chart-bar`, `chart-no-axes-combined`, `binary`, `brackets`, `hard-drive-upload`, `archive`, `folder-tree`, `save`, `import`, `git-merge`, `git-compare`, `git-commit-horizontal`, `square-terminal`, `code`, `bug-play`, `test-tube`, `wrench`, `hammer`, `sliders-horizontal`, `toggle-right`, `stamp`, `library`, `notebook`, `list-checks`, `infinity`, `recycle`, `history`, `calendar-clock`, `search-check`, `eye`, `user-lock`, `users-round`, `ban`, `skull`, `biohazard`, `scan-face`, `vault`, `shield-off`, `key`, `building`, `store`, `credit-card`, `wallet`, `landmark`, `factory`, `warehouse`, `truck`, `monitor`, `monitor-play`, `tv`, `laptop`, `tablet`, `printer`, `podcast`, `clapperboard`, `camera`, `contact`, `handshake`, `receipt`, `calculator`, `shopping-bag`, `heart-pulse`, `wand-sparkles`, `microscope`, `atom`, `tag`, `image`, `scan`, `bot-message-square`, `message-square-code` |
<!-- END GENERATED: icons -->

## Как добавить…

**Продукт** — Администрирование → Каталог → «Новый элемент» (или «Копия» похожего): `function`, `behavior`, `traits`, `roles`, `props`, `capacity`, `cost`, `icon`. Если функция новая, объявите её в категории.
Продукт без таблицы выпусков допустим: `versions: ""` — срок поддержки не проверяется.

**Вариант** — в `variants` вида: `id`, `name`, `description`, при необходимости `behavior`, `addTraits`, `defaults`, `versions`. Один вариант — `default: true`.

**Таблицу версий** — Каталог → Версии → «Новый элемент»: `id` (равный `versions` вида или варианта), `source` (адрес в `versions.sourceBases` или `manual`) и линии. Для «ручной» таблицы
срок поддержки задаёте вы; для таблицы с источником — кнопка «Обновить».

**Встроенное значение по умолчанию** (чтобы продукт был в чистой установке) — `deploy/catalog/`: каталог собирается из исходников на Python, так тысяча продуктов остаётся согласованной. Первая,
**основная** библиотека (105 продуктов, признак `core`) написана вручную в `part_*.py`; остальное добавлено модулями `kinds_<область>.py` через заготовки (`svc`, `app`, `saas`, `sqldb`, `qcluster`…, `forge.py`).
Правило: правите исходник → `python3 deploy/catalog/lint.py` (тот же набор проверок: повторы идентификаторов и названий, описание на двух языках, свойства, которых требуют правила, таблицы выпусков, значки)
→ `python3 deploy/catalog/build.py` → `--check` в CI убеждается, что `catalog.json` и названия таблиц актуальны. Значок вида берётся из его функции (`FUNCTION_ICON` в `build.py`) или задаётся `icon=` в описании вида;
без значка сборка не проходит. Подробности и пример модуля — [README каталога](../deploy/catalog/README.md).

**Сервис, который ведёт провайдер** (`saas(...)`, признаки `managed` + `provider-ha`): экземпляров считать нечего. Правила про «два экземпляра» и «нет единой точки отказа» его не трогают, ёмкость за него
отвечает провайдер (узел, у которого указан `rpsPerReplica`, проверяется как обычный), а доступность считается по SLA провайдера для функции (`reliability.provider.byFunction`, по умолчанию 99,9 %).
Признаки, означающие «провайдер сам решает», — настройка `assumptions.elasticTraits`. Вариант `managed` у продукта со свойством `replicas` под это не подпадает: число экземпляров там выбирает архитектор.

## Каталог по умолчанию

Таблица ниже генерируется из `catalog.json` и `versions.json` (`python3 deploy/gen-docs.py`). ★ — вариант по умолчанию.

<!-- BEGIN GENERATED: catalog -->
Видов: **956**, вариантов: **578**, функций: **100**. «Версии» — таблица выпусков, по которой проверяется срок поддержки (у варианта она может быть своей; прочерк — таблицы нет).

### Сеть и вход

**Пользователи и клиенты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `users` — Пользователи | `stateless` | — | — |

**Глобальная балансировка (GSLB)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `gslb` — GSLB / DNS | `stateless` | Свои DNS-серверы (PowerDNS / BIND + проверки) ★, Amazon Route 53, Cloudflare Load Balancing, Другой облачный DNS / Traffic Manager | — |
| `ns1` — IBM NS1 Connect | `stateless` | — | — |
| `azure_traffic_manager` — Azure Traffic Manager | `stateless` | — | — |
| `akamai_gtm` — Akamai Global Traffic Management | `stateless` | — | — |
| `f5_dns` — F5 BIG-IP DNS | `stateless` | — | BIG-IP |
| `aws_global_accelerator` — AWS Global Accelerator | `stateless` | — | — |

**Серверы DNS**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `dns_server` — DNS-сервер | `stateless` | BIND 9 ★, PowerDNS, CoreDNS, Облачная зона (Route 53, Cloud DNS…) | BIND 9 |
| `unbound` — Unbound | `stateless` | — | — |
| `knot_resolver` — Knot Resolver | `stateless` | — | — |
| `powerdns_recursor` — PowerDNS Recursor | `stateless` | — | — |
| `knot_dns` — Knot DNS | `stateless` | — | — |
| `nsd` — NSD | `stateless` | — | — |
| `dnsmasq` — dnsmasq | `stateless` | — | — |
| `dnsdist` — dnsdist | `stateless` | — | — |
| `infoblox_nios` — Infoblox NIOS | `stateless` | — | — |
| `technitium_dns` — Technitium DNS Server | `stateless` | — | — |
| `microsoft_dns` — Microsoft DNS Server | `stateless` | — | Windows Server |

**CDN**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cdn` — CDN | `stateless` | Cloudflare ★, Amazon CloudFront, Akamai, Fastly, Региональная CDN (Gcore, Yandex Cloud CDN, Selectel…), Свои кэширующие узлы (Varnish / NGINX) | — |
| `azure_front_door` — Azure Front Door | `stateless` | — | — |
| `google_cloud_cdn` — Google Cloud CDN | `stateless` | — | — |
| `gcore_cdn` — Gcore CDN | `stateless` | — | — |
| `cdnvideo` — CDNvideo | `stateless` | — | — |
| `yandex_cdn` — Yandex Cloud CDN | `stateless` | — | — |
| `selectel_cdn` — Selectel CDN | `stateless` | — | — |
| `bunny_cdn` — bunny.net CDN | `stateless` | — | — |
| `keycdn` — KeyCDN | `stateless` | — | — |
| `cachefly` — CacheFly | `stateless` | — | — |

**HTTP-кэш**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `varnish` — Varnish Cache | `stateless` | — | Varnish Cache |
| `apache_traffic_server` — Apache Traffic Server | `stateless` | — | — |

**WAF**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `waf` — WAF | `stateless` | Облачный WAF (Cloudflare, AWS WAF, Imperva…) ★, ModSecurity / Coraza + OWASP CRS, NGINX App Protect, Аппаратный / виртуальный (F5, FortiWeb, Imperva) | — |
| `coraza` — OWASP Coraza | `stateless` | — | — |
| `aws_waf` — AWS WAF | `stateless` | — | — |
| `azure_waf` — Azure Web Application Firewall | `stateless` | — | — |
| `imperva_waf` — Imperva Cloud WAF | `stateless` | — | — |
| `f5_advanced_waf` — F5 BIG-IP Advanced WAF | `stateless` | — | BIG-IP |
| `akamai_app_api_protector` — Akamai App & API Protector | `stateless` | — | — |
| `ptaf` — PT Application Firewall | `stateless` | — | — |
| `wallarm` — Wallarm | `stateless` | — | — |
| `fortiweb` — Fortinet FortiWeb | `stateless` | Устройство или виртуальная машина (HA) ★, FortiWeb Cloud | — |
| `barracuda_waf` — Barracuda Web Application Firewall | `stateless` | Устройство или виртуальная машина (кластер) ★, Barracuda WAF-as-a-Service | — |
| `nemesida_waf` — Nemesida WAF | `stateless` | — | — |
| `qrator_waf` — Qrator WAF | `stateless` | — | — |
| `google_cloud_armor` — Google Cloud Armor | `stateless` | — | — |
| `yandex_smart_web_security` — Yandex Smart Web Security | `stateless` | — | — |

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
| `f5_bigip_ltm` — F5 BIG-IP LTM | `stateless` | — | BIG-IP |
| `netscaler` — NetScaler ADC | `stateless` | Пара HA (primary / secondary) ★, Кластер (до 32 узлов) | — |
| `a10_thunder_adc` — A10 Thunder ADC | `stateless` | — | — |
| `kemp_loadmaster` — Progress Kemp LoadMaster | `stateless` | — | — |
| `radware_alteon` — Radware Alteon | `stateless` | — | — |
| `fortiadc` — Fortinet FortiADC | `stateless` | — | — |
| `avi_alb` — VMware Avi Load Balancer | `stateless` | — | — |
| `aws_alb` — Amazon Application Load Balancer | `stateless` | — | — |
| `gcp_https_lb` — Google Cloud Application Load Balancer | `stateless` | — | — |
| `azure_application_gateway` — Azure Application Gateway | `stateless` | — | — |
| `yandex_alb` — Yandex Application Load Balancer | `stateless` | — | — |
| `openresty` — OpenResty | `stateless` | — | — |
| `ingress_nginx` — ingress-nginx | `stateless` | — | — |
| `haproxy_ingress` — HAProxy Ingress | `stateless` | — | HAProxy Ingress |
| `contour` — Contour | `stateless` | — | Contour |
| `kgateway` — kgateway | `stateless` | — | — |

**Балансировщики L4**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `l4` — LB / VRRP | `stateless` | VRRP + LVS / HAProxy ★, Anycast + BGP (ECMP), Облачный L4 (NLB, Load Balancer) | — |
| `metallb` — MetalLB | `stateless` | Режим L2 (ARP / NDP) ★, Режим BGP (ECMP) | MetalLB |
| `lvs_keepalived` — LVS (IPVS) + keepalived | `stateless` | — | — |
| `katran` — Katran | `stateless` | — | — |
| `kube_vip` — kube-vip | `stateless` | — | — |
| `cilium_lb` — Cilium (балансировка L4) | `stateless` | — | Cilium |
| `aws_nlb` — Amazon Network Load Balancer | `stateless` | — | — |
| `aws_gateway_lb` — AWS Gateway Load Balancer | `stateless` | — | — |
| `gcp_network_lb` — Google Cloud Network Load Balancer | `stateless` | — | — |
| `azure_load_balancer` — Azure Load Balancer | `stateless` | — | — |
| `yandex_nlb` — Yandex Network Load Balancer | `stateless` | — | — |
| `openstack_octavia` — OpenStack Octavia | `stateless` | — | — |

**Прокси**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `squid` — Squid | `stateless` | — | Squid |
| `zscaler_internet_access` — Zscaler Internet Access | `stateless` | — | — |
| `symantec_proxysg` — Symantec ProxySG (Edge SWG) | `stateless` | — | — |
| `netskope` — Netskope One SWG | `stateless` | — | — |
| `cisco_secure_access` — Cisco Secure Access | `stateless` | — | — |

**Межсетевые экраны**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `firewall` — Межсетевой экран | `stateless` | OPNsense ★, Fortinet FortiGate, Palo Alto Networks (PAN-OS), pfSense, Cisco Secure Firewall (ASA / FTD), Облачный межсетевой экран (AWS Network Firewall, Azure Firewall) | OPNsense |
| `check_point` — Check Point Quantum | `stateless` | — | — |
| `juniper_srx` — Juniper SRX | `stateless` | Chassis cluster ★, Multinode High Availability (MNHA) | — |
| `sophos_firewall` — Sophos Firewall | `stateless` | — | — |
| `watchguard` — WatchGuard Firebox | `stateless` | — | — |
| `sonicwall` — SonicWall | `stateless` | — | — |
| `huawei_usg` — Huawei USG (HiSecEngine) | `stateless` | — | — |
| `vyos` — VyOS | `stateless` | — | — |
| `usergate` — UserGate NGFW | `stateless` | — | — |
| `ideco_ngfw` — Ideco NGFW | `stateless` | — | — |
| `continent_apc` — Континент 4 | `stateless` | — | — |
| `pt_ngfw` — PT NGFW | `stateless` | — | — |
| `aws_network_firewall` — AWS Network Firewall | `stateless` | — | — |
| `azure_firewall` — Azure Firewall | `stateless` | — | — |
| `gcp_cloud_firewall` — Google Cloud NGFW | `stateless` | — | — |

**Сетевое оборудование**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `network_device` — Маршрутизатор / коммутатор | `stateless` | Cisco IOS XE ★, Arista EOS, Juniper Junos, SONiC / Cumulus (open networking) | Cisco IOS XE |
| `mikrotik` — MikroTik RouterOS | `stateless` | — | RouterOS |
| `ubiquiti_unifi` — Ubiquiti UniFi | `stateless` | — | — |
| `huawei_cloudengine` — Huawei CloudEngine | `stateless` | — | — |
| `hpe_aruba_cx` — HPE Aruba Networking CX | `stateless` | — | — |
| `dell_powerswitch` — Dell PowerSwitch (OS10) | `stateless` | — | — |
| `nvidia_spectrum` — NVIDIA Spectrum (Cumulus Linux) | `stateless` | — | — |
| `extreme_networks` — Extreme Networks | `stateless` | — | — |
| `eltex` — Eltex | `stateless` | — | — |
| `cisco_nexus` — Cisco Nexus (NX-OS) | `stateless` | — | — |
| `cisco_aci` — Cisco ACI | `stateless` | — | — |
| `vmware_nsx` — VMware NSX | `stateless` | — | — |
| `aws_transit_gateway` — AWS Transit Gateway | `stateless` | — | — |
| `frrouting` — FRRouting | `stateless` | — | — |
| `bird` — BIRD | `stateless` | — | — |
| `open_vswitch` — Open vSwitch | `stateless` | — | — |
| `openstack_neutron` — OpenStack Neutron | `stateless` | — | — |

**VPN и каналы между площадками**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vpn_dci` — VPN / DCI | `stateless` | IPsec site-to-site ★, WireGuard, OpenVPN, Облачный VPN (AWS Site-to-Site, Cloud VPN, VPN Gateway), Выделенный канал (Direct Connect, ExpressRoute, оптика, L2), MPLS / SD-WAN | — |
| `openvpn_access_server` — OpenVPN Access Server | `stateless` | Failover (active/standby) ★, Cluster (active/active) | — |
| `strongswan` — strongSwan | `stateless` | — | — |
| `tailscale` — Tailscale | `stateless` | — | — |
| `netbird` — NetBird | `stateless` | — | — |
| `zerotier` — ZeroTier | `stateless` | — | — |
| `headscale` — Headscale | `stateless` | — | — |
| `nebula` — Nebula | `stateless` | — | — |
| `cisco_anyconnect` — Cisco Secure Client (AnyConnect) VPN | `stateless` | — | — |
| `globalprotect` — Palo Alto GlobalProtect | `stateless` | — | PAN-OS (Palo Alto Networks) |
| `ivanti_connect_secure` — Ivanti Connect Secure | `stateless` | — | — |
| `aws_site_to_site_vpn` — AWS Site-to-Site VPN | `stateless` | — | — |
| `azure_vpn_gateway` — Azure VPN Gateway | `stateless` | — | — |
| `gcp_cloud_vpn` — Google Cloud HA VPN | `stateless` | — | — |
| `aws_direct_connect` — AWS Direct Connect | `stateless` | — | — |
| `azure_expressroute` — Azure ExpressRoute | `stateless` | — | — |
| `gcp_cloud_interconnect` — Google Cloud Interconnect | `stateless` | — | — |
| `cisco_meraki_sdwan` — Cisco Meraki MX (Auto VPN / SD-WAN) | `stateless` | — | — |
| `versa_sdwan` — Versa SD-WAN | `stateless` | — | — |
| `velocloud_sdwan` — Arista VeloCloud SD-WAN | `stateless` | — | — |
| `cisco_catalyst_sdwan` — Cisco Catalyst SD-WAN | `stateless` | — | — |
| `vipnet_coordinator` — ViPNet Coordinator | `stateless` | — | — |
| `s_terra_gateway` — С-Терра Шлюз | `stateless` | — | — |

**Защита от DDoS**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `qrator` — Qrator Anti-DDoS | `stateless` | — | — |
| `stormwall` — StormWall | `stateless` | — | — |
| `kaspersky_ddos_protection` — Kaspersky DDoS Protection | `stateless` | — | — |
| `radware_defensepro` — Radware DefensePro | `stateless` | — | — |
| `netscout_arbor` — NETSCOUT Arbor Edge Defense | `stateless` | — | — |
| `akamai_prolexic` — Akamai Prolexic | `stateless` | — | — |
| `cloudflare_magic_transit` — Cloudflare Magic Transit | `stateless` | — | — |
| `imperva_ddos` — Imperva DDoS Protection | `stateless` | — | — |
| `gcore_ddos` — Gcore DDoS Protection | `stateless` | — | — |
| `aws_shield` — AWS Shield | `stateless` | — | — |
| `azure_ddos_protection` — Azure DDoS Protection | `stateless` | — | — |
| `yandex_ddos_protection` — Yandex DDoS Protection | `stateless` | — | — |

**Сетевые службы (NTP, DHCP, IPAM)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `chrony` — chrony | `stateless` | — | — |
| `ntpd` — NTP (ntpd) | `stateless` | — | — |
| `meinberg_lantime` — Meinberg LANTIME | `stateless` | — | — |
| `isc_kea` — ISC Kea | `stateless` | — | — |
| `isc_dhcp` — ISC DHCP (dhcpd) | `stateless` | — | — |
| `microsoft_dhcp` — Microsoft DHCP Server | `stateless` | — | Windows Server |
| `netbox` — NetBox | `stateless` | Свой NetBox ★, NetBox Cloud | — |
| `phpipam` — phpIPAM | `stateless` | — | — |
| `nautobot` — Nautobot | `stateless` | Свой Nautobot ★, Nautobot Cloud | — |

### Вычисления

**Kubernetes**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kubernetes` — Kubernetes | `stateless` | Свой кластер (kubeadm / RKE2 / k0s) ★, Red Hat OpenShift, Rancher (RKE2), Amazon EKS, Google GKE, Azure AKS | Kubernetes |
| `k3s` — K3s | `stateless` | — | Kubernetes |
| `rke2` — RKE2 | `stateless` | — | Kubernetes |
| `okd` — OKD | `stateless` | — | — |
| `deckhouse` — Deckhouse Kubernetes Platform | `stateless` | — | — |
| `talos_linux` — Talos Linux | `stateless` | — | — |
| `microk8s` — MicroK8s | `stateless` | — | Kubernetes |
| `charmed_kubernetes` — Charmed Kubernetes | `stateless` | — | Kubernetes |
| `vmware_vks` — VMware vSphere Kubernetes Service | `stateless` | — | — |
| `k0s` — k0s | `stateless` | — | Kubernetes |
| `kubermatic` — Kubermatic Kubernetes Platform | `stateless` | — | — |
| `gardener` — Gardener | `stateless` | — | — |
| `vcluster` — vCluster | `stateless` | — | — |
| `yandex_managed_kubernetes` — Yandex Managed Service for Kubernetes | `stateless` | — | — |
| `vk_cloud_kubernetes` — VK Cloud Kubernetes | `stateless` | — | — |
| `selectel_managed_kubernetes` — Selectel Managed Kubernetes | `stateless` | — | — |
| `cloud_ru_evolution_kubernetes` — Cloud.ru Evolution Managed Kubernetes | `stateless` | — | — |
| `oracle_oke` — Oracle Container Engine for Kubernetes (OKE) | `stateless` | — | — |
| `digitalocean_doks` — DigitalOcean Kubernetes (DOKS) | `stateless` | — | — |
| `linode_lke` — Linode Kubernetes Engine (LKE) | `stateless` | — | — |
| `aws_rosa` — Red Hat OpenShift Service on AWS (ROSA) | `stateless` | — | — |
| `azure_aro` — Azure Red Hat OpenShift (ARO) | `stateless` | — | — |

**Оркестраторы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `nomad` — HashiCorp Nomad | `stateless` | — | HashiCorp Nomad |
| `docker_swarm` — Docker Swarm | `stateless` | — | Docker Engine |
| `cloud_foundry` — Cloud Foundry | `stateless` | — | — |
| `amazon_ecs` — Amazon ECS | `stateless` | — | — |

**Виртуальные машины**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vm_pool` — Пул ВМ | `stateless` | VMware vSphere (ESXi) ★, Proxmox VE, XCP-ng, oVirt / RHV, Nutanix AHV, KVM / libvirt, Облачные виртуальные машины (EC2, Compute Engine, Yandex Compute…) | VMware ESXi |

**Функции (serverless)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `functions` — Functions | `stateless` | Облачные функции (Lambda, Cloud Functions, Yandex Cloud Functions) ★, Knative, OpenFaaS | — |
| `aws_lambda` — AWS Lambda | `stateless` | — | — |
| `azure_functions` — Azure Functions | `stateless` | — | — |
| `google_cloud_functions` — Google Cloud Functions | `stateless` | — | — |
| `yandex_cloud_functions` — Yandex Cloud Functions | `stateless` | — | — |
| `cloudflare_workers` — Cloudflare Workers | `stateless` | — | — |
| `fission` — Fission | `stateless` | — | — |
| `nuclio` — Nuclio | `stateless` | — | — |

**API-шлюзы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `api_gateway` — API Gateway | `stateless` | Свой шлюз (Envoy, Tyk, KrakenD…) ★, Облачный шлюз (API Gateway, Azure APIM, Apigee) | — |
| `kong` — Kong Gateway | `stateless` | DB-less (конфигурация из Git) ★, С базой данных PostgreSQL, Hybrid (control plane + data plane) | Kong Gateway |
| `apisix` — Apache APISIX | `stateless` | Конфигурация в etcd ★, Автономный режим (файл) | Apache APISIX |
| `tyk` — Tyk | `stateless` | Свой шлюз (с Redis) ★, Tyk Cloud | — |
| `krakend` — KrakenD | `stateless` | — | — |
| `gravitee` — Gravitee API Management | `stateless` | — | — |
| `wso2_api_manager` — WSO2 API Manager | `stateless` | — | — |
| `apigee` — Apigee | `stateless` | Apigee X (управляемый) ★, Apigee hybrid (среда выполнения в вашем Kubernetes) | — |
| `aws_api_gateway` — Amazon API Gateway | `stateless` | — | — |
| `azure_api_management` — Azure API Management | `stateless` | — | — |
| `mulesoft_anypoint` — MuleSoft Anypoint (Mule runtime) | `stateless` | Свои серверы или Runtime Fabric ★, CloudHub (облако MuleSoft) | Mule Runtime |
| `spring_cloud_gateway` — Spring Cloud Gateway | `stateless` | — | — |
| `yandex_api_gateway` — Yandex API Gateway | `stateless` | — | — |
| `ocelot` — Ocelot | `stateless` | — | — |

**Обработка данных**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `airflow` — Apache Airflow | `stateless` | — | Apache Airflow |
| `spark` — Apache Spark | `stateless` | — | Apache Spark |
| `flink` — Apache Flink | `stateless` | — | Apache Flink |
| `nifi` — Apache NiFi | `stateless` | — | Apache NiFi |
| `apache_hadoop` — Apache Hadoop | `stateless` | — | Apache Hadoop |
| `apache_hive` — Apache Hive | `stateless` | — | — |
| `trino` — Trino | `stateless` | — | — |
| `presto` — Presto | `stateless` | — | — |
| `dremio` — Dremio | `stateless` | — | — |
| `apache_storm` — Apache Storm | `stateless` | — | — |
| `dask` — Dask | `stateless` | — | — |
| `ray` — Ray | `stateless` | — | — |
| `databricks` — Databricks | `stateless` | — | — |
| `aws_emr` — Amazon EMR | `stateless` | — | — |
| `google_cloud_dataproc` — Google Cloud Dataproc | `stateless` | — | — |
| `cloudera_cdp` — Cloudera Data Platform (CDP) | `stateless` | — | — |
| `arenadata_hyperwave` — Arenadata Hyperwave (ADH) | `stateless` | — | — |
| `aws_glue` — AWS Glue | `stateless` | — | Amazon Glue |
| `aws_athena` — Amazon Athena | `stateless` | — | — |
| `google_cloud_dataflow` — Google Cloud Dataflow | `stateless` | — | — |
| `azure_data_factory` — Azure Data Factory | `stateless` | — | — |

**Платформы виртуализации**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vmware_vsphere` — VMware vSphere | `stateless` | — | VMware ESXi |
| `vmware_cloud_foundation` — VMware Cloud Foundation | `stateless` | — | VMware Cloud Foundation |
| `proxmox_ve` — Proxmox VE | `stateless` | — | Proxmox VE |
| `kvm_libvirt` — KVM / libvirt | `stateless` | — | — |
| `microsoft_hyperv` — Microsoft Hyper-V | `stateless` | — | Windows Server |
| `nutanix_ahv` — Nutanix AHV | `stateless` | — | Nutanix AOS |
| `openstack` — OpenStack | `stateless` | — | — |
| `apache_cloudstack` — Apache CloudStack | `stateless` | — | — |
| `opennebula` — OpenNebula | `stateless` | — | — |
| `zvirt` — zVirt | `stateless` | — | — |
| `harvester` — Harvester | `stateless` | — | — |
| `kubevirt` — KubeVirt | `stateless` | — | — |
| `firecracker` — Firecracker | `stateless` | — | — |
| `lxd_incus` — LXD / Incus | `stateless` | — | — |
| `xcp_ng` — XCP-ng | `stateless` | — | XCP-ng |
| `ovirt` — oVirt | `stateless` | — | oVirt |

**Облачные виртуальные машины (IaaS)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `aws_ec2` — Amazon EC2 | `stateless` | — | — |
| `azure_virtual_machines` — Azure Virtual Machines | `stateless` | — | — |
| `google_compute_engine` — Google Compute Engine | `stateless` | — | — |
| `yandex_compute_cloud` — Yandex Compute Cloud | `stateless` | — | — |
| `vk_cloud_servers` — VK Cloud Servers | `stateless` | — | — |
| `selectel_cloud_servers` — Selectel Cloud Servers | `stateless` | — | — |
| `oracle_oci_compute` — Oracle Cloud Infrastructure Compute | `stateless` | — | — |

**Среды выполнения контейнеров**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `docker_engine` — Docker Engine | `stateless` | — | Docker Engine |
| `containerd` — containerd | `stateless` | — | — |
| `podman` — Podman | `stateless` | — | Podman |
| `cri_o` — CRI-O | `stateless` | — | Kubernetes |
| `kata_containers` — Kata Containers | `stateless` | — | — |
| `gvisor` — gVisor | `stateless` | — | — |

**Платформы приложений (PaaS)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `heroku` — Heroku | `stateless` | — | — |
| `google_cloud_run` — Google Cloud Run | `stateless` | — | — |
| `google_app_engine` — Google App Engine | `stateless` | — | — |
| `aws_elastic_beanstalk` — AWS Elastic Beanstalk | `stateless` | Окружение с балансировщиком и Auto Scaling ★, Одиночный экземпляр | — |
| `azure_app_service` — Azure App Service | `stateless` | — | — |
| `azure_container_apps` — Azure Container Apps | `stateless` | — | — |
| `dokku` — Dokku | `stateless` | — | — |
| `caprover` — CapRover | `stateless` | — | — |
| `coolify` — Coolify | `stateless` | — | — |
| `fly_io` — Fly.io | `stateless` | — | — |
| `vercel` — Vercel | `stateless` | — | — |

**Серверы приложений**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `tomcat` — Apache Tomcat | `stateless` | — | Apache Tomcat |
| `eclipse_jetty` — Eclipse Jetty | `stateless` | — | Eclipse Jetty |
| `wildfly` — WildFly | `stateless` | — | — |
| `jboss_eap` — Red Hat JBoss EAP | `stateless` | — | Red Hat JBoss Enterprise Application Platform |
| `oracle_weblogic` — Oracle WebLogic Server | `stateless` | — | — |
| `ibm_websphere` — IBM WebSphere Application Server | `stateless` | — | — |
| `payara_server` — Payara Server | `stateless` | — | — |
| `eclipse_glassfish` — Eclipse GlassFish | `stateless` | — | — |
| `microsoft_iis` — Microsoft IIS | `stateless` | — | Windows Server |
| `php_fpm` — PHP-FPM | `stateless` | — | PHP |
| `gunicorn` — Gunicorn | `stateless` | — | — |

**Оркестрация процессов и заданий**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `temporal` — Temporal | `stateless` | Свой кластер ★, Temporal Cloud | — |
| `uber_cadence` — Cadence | `stateless` | — | — |
| `zeebe` — Camunda 8 (Zeebe) | `quorum_log` | Свой кластер ★, Camunda 8 SaaS | — |
| `camunda_7` — Camunda 7 | `stateless` | — | — |
| `conductor_oss` — Conductor OSS (Netflix Conductor) | `stateless` | — | — |
| `argo_workflows` — Argo Workflows | `stateless` | — | Argo Workflows |
| `prefect` — Prefect | `stateless` | Свой сервер ★, Prefect Cloud | — |
| `dagster` — Dagster | `stateless` | — | — |
| `luigi` — Luigi | `stateless` | — | — |
| `n8n` — n8n | `stateless` | — | — |
| `apache_dolphinscheduler` — Apache DolphinScheduler | `stateless` | — | — |
| `control_m` — Control-M | `stateless` | — | Control-M |
| `kestra` — Kestra | `stateless` | — | — |
| `aws_step_functions` — AWS Step Functions | `stateless` | — | — |
| `aws_mwaa` — Amazon MWAA | `stateless` | — | — |
| `google_cloud_composer` — Google Cloud Composer | `stateless` | — | — |

**Пакетные и HPC-планировщики**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `slurm` — Slurm | `stateless` | — | — |
| `pbs_professional` — Altair PBS Professional | `stateless` | — | — |
| `htcondor` — HTCondor | `stateless` | — | — |
| `ibm_spectrum_lsf` — IBM Spectrum LSF | `stateless` | — | — |
| `volcano` — Volcano | `stateless` | — | — |
| `kueue` — Kueue | `stateless` | — | — |
| `aws_batch` — AWS Batch | `stateless` | — | — |
| `azure_batch` — Azure Batch | `stateless` | — | — |
| `google_cloud_batch` — Google Cloud Batch | `stateless` | — | — |

### Данные

**Реляционные СУБД**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `postgresql` — PostgreSQL | `primary_replica` | Patroni + etcd/Consul ★, repmgr + witness, Потоковая репликация, ручное переключение, CloudNativePG в Kubernetes, Amazon RDS Multi-AZ, Amazon Aurora PostgreSQL, Azure Database for PostgreSQL, Другой облачный PostgreSQL | PostgreSQL |
| `mysql` — MySQL | `primary_replica` | InnoDB Cluster (Group Replication + MySQL Router) ★, Асинхронная репликация + Orchestrator, Полусинхронная репликация, ручное переключение, Percona XtraDB Cluster (Galera), Amazon RDS for MySQL (Multi-AZ), Amazon Aurora MySQL, Azure Database for MySQL, Другой облачный MySQL | MySQL |
| `mariadb` — MariaDB | `primary_replica` | MariaDB Galera Cluster ★, Репликация + MaxScale, Полусинхронная репликация, ручное переключение, Amazon RDS for MariaDB (Multi-AZ), Другой облачный MariaDB | MariaDB |
| `mssql` — Microsoft SQL Server | `primary_replica` | Always On группы доступности (WSFC) ★, Кластер экземпляров (общее хранилище), Доставка журналов, ручное переключение, Облачный SQL Server (Azure SQL MI, Amazon RDS) | Microsoft SQL Server |
| `oracle` — Oracle Database | `primary_replica` | Data Guard + Fast-Start Failover ★, Data Guard, ручное переключение, Real Application Clusters (RAC), Облачный Oracle (OCI, Amazon RDS for Oracle) | Oracle Database |
| `ibm_db2` — IBM Db2 | `primary_replica` | HADR + Pacemaker (кластерные службы Db2) ★, HADR, ручное переключение, pureScale (общее хранилище), Облачный Db2 (Amazon RDS for Db2 и др.) | IBM Db2 |
| `sap_hana` — SAP HANA | `primary_replica` | System Replication + кластер Pacemaker ★, System Replication, ручной takeover, Scale-out с резервными хостами, SAP HANA Cloud | — |
| `firebird` — Firebird | `primary_replica` | — | — |
| `informix` — HCL Informix | `primary_replica` | HDR / RSS / SDS + Connection Manager ★, Ручное переключение | — |
| `sap_ase` — SAP ASE | `primary_replica` | Always-On (HADR) + Fault Manager ★, Cluster Edition (общий диск), HADR, ручное переключение | — |
| `postgres_pro` — Postgres Pro | `primary_replica` | Patroni + etcd/Consul ★, BiHA (встроенная высокая доступность), Потоковая репликация, ручное переключение | — |
| `pangolin` — Platform V Pangolin | `primary_replica` | Patroni + etcd ★, Потоковая репликация, ручное переключение | — |
| `tantor` — Tantor Postgres | `primary_replica` | Patroni + etcd ★, Потоковая репликация, ручное переключение | — |
| `jatoba` — Jatoba | `primary_replica` | Отказоустойчивый кластер ★, Потоковая репликация, ручное переключение | — |
| `edb_postgres` — EDB Postgres Advanced Server | `primary_replica` | Failover Manager (EFM) + witness ★, EDB Postgres Distributed (Raft), Потоковая репликация, ручное переключение, EDB Postgres AI Cloud Service (ранее BigAnimal) | — |
| `citus` — Citus | `primary_replica` | Patroni + etcd/Consul на каждый узел ★, Потоковая репликация, ручное переключение | — |
| `alibaba_polardb` — Alibaba Cloud PolarDB | `stateless` | — | — |
| `azure_sql_database` — Azure SQL Database | `stateless` | — | — |
| `google_cloud_sql` — Google Cloud SQL | `stateless` | — | — |
| `alloydb` — AlloyDB for PostgreSQL | `stateless` | — | — |

**Распределённые SQL**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cockroachdb` — CockroachDB | `quorum_log` | Свой кластер ★, CockroachDB Cloud | CockroachDB |
| `yugabytedb` — YugabyteDB | `quorum_log` | Свой кластер ★, YugabyteDB Aeon | — |
| `tidb` — TiDB | `quorum_log` | Свой кластер (TiUP / оператор) ★, TiDB Cloud | — |
| `vitess` — Vitess | `primary_replica` | — | Vitess |
| `google_spanner` — Google Cloud Spanner | `stateless` | — | — |
| `amazon_aurora_dsql` — Amazon Aurora DSQL | `stateless` | — | — |
| `oceanbase` — OceanBase | `quorum_log` | Свой кластер (OBD / ob-operator) ★, OceanBase Cloud | — |
| `ydb` — YDB | `quorum_log` | mirror-3-dc (три дата-центра) ★, block-4-2 (один дата-центр), Yandex Managed Service for YDB | — |
| `polardb_x` — PolarDB-X | `quorum_log` | Свой кластер ★, PolarDB-X в Alibaba Cloud | — |

**Документные базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `mongodb` — MongoDB | `primary_replica` | Реплика-сет ★, Шардированный кластер, MongoDB Atlas, Amazon DocumentDB, Другой облачный MongoDB | MongoDB |
| `couchbase` — Couchbase | `quorum_log` | — | Couchbase Server |
| `couchdb` — Apache CouchDB | `quorum_log` | — | Apache CouchDB |
| `arangodb` — ArangoDB | `quorum_log` | — | ArangoDB |
| `azure_cosmos_db` — Azure Cosmos DB | `stateless` | — | — |
| `ferretdb` — FerretDB | `stateless` | — | — |
| `marklogic` — MarkLogic Server | `quorum_log` | — | — |
| `google_firestore` — Google Cloud Firestore | `stateless` | — | — |

**Графовые базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `neo4j` — Neo4j | `quorum_log` | — | Neo4j |
| `janusgraph` — JanusGraph | `stateless` | — | — |
| `dgraph` — Dgraph | `quorum_log` | — | — |
| `nebulagraph` — NebulaGraph | `quorum_log` | — | — |
| `memgraph` — Memgraph | `primary_replica` | HA-кластер (Enterprise): координаторы Raft ★, Репликация MAIN/REPLICA, ручное переключение | — |
| `amazon_neptune` — Amazon Neptune | `stateless` | — | Amazon Neptune |

**Колоночные базы (wide-column)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cassandra` — Apache Cassandra | `quorum_log` | Свой кластер ★, K8ssandra (оператор в Kubernetes), Облачный Cassandra (Amazon Keyspaces, Astra DB) | Apache Cassandra |
| `scylladb` — ScyllaDB | `quorum_log` | Свой кластер ★, ScyllaDB Cloud | — |
| `hbase` — Apache HBase | `quorum_log` | — | Apache HBase |
| `google_bigtable` — Google Cloud Bigtable | `stateless` | — | — |
| `apache_accumulo` — Apache Accumulo | `quorum_log` | — | — |

**Ключ-значение**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `foundationdb` — FoundationDB | `quorum_log` | — | FoundationDB |
| `tarantool` — Tarantool | `primary_replica` | Встроенный Raft (election_mode) ★, Master-replica, ручное переключение | Tarantool |
| `amazon_dynamodb` — Amazon DynamoDB | `stateless` | — | — |
| `aerospike` — Aerospike | `quorum_log` | — | — |
| `hazelcast` — Hazelcast | `quorum_log` | — | — |
| `apache_ignite` — Apache Ignite | `quorum_log` | — | — |
| `gridgain` — GridGain | `quorum_log` | — | — |
| `infinispan` — Infinispan | `quorum_log` | — | — |
| `oracle_coherence` — Oracle Coherence | `quorum_log` | — | — |
| `tikv` — TiKV | `quorum_log` | — | — |

**Временные ряды**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `influxdb` — InfluxDB | `primary_replica` | Core (один узел + копии) ★, Enterprise (кластер) | InfluxDB |
| `timescaledb` — TimescaleDB | `primary_replica` | Patroni + etcd/Consul ★, Потоковая репликация, ручное переключение, Tiger Cloud | — |
| `grafana_mimir` — Grafana Mimir | `quorum_log` | Свой кластер ★, Grafana Cloud Metrics | — |
| `thanos` — Thanos | `stateless` | — | — |
| `cortex` — Cortex | `quorum_log` | — | — |
| `questdb` — QuestDB | `primary_replica` | Открытая версия (один узел) ★, Enterprise (репликация через объектное хранилище) | — |
| `cratedb` — CrateDB | `quorum_log` | Свой кластер ★, CrateDB Cloud | — |
| `tdengine` — TDengine | `quorum_log` | Свой кластер ★, TDengine Cloud | — |

**Кэши**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `redis` — Redis | `primary_replica` | Один узел ★, Sentinel (master + реплики), Redis Cluster (шарды), Amazon ElastiCache, Другой облачный Redis | Redis |
| `valkey` — Valkey | `primary_replica` | Один узел ★, Sentinel (master + реплики), Кластер (шарды), Amazon ElastiCache for Valkey, Другой облачный Valkey / Redis | Valkey |
| `memcached` — Memcached | `stateless` | Свои узлы ★, Amazon ElastiCache for Memcached | Memcached |
| `keydb` — KeyDB | `primary_replica` | Один узел ★, Sentinel (master + реплики), Кластер (шарды) | — |
| `dragonfly` — Dragonfly | `primary_replica` | Один узел ★, Dragonfly Operator (Kubernetes), Dragonfly Cloud | — |
| `garnet` — Garnet | `primary_replica` | — | — |
| `redis_enterprise` — Redis Software (Redis Enterprise) | `quorum_log` | Свой кластер ★, Redis Cloud | — |
| `azure_cache_for_redis` — Azure Cache for Redis | `stateless` | Azure Managed Redis (Redis Enterprise) ★, Azure Cache for Redis (Basic, Standard, Premium) | — |
| `google_memorystore` — Google Cloud Memorystore | `stateless` | — | — |

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
| `amazon_sqs` — Amazon SQS | `stateless` | — | — |
| `amazon_sns` — Amazon SNS | `stateless` | — | — |
| `amazon_kinesis` — Amazon Kinesis Data Streams | `stateless` | — | — |
| `azure_service_bus` — Azure Service Bus | `stateless` | — | — |
| `azure_event_hubs` — Azure Event Hubs | `stateless` | — | — |
| `azure_event_grid` — Azure Event Grid | `stateless` | — | — |
| `google_pubsub` — Google Cloud Pub/Sub | `stateless` | — | — |
| `aws_eventbridge` — Amazon EventBridge | `stateless` | — | — |
| `yandex_message_queue` — Yandex Message Queue | `stateless` | — | — |
| `mosquitto` — Eclipse Mosquitto | `primary_replica` | — | — |
| `emqx` — EMQX | `stateless` | Кластер на своих серверах ★, EMQX Cloud | — |
| `hivemq` — HiveMQ | `stateless` | Кластер на своих серверах ★, HiveMQ Cloud | — |
| `vernemq` — VerneMQ | `stateless` | — | — |
| `solace_pubsub` — Solace PubSub+ | `primary_replica` | Группа высокой доступности (основной, резервный, наблюдатель) ★, Один брокер, Solace Cloud | — |
| `tibco_ems` — TIBCO Enterprise Message Service | `primary_replica` | — | — |
| `nsq` — NSQ | `stateless` | — | — |
| `apache_rocketmq` — Apache RocketMQ | `primary_replica` | Режим контроллера (5.x) ★, Классический master–slave | — |
| `confluent_platform` — Confluent Platform | `quorum_log` | — | — |

**Поиск**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `opensearch` — OpenSearch | `quorum_log` | Выделенные мастер-узлы ★, Совмещённые роли, Amazon OpenSearch Service | OpenSearch |
| `elasticsearch` — Elasticsearch | `quorum_log` | Выделенные мастер-узлы ★, Совмещённые роли, Elastic Cloud | Elasticsearch |
| `solr` — Apache Solr | `quorum_log` | — | Apache Solr |
| `meilisearch` — Meilisearch | `primary_replica` | — | Meilisearch |
| `typesense` — Typesense | `quorum_log` | Свой кластер ★, Typesense Cloud | — |
| `manticore_search` — Manticore Search | `quorum_log` | — | — |
| `algolia` — Algolia | `stateless` | — | — |
| `azure_ai_search` — Azure AI Search | `stateless` | — | — |

**Аналитические базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `clickhouse` — ClickHouse | `quorum_log` | Реплики (ClickHouse Keeper) ★, Шарды + реплики, ClickHouse Cloud / другой управляемый | ClickHouse |
| `apache_druid` — Apache Druid | `stateless` | Свой кластер ★, Управляемый Druid (Imply) | — |
| `apache_pinot` — Apache Pinot | `stateless` | Свой кластер ★, StarTree Cloud | — |
| `apache_doris` — Apache Doris | `quorum_log` | Свой кластер ★, SelectDB Cloud | — |
| `starrocks` — StarRocks | `quorum_log` | Свой кластер ★, CelerData Cloud | — |
| `greenplum` — Greenplum | `primary_replica` | Свой кластер ★, Yandex Managed Service for Greenplum | — |
| `arenadata_db` — Arenadata DB | `primary_replica` | — | — |
| `vertica` — Vertica | `quorum_log` | Enterprise (данные на узлах) ★, Eon (общее хранилище) | — |
| `teradata` — Teradata Vantage | `primary_replica` | Своя система ★, Teradata VantageCloud | — |
| `exasol` — Exasol | `primary_replica` | — | — |
| `snowflake` — Snowflake | `stateless` | — | — |
| `google_bigquery` — Google BigQuery | `stateless` | — | — |
| `amazon_redshift` — Amazon Redshift | `stateless` | — | — |
| `azure_synapse_analytics` — Azure Synapse Analytics | `stateless` | — | — |

**Объектные хранилища**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `object_storage` — Объектное хранилище | `stateless` | MinIO (distributed, erasure coding) ★, Ceph RGW, Облачное S3 (AWS S3, Yandex Object Storage…) | — |
| `amazon_s3` — Amazon S3 | `stateless` | — | — |
| `google_cloud_storage` — Google Cloud Storage | `stateless` | — | — |
| `azure_blob_storage` — Azure Blob Storage | `stateless` | — | — |
| `yandex_object_storage` — Yandex Object Storage | `stateless` | — | — |
| `selectel_s3` — Selectel Object Storage | `stateless` | — | — |
| `vk_cloud_object_storage` — VK Cloud Object Storage | `stateless` | — | — |
| `wasabi` — Wasabi | `stateless` | — | — |
| `backblaze_b2` — Backblaze B2 | `stateless` | — | — |
| `cloudflare_r2` — Cloudflare R2 | `stateless` | — | — |
| `digitalocean_spaces` — DigitalOcean Spaces | `stateless` | — | — |
| `openstack_swift` — OpenStack Swift | `stateless` | — | — |
| `garage` — Garage | `quorum_log` | — | — |
| `seaweedfs` — SeaweedFS | `quorum_log` | — | — |
| `scality_ring` — Scality RING | `stateless` | — | — |
| `netapp_storagegrid` — NetApp StorageGRID | `stateless` | — | — |
| `dell_ecs` — Dell ECS | `stateless` | — | — |
| `apache_ozone` — Apache Ozone | `quorum_log` | — | — |

**Хранилища (SDS, СХД)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `ceph` — Ceph | `quorum_log` | Развёртывание cephadm ★, Rook в Kubernetes | Ceph |
| `longhorn` — Longhorn | `stateless` | — | Longhorn |
| `nas_san` — Дисковый массив / NAS | `primary_replica` | NetApp ONTAP MetroCluster ★, NetApp ONTAP SnapMirror, TrueNAS, Другой массив (Dell, HPE, Huawei…) | NetApp ONTAP |
| `glusterfs` — GlusterFS | `quorum_log` | Replica 3 ★, Replica 2 + arbiter | — |
| `openebs` — OpenEBS | `stateless` | Replicated PV (Mayastor) ★, Local PV | — |
| `portworx` — Portworx | `quorum_log` | — | — |
| `linstor_drbd` — LINSTOR / DRBD | `primary_replica` | Синхронная репликация с кворумом ★, Асинхронная репликация на удалённую площадку | — |
| `vmware_vsan` — VMware vSAN | `quorum_log` | — | — |
| `lustre` — Lustre | `primary_replica` | — | — |
| `beegfs` — BeeGFS | `primary_replica` | — | — |
| `ibm_spectrum_scale` — IBM Storage Scale (Spectrum Scale) | `quorum_log` | — | — |
| `nfs_server` — NFS-сервер | `primary_replica` | Один сервер ★, Пара active/passive (Pacemaker) | — |
| `moosefs` — MooseFS | `primary_replica` | Community ★, Pro (резервные master) | — |
| `hdfs` — HDFS | `primary_replica` | — | Apache Hadoop |

**Резервное копирование**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `backup_storage` — Бэкапы (offsite) | `stateless` | Облачное хранилище с Object Lock ★, Veeam Backup & Replication, Proxmox Backup Server, Commvault, restic / Borg + хранилище, Ленточная библиотека | — |
| `bacula` — Bacula | `stateless` | — | — |
| `bareos` — Bareos | `stateless` | — | — |
| `veritas_netbackup` — NetBackup | `stateless` | — | — |
| `cohesity` — Cohesity | `stateless` | Свой кластер ★, Cohesity DataProtect как услуга | — |
| `rubrik` — Rubrik | `stateless` | — | — |
| `dell_powerprotect` — Dell PowerProtect | `stateless` | — | — |
| `ibm_spectrum_protect` — IBM Storage Protect (Spectrum Protect) | `stateless` | — | — |
| `acronis_cyber_protect` — Acronis Cyber Protect | `stateless` | Свой сервер управления ★, Acronis Cyber Protect Cloud | — |
| `arcserve` — Arcserve UDP | `stateless` | — | — |
| `hycu` — HYCU | `stateless` | — | — |
| `nakivo` — NAKIVO Backup & Replication | `stateless` | — | — |
| `pgbackrest` — pgBackRest | `stateless` | — | — |
| `barman` — Barman | `stateless` | — | — |
| `aws_backup` — AWS Backup | `stateless` | — | — |
| `kopia` — Kopia | `stateless` | — | — |
| `velero` — Velero | `stateless` | В кластере Kubernetes ★ | — |
| `kasten_k10` — Veeam Kasten (K10) | `stateless` | В кластере Kubernetes ★ | — |
| `wal_g` — WAL-G | `stateless` | — | — |
| `zerto` — HPE Zerto | `stateless` | — | HPE Zerto |
| `vmware_srm` — VMware Site Recovery Manager | `stateless` | — | VMware Site Recovery Manager |

**Векторные базы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `pgvector` — pgvector | `primary_replica` | Patroni + etcd/Consul ★, Потоковая репликация, ручное переключение, Amazon RDS for PostgreSQL (Multi-AZ), Amazon Aurora PostgreSQL, Другой облачный PostgreSQL с pgvector | PostgreSQL |
| `milvus` — Milvus | `stateless` | Distributed (кластер) ★, Standalone (один процесс), Zilliz Cloud (управляемый Milvus) | — |
| `qdrant` — Qdrant | `quorum_log` | Свой кластер ★, Qdrant Cloud | — |
| `weaviate` — Weaviate | `quorum_log` | Свой кластер ★, Weaviate Cloud | — |
| `chroma` — Chroma | `primary_replica` | Один сервер (клиент-серверный режим) ★, Chroma Cloud | — |
| `pinecone` — Pinecone | `stateless` | — | — |
| `vespa` — Vespa | `quorum_log` | Свой кластер ★, Vespa Cloud | — |
| `lancedb` — LanceDB | `stateless` | Данные в объектном хранилище ★, Данные на локальном диске, LanceDB Cloud / Enterprise | — |

**Системы хранения (СХД и NAS)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `netapp_ontap` — NetApp ONTAP | `primary_replica` | HA-пара + SnapMirror ★, MetroCluster, SnapMirror active sync, Amazon FSx for NetApp ONTAP | NetApp ONTAP |
| `dell_powerstore` — Dell PowerStore | `primary_replica` | Асинхронная репликация ★, Metro Volume | — |
| `dell_powermax` — Dell PowerMax | `primary_replica` | SRDF/S и SRDF/A ★, SRDF/Metro | — |
| `dell_powerscale` — Dell PowerScale | `quorum_log` | — | — |
| `pure_flasharray` — Pure Storage FlashArray | `primary_replica` | Асинхронная репликация и ActiveDR ★, ActiveCluster | — |
| `hpe_primera` — HPE Primera / Alletra 9000 | `primary_replica` | Remote Copy ★, Peer Persistence | — |
| `hpe_nimble` — HPE Nimble Storage / Alletra 5000, 6000 | `primary_replica` | Репликация снимков ★, Peer Persistence | — |
| `huawei_oceanstor` — Huawei OceanStor | `primary_replica` | HyperReplication ★, HyperMetro | — |
| `hitachi_vsp` — Hitachi VSP | `primary_replica` | Universal Replicator / TrueCopy ★, Global-Active Device | — |
| `ibm_flashsystem` — IBM FlashSystem | `primary_replica` | Metro Mirror / Global Mirror ★, HyperSwap | — |
| `synology` — Synology NAS | `primary_replica` | Один NAS ★, Synology High Availability | — |
| `qnap` — QNAP NAS | `primary_replica` | Один контроллер ★, Два контроллера (серия ES) | — |
| `yadro_tatlin` — YADRO TATLIN.UNIFIED | `primary_replica` | — | — |
| `truenas` — TrueNAS | `primary_replica` | Community (один узел) ★, Enterprise HA (два контроллера) | TrueNAS |

**Интеграция и репликация данных**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `debezium` — Debezium | `stateless` | Коннекторы в Kafka Connect ★, Debezium Server | — |
| `kafka_connect` — Kafka Connect | `stateless` | Распределённый режим ★, Автономный режим | Apache Kafka |
| `airbyte` — Airbyte | `stateless` | Свой Airbyte ★, Airbyte Cloud | — |
| `fivetran` — Fivetran | `stateless` | — | — |
| `talend` — Qlik Talend | `stateless` | — | — |
| `informatica` — Informatica | `stateless` | — | — |
| `striim` — Striim | `stateless` | Свой кластер ★, Striim Cloud | — |
| `streamsets` — IBM StreamSets | `stateless` | — | — |
| `meltano` — Meltano | `stateless` | — | — |
| `symmetricds` — SymmetricDS | `stateless` | — | — |
| `oracle_goldengate` — Oracle GoldenGate | `stateless` | Свой GoldenGate ★, OCI GoldenGate | — |
| `aws_dms` — AWS Database Migration Service | `stateless` | — | — |

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
| `cyberark_conjur` — CyberArk Conjur | `primary_replica` | Кластер: Leader + Standby (автопереключение) ★, Leader + Standby, ручное переключение, CyberArk Secrets Manager SaaS (Conjur Cloud) | — |
| `aws_secrets_manager` — AWS Secrets Manager | `stateless` | — | — |
| `azure_key_vault` — Azure Key Vault | `stateless` | — | — |
| `gcp_secret_manager` — Google Secret Manager | `stateless` | — | — |
| `yandex_lockbox` — Yandex Lockbox | `stateless` | — | — |
| `infisical` — Infisical | `stateless` | Свой Infisical ★, Infisical Cloud | — |
| `doppler` — Doppler | `stateless` | — | — |

**Мониторинг**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `monitoring` — Мониторинг | `stateless` | Prometheus HA (пара) ★, VictoriaMetrics (кластер), Облачный мониторинг (Managed Prometheus, Grafana Cloud) | Prometheus |
| `zabbix` — Zabbix | `primary_replica` | Кластер active/standby ★, Один сервер | Zabbix |
| `icinga` — Icinga | `stateless` | — | Icinga |
| `checkmk` — Checkmk | `stateless` | — | Checkmk |
| `nagios` — Nagios | `stateless` | — | — |
| `datadog` — Datadog | `stateless` | — | — |
| `new_relic` — New Relic | `stateless` | — | — |
| `dynatrace` — Dynatrace | `stateless` | — | — |
| `splunk_observability_cloud` — Splunk Observability Cloud | `stateless` | — | — |
| `signoz` — SigNoz | `stateless` | Свой SigNoz (Docker, Kubernetes) ★, SigNoz Cloud | — |
| `uptime_kuma` — Uptime Kuma | `stateless` | — | — |
| `prtg` — PRTG Network Monitor | `stateless` | — | — |
| `librenms` — LibreNMS | `stateless` | — | — |
| `netdata` — Netdata | `stateless` | — | — |
| `opennms` — OpenNMS | `stateless` | — | — |
| `centreon` — Centreon | `stateless` | — | Centreon |
| `yandex_monitoring` — Yandex Monitoring | `stateless` | — | — |

**Сбор телеметрии**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `telegraf` — Telegraf | `stateless` | — | Telegraf |
| `fluent_bit` — Fluent Bit | `stateless` | — | Fluent Bit |
| `otel_collector` — OpenTelemetry Collector | `stateless` | — | — |
| `fluentd` — Fluentd | `stateless` | — | — |
| `vector` — Vector | `stateless` | — | — |
| `filebeat` — Filebeat | `stateless` | — | Elastic Beats |
| `grafana_alloy` — Grafana Alloy | `stateless` | — | — |
| `syslog_ng` — syslog-ng | `stateless` | — | — |
| `rsyslog` — rsyslog | `stateless` | — | — |
| `cribl_stream` — Cribl Stream | `stateless` | Свой Leader и рабочие узлы ★, Cribl.Cloud | — |

**Логи**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `loki` — Grafana Loki | `stateless` | — | Grafana Loki |
| `graylog` — Graylog | `stateless` | — | Graylog |
| `logstash` — Logstash | `stateless` | — | Logstash |
| `kibana` — Kibana | `stateless` | — | Kibana |
| `sumo_logic` — Sumo Logic | `stateless` | — | — |
| `opensearch_dashboards` — OpenSearch Dashboards | `stateless` | — | OpenSearch |
| `victorialogs` — VictoriaLogs | `stateless` | Одиночные узлы с дублированием потока ★, Кластер vlinsert / vlselect / vlstorage | — |

**Трассировка**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `jaeger` — Jaeger | `stateless` | — | Jaeger |
| `zipkin` — Zipkin | `stateless` | — | — |
| `grafana_tempo` — Grafana Tempo | `stateless` | — | — |
| `apache_skywalking` — Apache SkyWalking | `stateless` | — | — |
| `elastic_apm` — Elastic APM | `stateless` | — | Elasticsearch |
| `honeycomb` — Honeycomb | `stateless` | — | — |

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
| `github` — GitHub | `stateless` | — | — |
| `github_enterprise_server` — GitHub Enterprise Server | `primary_replica` | Основной аппарат и пассивная реплика ★, Один аппарат | — |
| `github_actions_runner` — Раннеры GitHub Actions | `stateless` | — | — |
| `gitlab_runner` — GitLab Runner | `stateless` | — | GitLab Runner |
| `bitbucket` — Bitbucket | `stateless` | Data Center (кластер на своих серверах) ★, Bitbucket Cloud | Bitbucket |
| `gitea` — Gitea | `stateless` | — | — |
| `teamcity` — TeamCity | `stateless` | Многоузловая конфигурация ★, Один сервер, TeamCity Cloud | — |
| `bamboo` — Bamboo | `stateless` | — | Bamboo |
| `circleci` — CircleCI | `stateless` | — | — |
| `woodpecker_ci` — Woodpecker CI | `stateless` | — | — |
| `tekton` — Tekton | `stateless` | — | — |
| `buildkite` — Buildkite | `stateless` | — | — |
| `azure_devops` — Azure DevOps | `stateless` | Azure DevOps Server (на своих серверах) ★, Azure DevOps Services | Azure DevOps Server |
| `gerrit` — Gerrit | `stateless` | — | Gerrit |
| `concourse` — Concourse | `stateless` | — | — |

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
| `distribution` — Distribution (Docker Registry) | `stateless` | — | — |
| `quay` — Red Hat Quay | `stateless` | — | — |
| `aws_ecr` — Amazon ECR | `stateless` | — | — |
| `google_artifact_registry` — Google Artifact Registry | `stateless` | — | — |
| `azure_container_registry` — Azure Container Registry | `stateless` | — | — |
| `yandex_container_registry` — Yandex Container Registry | `stateless` | — | — |
| `docker_hub` — Docker Hub | `stateless` | — | — |
| `github_container_registry` — GitHub Container Registry | `stateless` | — | — |
| `cloudsmith` — Cloudsmith | `stateless` | — | — |

**Идентификация**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `keycloak` — Keycloak | `stateless` | Кластер на одной площадке ★, Несколько площадок (active/active) | Keycloak |
| `authentik` — authentik | `stateless` | — | authentik |
| `okta` — Okta | `stateless` | — | — |
| `auth0` — Auth0 | `stateless` | — | — |
| `microsoft_entra_id` — Microsoft Entra ID | `stateless` | — | — |
| `jumpcloud` — JumpCloud | `stateless` | — | — |
| `amazon_cognito` — Amazon Cognito | `stateless` | — | — |
| `pingfederate` — PingFederate | `stateless` | — | — |
| `pingam` — PingAM (ForgeRock AM) | `stateless` | — | — |
| `ory_hydra` — Ory Hydra | `stateless` | Свой Hydra ★, Ory Network (облако) | — |
| `ory_kratos` — Ory Kratos | `stateless` | Свой Kratos ★, Ory Network (облако) | — |
| `dex` — Dex | `stateless` | — | — |
| `zitadel` — ZITADEL | `stateless` | Свой ZITADEL ★, ZITADEL Cloud | — |
| `fusionauth` — FusionAuth | `stateless` | Свой FusionAuth ★, FusionAuth Cloud | — |
| `wso2_identity_server` — WSO2 Identity Server | `stateless` | — | — |
| `apereo_cas` — Apereo CAS | `stateless` | — | — |
| `shibboleth` — Shibboleth Identity Provider | `stateless` | — | — |
| `authelia` — Authelia | `stateless` | — | — |
| `privacyidea` — privacyIDEA | `stateless` | — | — |
| `freeradius` — FreeRADIUS | `stateless` | — | — |
| `spire` — SPIFFE/SPIRE | `stateless` | — | — |
| `sailpoint` — SailPoint | `stateless` | IdentityIQ на своих серверах ★, SailPoint Identity Security Cloud | — |
| `microsoft_active_directory` — Microsoft Active Directory | `stateless` | Свои контроллеры домена ★, Управляемый AD (AWS Managed Microsoft AD, Microsoft Entra Domain Services) | Windows Server |
| `microsoft_adfs` — Active Directory Federation Services | `stateless` | — | Windows Server |
| `freeipa` — FreeIPA | `stateless` | — | — |
| `openldap` — OpenLDAP | `stateless` | — | — |
| `ds389` — 389 Directory Server | `stateless` | — | — |
| `samba_ad` — Samba AD DC | `stateless` | — | Samba |
| `univention_corporate_server` — Univention Corporate Server | `stateless` | — | — |

**Доступ к инфраструктуре**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `teleport` — Teleport | `stateless` | Свой кластер ★, Teleport Enterprise Cloud | Teleport |
| `boundary` — HashiCorp Boundary | `stateless` | — | HashiCorp Boundary |

**Сертификаты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cert_manager` — cert-manager | `stateless` | — | cert-manager |
| `smallstep_ca` — step-ca (Smallstep) | `stateless` | — | — |
| `ejbca` — EJBCA | `stateless` | — | — |
| `dogtag_pki` — Dogtag PKI | `stateless` | — | — |
| `microsoft_adcs` — Active Directory Certificate Services | `stateless` | — | Windows Server |
| `lets_encrypt` — Let’s Encrypt | `stateless` | — | — |
| `aws_certificate_manager` — AWS Certificate Manager | `stateless` | — | — |
| `aws_private_ca` — AWS Private CA | `stateless` | — | — |
| `digicert` — DigiCert | `stateless` | — | — |
| `yandex_certificate_manager` — Yandex Certificate Manager | `stateless` | — | — |

**Политики**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kyverno` — Kyverno | `stateless` | — | Kyverno |
| `gatekeeper` — OPA Gatekeeper | `stateless` | — | OPA Gatekeeper |
| `open_policy_agent` — Open Policy Agent | `stateless` | — | — |
| `kubewarden` — Kubewarden | `stateless` | — | — |
| `cloud_custodian` — Cloud Custodian | `stateless` | — | — |
| `openfga` — OpenFGA | `stateless` | — | — |
| `spicedb` — SpiceDB | `stateless` | — | — |

**Сеть кластера**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cni` — Сеть кластера (CNI) | `stateless` | Cilium (eBPF) ★, Calico, Flannel, CNI провайдера (VPC CNI, Azure CNI) | Cilium |
| `antrea` — Antrea | `stateless` | — | — |
| `multus` — Multus CNI | `stateless` | — | — |
| `kube_ovn` — Kube-OVN | `stateless` | — | — |
| `ovn_kubernetes` — OVN-Kubernetes | `stateless` | — | — |
| `kube_router` — kube-router | `stateless` | — | — |
| `submariner` — Submariner | `stateless` | — | — |

**Сервисная сетка**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `service_mesh` — Сервисная сетка | `stateless` | Istio ★, Linkerd, Cilium Service Mesh, Consul Connect | Istio |
| `kuma_mesh` — Kuma | `stateless` | Kubernetes (состояние в CRD) ★, Universal (виртуальные машины, PostgreSQL) | Kuma |
| `kong_mesh` — Kong Mesh | `stateless` | — | — |
| `google_cloud_service_mesh` — Google Cloud Service Mesh | `stateless` | — | — |

**BI и аналитика**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `apache_superset` — Apache Superset | `stateless` | На своих серверах ★, Preset (облако) | — |
| `metabase` — Metabase | `stateless` | На своих серверах ★, Metabase Cloud | Metabase |
| `redash` — Redash | `stateless` | — | — |
| `tableau_server` — Tableau Server | `stateless` | Tableau Server (на своих серверах) ★, Tableau Cloud | — |
| `power_bi` — Power BI | `stateless` | — | — |
| `looker` — Looker | `stateless` | — | Looker |
| `yandex_datalens` — Yandex DataLens | `stateless` | — | — |

**Доставка и релизы (CD)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `spinnaker` — Spinnaker | `stateless` | — | — |
| `octopus_deploy` — Octopus Deploy | `stateless` | Octopus Server (на своих серверах) ★, Octopus Cloud | — |
| `harness` — Harness | `stateless` | — | — |
| `aws_codepipeline` — AWS CodePipeline | `stateless` | — | — |
| `aws_codedeploy` — AWS CodeDeploy | `stateless` | — | — |
| `google_cloud_deploy` — Google Cloud Deploy | `stateless` | — | — |

**Оповещения и дежурства**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `prometheus_alertmanager` — Prometheus Alertmanager | `stateless` | — | — |
| `pagerduty` — PagerDuty | `stateless` | — | — |

**Инфраструктура как код и управление конфигурацией**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `terraform` — Terraform | `stateless` | CLI в CI с удалённым состоянием ★, Terraform Enterprise (свой сервер), HCP Terraform | Hashicorp Terraform |
| `opentofu` — OpenTofu | `stateless` | — | OpenTofu |
| `pulumi` — Pulumi | `stateless` | Свой бэкенд состояния ★, Pulumi Cloud | — |
| `ansible` — Ansible | `stateless` | — | Ansible |
| `ansible_awx` — AWX | `stateless` | — | — |
| `red_hat_aap` — Red Hat Ansible Automation Platform | `stateless` | — | Red Hat Ansible Automation Platform |
| `chef_infra_server` — Chef Infra Server | `stateless` | — | Chef Infra Server |
| `puppet` — Puppet | `stateless` | — | Puppet |
| `saltstack` — Salt | `stateless` | — | Salt |
| `cfengine` — CFEngine | `stateless` | — | CFEngine |
| `aws_cloudformation` — AWS CloudFormation | `stateless` | — | — |
| `crossplane` — Crossplane | `stateless` | — | — |
| `hashicorp_packer` — HashiCorp Packer | `stateless` | — | Hashicorp Packer |
| `atlantis` — Atlantis | `stateless` | — | — |
| `spacelift` — Spacelift | `stateless` | — | — |
| `env0` — env0 | `stateless` | — | — |
| `rundeck` — Rundeck | `stateless` | — | — |
| `foreman` — Foreman | `stateless` | — | Foreman |
| `cobbler` — Cobbler | `stateless` | — | — |
| `canonical_maas` — MAAS | `stateless` | — | — |
| `openstack_ironic` — OpenStack Ironic | `stateless` | — | — |

**Флаги функций**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `unleash` — Unleash | `stateless` | Свой Unleash ★, Unleash (облако) | — |
| `flagsmith` — Flagsmith | `stateless` | Свой Flagsmith ★, Flagsmith (облако) | — |
| `launchdarkly` — LaunchDarkly | `stateless` | — | — |
| `growthbook` — GrowthBook | `stateless` | Свой GrowthBook ★, GrowthBook Cloud | — |
| `configcat` — ConfigCat | `stateless` | — | — |

**Обнаружение сервисов**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `netflix_eureka` — Netflix Eureka | `stateless` | — | — |
| `aws_cloud_map` — AWS Cloud Map | `stateless` | — | — |
| `alibaba_nacos` — Nacos | `consensus` | Свой кластер Nacos ★, Alibaba Cloud MSE (управляемый Nacos) | — |

**Ключи и HSM**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `thales_luna_hsm` — Thales Luna Network HSM | `stateless` | — | — |
| `entrust_nshield` — Entrust nShield HSM | `stateless` | — | — |
| `utimaco_hsm` — Utimaco General Purpose HSM | `stateless` | — | — |
| `aws_kms` — AWS KMS | `stateless` | — | — |
| `yandex_kms` — Yandex Key Management Service | `stateless` | — | — |

**Инструменты работы с секретами**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `sops` — SOPS | `stateless` | — | — |
| `external_secrets_operator` — External Secrets Operator | `stateless` | — | — |
| `sealed_secrets` — Sealed Secrets | `stateless` | — | — |
| `secrets_store_csi_driver` — Secrets Store CSI Driver | `stateless` | — | — |

### Безопасность

**SIEM и аналитика угроз**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `splunk_enterprise_security` — Splunk Enterprise Security | `quorum_log` | Кластер search head’ов (SHC) ★, Один search head, Splunk Cloud Platform | Splunk |
| `splunk_indexers` — Индексаторы Splunk Enterprise | `stateless` | Локальные диски пиров ★, SmartStore (S3-совместимое хранилище) | Splunk |
| `ibm_qradar` — IBM QRadar SIEM | `stateless` | — | — |
| `arcsight_esm` — OpenText ArcSight ESM | `stateless` | — | — |
| `security_onion` — Security Onion | `stateless` | Распределённая сетка (менеджер + поисковые узлы + сенсоры) ★, Один узел (standalone) | — |
| `kuma` — Kaspersky Unified Monitoring and Analysis Platform (KUMA) | `stateless` | — | Kuma |
| `wazuh` — Wazuh | `stateless` | Свой кластер (мастер + воркеры) ★, Wazuh Cloud | — |
| `wazuh_indexer` — Wazuh indexer | `quorum_log` | — | — |
| `elastic_security` — Elastic Security | `stateless` | Свой стек (Kibana + Fleet Server) ★, Elastic Cloud (Hosted или Serverless) | Kibana |
| `microsoft_sentinel` — Microsoft Sentinel | `stateless` | — | — |
| `google_secops` — Google Security Operations (Chronicle) | `stateless` | — | — |
| `exabeam` — Exabeam | `stateless` | — | — |
| `cortex_xsiam` — Cortex XSIAM | `stateless` | — | — |
| `sumo_logic_cloud_siem` — Sumo Logic Cloud SIEM | `stateless` | — | — |
| `misp` — MISP | `stateless` | — | — |
| `opencti` — OpenCTI | `stateless` | — | — |

**EDR и XDR (защита конечных точек)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `crowdstrike_falcon` — CrowdStrike Falcon | `stateless` | — | — |
| `sentinelone` — SentinelOne Singularity | `stateless` | — | — |
| `microsoft_defender_for_endpoint` — Microsoft Defender for Endpoint | `stateless` | — | — |
| `cortex_xdr` — Cortex XDR | `stateless` | — | Palo Alto Networks Cortex XDR agent |
| `carbon_black` — Carbon Black Cloud | `stateless` | — | — |
| `trend_vision_one` — Trend Vision One | `stateless` | — | — |
| `sophos_intercept_x` — Sophos Intercept X | `stateless` | — | — |
| `kaspersky_edr` — Kaspersky EDR | `stateless` | Серверы у вас ★, Облачная консоль Kaspersky | — |
| `eset_protect` — ESET PROTECT | `stateless` | ESET PROTECT On-Prem ★, ESET PROTECT Cloud | — |

**IDS/IPS и сетевой мониторинг безопасности**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `suricata` — Suricata | `stateless` | IDS: зеркало порта или TAP ★, IPS: в разрыв (inline) | — |
| `snort` — Snort | `stateless` | IDS: зеркало порта или TAP ★, IPS: в разрыв (inline) | — |
| `zeek` — Zeek | `stateless` | — | — |
| `falco` — Falco | `stateless` | DaemonSet в Kubernetes ★, Служба на хосте (пакет, systemd) | — |
| `cilium_tetragon` — Tetragon | `stateless` | — | — |
| `aqua_tracee` — Aqua Tracee | `stateless` | — | — |
| `crowdsec` — CrowdSec | `stateless` | — | — |
| `arkime` — Arkime | `stateless` | — | — |
| `pt_nad` — Positive Technologies Network Attack Discovery (PT NAD) | `stateless` | — | — |

**Управление уязвимостями**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `tenable_nessus` — Tenable Nessus | `stateless` | — | — |
| `tenable_security_center` — Tenable Security Center | `stateless` | — | — |
| `tenable_vm` — Tenable Vulnerability Management | `stateless` | — | — |
| `qualys_vmdr` — Qualys VMDR | `stateless` | — | — |
| `openvas` — OpenVAS (Greenbone) | `stateless` | — | — |
| `rapid7_insightvm` — Rapid7 InsightVM | `stateless` | — | — |

**SOAR и реагирование на инциденты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `thehive` — TheHive | `stateless` | Кластер (Cassandra + Elasticsearch + S3) ★, Один узел | — |
| `shuffle` — Shuffle | `stateless` | — | — |
| `cortex_xsoar` — Cortex XSOAR | `stateless` | Свой сервер ★, Cortex XSOAR 8 (облако) | — |
| `splunk_soar` — Splunk SOAR | `stateless` | Локальная установка (узел или кластер) ★, Splunk SOAR Cloud | — |

**Безопасность приложений и цепочки поставок**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `snyk` — Snyk | `stateless` | — | — |
| `checkmarx` — Checkmarx One | `stateless` | — | — |
| `veracode` — Veracode | `stateless` | — | — |
| `opentext_fortify` — OpenText Fortify | `stateless` | Свои серверы (Software Security Center + сенсоры) ★, Fortify on Demand (OpenText Core Application Security) | — |
| `black_duck` — Black Duck | `stateless` | — | — |
| `semgrep` — Semgrep | `stateless` | — | — |
| `trivy` — Trivy | `stateless` | Задание в CI (командная строка) ★, Режим server (общий сервис с кэшем) | — |
| `grype` — Grype | `stateless` | — | — |
| `anchore_enterprise` — Anchore Enterprise | `stateless` | — | — |
| `clair` — Clair | `stateless` | — | — |
| `dependency_track` — Dependency-Track | `stateless` | — | Dependency-Track |
| `defectdojo` — DefectDojo | `stateless` | — | — |
| `jfrog_xray` — JFrog Xray | `stateless` | — | JFrog Xray |
| `owasp_zap` — ZAP (Zed Attack Proxy) | `stateless` | — | — |
| `gitleaks` — Gitleaks | `stateless` | — | — |

**Защита от утечек (DLP)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `symantec_dlp` — Symantec Data Loss Prevention | `stateless` | Мониторинг и обнаружение (вне потока) ★, Блокировка в потоке (Network Prevent) | — |
| `forcepoint_dlp` — Forcepoint DLP | `stateless` | Свои серверы (Forcepoint DLP On-Premises) ★, Forcepoint DLP SaaS | — |
| `infowatch_traffic_monitor` — InfoWatch Traffic Monitor | `stateless` | — | — |
| `solar_dozor` — Solar Dozor | `stateless` | — | — |
| `zecurion_dlp` — Zecurion DLP | `stateless` | — | — |
| `digital_guardian` — Digital Guardian | `stateless` | — | — |
| `microsoft_purview_dlp` — Microsoft Purview DLP | `stateless` | — | — |

**Контроль доступа к сети (NAC)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cisco_ise` — Cisco Identity Services Engine | `stateless` | — | — |
| `aruba_clearpass` — Aruba ClearPass | `stateless` | — | — |
| `packetfence` — PacketFence | `quorum_log` | — | — |
| `fortinac` — Fortinet FortiNAC | `stateless` | — | — |
| `forescout` — Forescout Platform | `stateless` | — | — |

**Привилегированный доступ (PAM)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cyberark_pam` — CyberArk Privileged Access Manager | `primary_replica` | Кластер Vault (общее хранилище) ★, Vault + DR-копия (репликация), CyberArk Privilege Cloud | — |
| `delinea_secret_server` — Delinea Secret Server | `stateless` | Свои узлы IIS за балансировщиком ★, Secret Server Cloud | — |
| `wallix_bastion` — WALLIX Bastion | `primary_replica` | — | — |
| `apache_guacamole` — Apache Guacamole | `stateless` | — | — |
| `jumpserver` — JumpServer | `stateless` | — | — |

**Защита почты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `proofpoint` — Proofpoint Email Protection | `stateless` | — | — |
| `mimecast` — Mimecast Email Security | `stateless` | — | — |
| `microsoft_defender_for_office_365` — Microsoft Defender for Office 365 | `stateless` | — | — |
| `cisco_secure_email` — Cisco Secure Email Gateway | `stateless` | Свои устройства или виртуальные машины ★, Cisco Secure Email Cloud Gateway | — |
| `barracuda_email_security` — Barracuda Email Security Gateway | `stateless` | — | — |
| `kaspersky_secure_mail_gateway` — Kaspersky Secure Mail Gateway | `stateless` | — | — |
| `rspamd` — Rspamd | `stateless` | — | — |
| `spamassassin` — Apache SpamAssassin | `stateless` | — | — |
| `clamav` — ClamAV | `stateless` | — | ClamAV |
| `proxmox_mail_gateway` — Proxmox Mail Gateway | `stateless` | — | Proxmox Mail Gateway |

**Безопасность облаков (CSPM, CWPP)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `wiz` — Wiz | `stateless` | — | — |
| `orca_security` — Orca Security | `stateless` | — | — |
| `palo_alto_prisma_cloud` — Palo Alto Prisma Cloud | `stateless` | — | — |
| `aqua_security` — Aqua Platform | `stateless` | Самостоятельная установка (сервер, gateway, PostgreSQL) ★, Aqua SaaS | — |
| `sysdig_secure` — Sysdig Secure | `stateless` | — | — |
| `microsoft_defender_for_cloud` — Microsoft Defender for Cloud | `stateless` | — | — |
| `amazon_guardduty` — Amazon GuardDuty | `stateless` | — | — |
| `google_security_command_center` — Google Security Command Center | `stateless` | — | — |

**Zero Trust доступ (ZTNA)**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `cloudflare_access` — Cloudflare Access | `stateless` | — | — |
| `zscaler_private_access` — Zscaler Private Access | `stateless` | — | — |
| `twingate` — Twingate | `stateless` | — | — |
| `google_cloud_iap` — Google Cloud Identity-Aware Proxy | `stateless` | — | — |
| `microsoft_entra_private_access` — Microsoft Entra Private Access | `stateless` | — | — |
| `pomerium` — Pomerium | `stateless` | Databroker на PostgreSQL ★, Databroker в памяти (по умолчанию) | — |

### Приложения и сервисы

**Service desk и ITSM**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `servicenow` — ServiceNow | `stateless` | — | — |
| `jira_service_management` — Jira Service Management | `stateless` | Data Center (кластер узлов) ★, Jira Service Management Cloud | Jira Service Management |
| `zammad` — Zammad | `stateless` | — | — |
| `glpi` — GLPI | `stateless` | — | — |
| `otobo` — OTOBO | `stateless` | — | OTOBO |
| `itop` — iTop | `stateless` | — | — |
| `naumen_service_desk` — NAUMEN Service Desk | `stateless` | — | — |
| `bmc_helix` — BMC Helix ITSM | `stateless` | — | — |

**Трекеры задач и вики**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `jira_software` — Jira Software | `stateless` | Data Center (кластер узлов) ★, Jira Cloud | Jira Software |
| `confluence` — Confluence | `stateless` | Data Center (кластер узлов) ★, Confluence Cloud | Confluence |
| `redmine` — Redmine | `stateless` | — | Redmine |
| `youtrack` — YouTrack | `stateless` | Свой сервер (standalone) ★, YouTrack Cloud | YouTrack |
| `bugzilla` — Bugzilla | `stateless` | — | — |
| `openproject` — OpenProject | `stateless` | — | — |
| `mediawiki` — MediaWiki | `stateless` | — | MediaWiki |
| `bookstack` — BookStack | `stateless` | — | — |
| `outline` — Outline | `stateless` | — | — |

**Мессенджеры и конференции**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `mattermost` — Mattermost | `stateless` | — | Mattermost |
| `rocket_chat` — Rocket.Chat | `stateless` | — | Rocket.Chat |
| `matrix_synapse` — Matrix Synapse | `stateless` | — | — |
| `jitsi_meet` — Jitsi Meet | `stateless` | Свои серверы ★, Jitsi as a Service (8x8) | — |
| `bigbluebutton` — BigBlueButton | `stateless` | — | BigBlueButton |
| `zulip` — Zulip | `stateless` | — | — |
| `ejabberd` — ejabberd | `stateless` | — | — |
| `microsoft_teams` — Microsoft Teams | `stateless` | — | — |
| `slack` — Slack | `stateless` | — | — |
| `zoom` — Zoom | `stateless` | — | — |

**Файлы и офис**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `nextcloud` — Nextcloud | `stateless` | Кластер веб-узлов ★, Nextcloud у хостинг-провайдера | Nextcloud |
| `seafile` — Seafile | `stateless` | Pro: кластер узлов ★, Community: один сервер | — |
| `collabora_online` — Collabora Online | `stateless` | — | — |
| `onlyoffice_docs` — ONLYOFFICE Docs | `stateless` | — | — |
| `microsoft_sharepoint` — Microsoft SharePoint | `stateless` | Ферма SharePoint Server ★, SharePoint Online (Microsoft 365) | Microsoft SharePoint |
| `google_workspace` — Google Workspace | `stateless` | — | — |

**Почтовые серверы и клиенты**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `postfix` — Postfix | `stateless` | — | Postfix |
| `exim` — Exim | `stateless` | — | Exim |
| `dovecot` — Dovecot | `stateless` | Пара серверов с репликацией dsync ★, Общее хранилище (NFS или кластерная ФС) | Dovecot |
| `microsoft_exchange_server` — Microsoft Exchange Server | `stateless` | DAG (группа доступности баз данных) ★, Exchange Online (Microsoft 365) | Microsoft Exchange |
| `zimbra` — Zimbra Collaboration | `stateless` | — | — |
| `mailcow` — mailcow | `stateless` | — | — |
| `iredmail` — iRedMail | `stateless` | — | — |
| `roundcube` — Roundcube Webmail | `stateless` | — | Roundcube Webmail |
| `sogo` — SOGo | `stateless` | — | — |
| `stalwart_mail` — Stalwart Mail Server | `stateless` | Кластер с внешними хранилищами ★, Один узел со встроенным хранилищем | — |

**Телефония и VoIP**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `asterisk` — Asterisk | `stateless` | — | — |
| `freeswitch` — FreeSWITCH | `stateless` | — | — |
| `kamailio` — Kamailio | `stateless` | — | — |
| `opensips` — OpenSIPS | `stateless` | — | — |
| `cisco_unified_communications_manager` — Cisco Unified Communications Manager | `stateless` | — | — |
| `three_cx` — 3CX Phone System | `stateless` | — | — |
| `freepbx` — FreePBX | `stateless` | — | — |
| `rtpengine` — rtpengine | `stateless` | — | rtpengine |
| `coturn` — coturn | `stateless` | — | — |

**CMS и порталы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `wordpress` — WordPress | `stateless` | Узлы PHP-FPM за балансировщиком ★, Kubernetes, Управляемый WordPress (WordPress.com, WP Engine, Kinsta…) | WordPress |
| `drupal` — Drupal | `stateless` | Узлы PHP-FPM за балансировщиком ★, Управляемый Drupal (Acquia, Pantheon…) | Drupal |
| `joomla` — Joomla! | `stateless` | — | Joomla! |
| `typo3` — TYPO3 | `stateless` | — | TYPO3 |
| `umbraco` — Umbraco CMS | `stateless` | Свои серверы за балансировщиком ★, Umbraco Cloud | Umbraco CMS |
| `craft_cms` — Craft CMS | `stateless` | — | Craft CMS |
| `bitrix_site_manager` — 1С-Битрикс: Управление сайтом | `stateless` | — | — |
| `strapi` — Strapi | `stateless` | Свои узлы ★, Strapi Cloud | — |
| `directus` — Directus | `stateless` | Свои узлы ★, Directus Cloud | — |
| `liferay` — Liferay | `stateless` | — | — |

**Электронная коммерция**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `adobe_commerce_magento` — Adobe Commerce (Magento) | `stateless` | Свои серверы ★, Adobe Commerce on cloud infrastructure | Magento |
| `shopware` — Shopware | `stateless` | — | Shopware |
| `prestashop` — PrestaShop | `stateless` | — | — |
| `woocommerce` — WooCommerce | `stateless` | — | — |
| `opencart` — OpenCart | `stateless` | — | — |
| `sylius` — Sylius | `stateless` | — | — |
| `saleor` — Saleor | `stateless` | Свои узлы ★, Saleor Cloud | — |
| `medusa` — Medusa | `stateless` | Свои узлы ★, Medusa Cloud | — |
| `shopify` — Shopify | `stateless` | — | — |
| `commercetools` — commercetools | `stateless` | — | — |

**ERP, CRM и бизнес-системы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `one_c_enterprise` — 1С:Предприятие | `stateless` | Свой кластер серверов 1С ★, Облачный сервис (1С:Фреш, облака партнёров) | — |
| `bitrix24` — Битрикс24 | `stateless` | «Коробка» на своих серверах ★, Облако Битрикс24 | — |
| `sap_s4hana` — SAP S/4HANA | `stateless` | Свои серверы или IaaS (on-premise) ★, SAP S/4HANA Cloud и RISE with SAP | — |
| `odoo` — Odoo | `stateless` | Свои серверы ★, Odoo Online и Odoo.sh | — |
| `erpnext` — ERPNext | `stateless` | Свои серверы ★, Frappe Cloud | — |
| `microsoft_dynamics_365` — Microsoft Dynamics 365 | `stateless` | — | — |
| `suitecrm` — SuiteCRM | `stateless` | — | — |
| `salesforce` — Salesforce | `stateless` | — | — |

**Обучение и сообщества**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `moodle` — Moodle | `stateless` | Свои серверы ★, MoodleCloud и хостинг партнёров Moodle | Moodle |
| `discourse` — Discourse | `stateless` | Свои серверы ★, Хостинг Discourse от разработчиков | Discourse |
| `matomo` — Matomo | `stateless` | Свои серверы ★, Matomo Cloud | Matomo |
| `open_edx` — Open edX | `stateless` | — | — |
| `mastodon` — Mastodon | `stateless` | — | — |

**Видео и потоковое вещание**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `wowza_streaming_engine` — Wowza Streaming Engine | `stateless` | Origin и edge на своих серверах ★, Wowza Video | — |
| `nginx_rtmp` — Модуль NGINX RTMP | `stateless` | — | — |
| `janus_gateway` — Janus WebRTC Server | `stateless` | — | — |
| `livekit` — LiveKit | `stateless` | Свои узлы ★, LiveKit Cloud | — |
| `red5` — Red5 / Red5 Pro | `stateless` | Свои серверы ★, Red5 Cloud | — |
| `ant_media_server` — Ant Media Server | `stateless` | — | — |
| `mediamtx` — MediaMTX | `stateless` | — | — |
| `flussonic_media_server` — Flussonic Media Server | `stateless` | — | — |

**IoT и промышленная автоматизация**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `thingsboard` — ThingsBoard | `stateless` | Свой кластер или монолит ★, ThingsBoard Cloud | — |
| `node_red` — Node-RED | `stateless` | — | — |
| `chirpstack` — ChirpStack | `stateless` | — | — |
| `aws_iot_core` — AWS IoT Core | `stateless` | — | — |
| `azure_iot_hub` — Azure IoT Hub | `stateless` | — | — |

### ИИ и машинное обучение

**Платформы ML и MLOps**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kubeflow` — Kubeflow | `stateless` | — | — |
| `mlflow` — MLflow | `stateless` | Свои серверы отслеживания ★, Управляемый MLflow (Databricks, Amazon SageMaker AI) | — |
| `clearml` — ClearML | `stateless` | Свой сервер ClearML ★, ClearML Hosted Service | — |
| `langfuse` — Langfuse | `stateless` | Helm-чарт в Kubernetes или облаке ★, Docker Compose (одна ВМ), Langfuse Cloud | — |
| `red_hat_openshift_ai` — Red Hat OpenShift AI | `stateless` | — | — |
| `amazon_sagemaker` — Amazon SageMaker AI | `stateless` | — | — |
| `google_vertex_ai` — Google Vertex AI (Gemini Enterprise Agent Platform) | `stateless` | — | — |
| `azure_machine_learning` — Azure Machine Learning | `stateless` | — | — |
| `yandex_datasphere` — Yandex DataSphere | `stateless` | — | — |
| `hugging_face_hub` — Hugging Face Hub | `stateless` | — | — |

**Обслуживание моделей**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `kserve` — KServe | `stateless` | Обычные Deployment (RawDeployment) ★, Knative (serverless) | — |
| `seldon_core` — Seldon Core | `stateless` | — | — |
| `bentoml` — BentoML | `stateless` | Свои контейнеры ★, BentoCloud | — |
| `nvidia_triton` — NVIDIA Dynamo-Triton (Triton Inference Server) | `stateless` | — | — |
| `torchserve` — TorchServe | `stateless` | — | — |
| `ray_serve` — Ray Serve | `stateless` | — | — |
| `tensorflow_serving` — TensorFlow Serving | `stateless` | — | — |
| `openvino_model_server` — OpenVINO Model Server | `stateless` | — | — |

**Запуск LLM и шлюзы**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `vllm` — vLLM | `stateless` | — | — |
| `sglang` — SGLang | `stateless` | — | — |
| `text_generation_inference` — Hugging Face Text Generation Inference | `stateless` | — | — |
| `ollama` — Ollama | `stateless` | — | — |
| `llama_cpp_server` — llama.cpp server | `stateless` | — | — |
| `localai` — LocalAI | `stateless` | — | — |
| `nvidia_nim` — NVIDIA NIM | `stateless` | — | — |
| `nvidia_dynamo` — NVIDIA Dynamo | `stateless` | — | — |
| `litellm` — LiteLLM | `stateless` | — | — |
| `open_webui` — Open WebUI | `stateless` | Один контейнер (SQLite) ★, Несколько реплик | — |
| `amazon_bedrock` — Amazon Bedrock | `stateless` | — | — |
| `azure_openai` — Azure OpenAI | `stateless` | — | — |
| `openai_api` — OpenAI API | `stateless` | — | — |
| `anthropic_api` — Claude API (Anthropic) | `stateless` | — | — |
| `yandex_ai_studio` — Yandex AI Studio (YandexGPT) | `stateless` | — | — |
| `gigachat_api` — GigaChat API (Сбер) | `stateless` | — | — |

**Ноутбуки и среды разработки**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `jupyterhub` — JupyterHub | `stateless` | Zero to JupyterHub (Kubernetes) ★, The Littlest JupyterHub (один сервер) | — |
| `posit_workbench` — Posit Workbench (RStudio Server) | `stateless` | Один узел (RStudio Server или Workbench) ★, Несколько узлов с балансировкой (Workbench) | — |
| `apache_zeppelin` — Apache Zeppelin | `stateless` | — | — |
| `code_server` — code-server | `stateless` | — | — |

**Feature store**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `feast` — Feast | `stateless` | — | — |
| `hopsworks` — Hopsworks | `stateless` | Свой кластер (Kubernetes) ★, Hopsworks Serverless (SaaS) | — |

**GPU и планирование для ML**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `nvidia_gpu_operator` — NVIDIA GPU Operator | `stateless` | — | — |
| `run_ai` — NVIDIA Run:ai | `stateless` | Свой управляющий слой ★, Управляющий слой как сервис (SaaS) | — |
| `kai_scheduler` — NVIDIA KAI Scheduler | `stateless` | — | — |

**Разметка данных**

| Вид | Поведение | Варианты | Версии |
|---|---|---|---|
| `label_studio` — Label Studio | `stateless` | Свой экземпляр ★, Облачная версия HumanSignal | — |
| `cvat` — CVAT | `stateless` | Свой экземпляр ★, CVAT Online | — |
<!-- END GENERATED: catalog -->

## Таблицы выпусков по умолчанию

<!-- BEGIN GENERATED: versions -->
Таблиц: **277**, данные на **2026-10-26** (самая старая — 2026-10-26). «LTS» — есть линия с долгой поддержкой.

| Таблица | Продукт | Линий | Новейшая линия | LTS |
|---|---|---|---|---|
| `activemq` | Apache ActiveMQ Classic | 8 | `6.3` | — |
| `airflow` | Apache Airflow | 5 | `3.3` | — |
| `alibaba_ack` | Alibaba ACK | 14 | `1.34` | — |
| `almalinux` | AlmaLinux | 3 | `10` | — |
| `alpine` | Alpine Linux | 16 | `3.24` | — |
| `amazon-aurora-mysql` | Amazon Aurora MySQL | 2 | `8.4` | — |
| `amazon-aurora-postgresql` | Amazon Aurora PostgreSQL | 7 | `18` | — |
| `amazon-documentdb` | Amazon DocumentDB | 4 | `8.0` | да |
| `amazon-eks` | Amazon EKS | 20 | `1.37` | — |
| `amazon-elasticache-redis` | Amazon ElastiCache for Redis | 2 | `7` | — |
| `amazon-linux` | Amazon Linux | 1 | `2023` | — |
| `amazon-mq-activemq` | Amazon MQ for ActiveMQ | 4 | `5.19` | — |
| `amazon-mq-rabbitmq` | Amazon MQ for RabbitMQ | 4 | `4.3` | — |
| `amazon-msk` | Amazon MSK | 19 | `4.2` | — |
| `amazon-opensearch` | Amazon OpenSearch Service | 17 | `3.7` | — |
| `amazon-rds-mariadb` | Amazon RDS for MariaDB | 7 | `12.3` | — |
| `amazon-rds-mysql` | Amazon RDS for MySQL | 15 | `8.4.9` | — |
| `amazon-rds-postgresql` | Amazon RDS for PostgreSQL | 8 | `18` | — |
| `amazon_elasticsearch` | Amazon Elasticsearch | 10 | `7.10` | — |
| `amazon_glue` | Amazon Glue | 9 | `6.0` | — |
| `amazon_neptune` | Amazon Neptune | 39 | `1.5.0.0` | — |
| `ansible` | Ansible | 14 | `14` | — |
| `ansible_core` | Ansible-core | 13 | `2.21` | — |
| `apache` | Apache HTTP Server | 1 | `2.4` | — |
| `apache_camel` | Apache Camel | 48 | `4.23` | да |
| `apache_hadoop` | Apache Hadoop | 5 | `3.5` | — |
| `apache_http_server` | Apache HTTP Server | 1 | `2.4` | — |
| `apache_subversion` | Apache Subversion | 5 | `1.15` | да |
| `apisix` | Apache APISIX | 26 | `3.19` | да |
| `arangodb` | ArangoDB | 9 | `3.12` | — |
| `argo_cd` | Argo CD | 30 | `3.5` | — |
| `argo_workflows` | Argo Workflows | 7 | `4.1` | — |
| `artemis` | Apache ActiveMQ Artemis | 46 | `2.57` | — |
| `artifactory` | JFrog Artifactory | 30 | `7.161` | — |
| `authentik` | authentik | 12 | `2026.8` | — |
| `aws_lambda` | AWS Lambda | 42 | `java8.al2023` | — |
| `azure-database-for-mysql` | Azure Database for MySQL | 2 | `8.4` | — |
| `azure-database-for-postgresql` | Azure Database for PostgreSQL | 8 | `18` | — |
| `azure-kubernetes-service` | Azure Kubernetes Service | 16 | `1.36` | — |
| `azure_devops_server` | Azure DevOps Server | 10 | `continuous` | — |
| `bamboo` | Bamboo | 22 | `12.1` | да |
| `beats` | Elastic Beats | 11 | `9.5` | — |
| `big_ip` | BIG-IP | 10 | `21.1` | да |
| `bigbluebutton` | BigBlueButton | 4 | `3.0` | — |
| `bind` | BIND 9 | 4 | `9.21` | да |
| `bitbucket` | Bitbucket | 67 | `10.5` | да |
| `boundary` | HashiCorp Boundary | 6 | `0.21` | — |
| `caddy` | Caddy | 2 | `2` | — |
| `calico` | Calico | 9 | `3.33` | — |
| `cassandra` | Apache Cassandra | 3 | `5.0` | — |
| `centreon` | Centreon | 14 | `26.10` | — |
| `ceph` | Ceph | 7 | `20` | — |
| `cert_manager` | cert-manager | 12 | `1.21` | да |
| `cfengine` | CFEngine | 14 | `3.28` | да |
| `checkmk` | Checkmk | 6 | `2.5` | — |
| `chef_infra_client` | Chef Infra Client | 5 | `19` | — |
| `chef_infra_server` | Chef Infra Server | 3 | `15` | — |
| `chef_inspec` | Chef InSpec | 4 | `7` | — |
| `chef_supermarket` | Chef Supermarket | 2 | `5` | — |
| `cilium` | Cilium | 8 | `1.20` | — |
| `cisco_ios_xe` | Cisco IOS XE | 20 | `26.1` | да |
| `citrix_vad` | Citrix Virtual Apps and Desktops | 25 | `2607` | да |
| `clamav` | ClamAV | 11 | `1.5` | да |
| `clickhouse` | ClickHouse | 21 | `26.9` | да |
| `cockroachdb` | CockroachDB | 21 | `26.3` | да |
| `coder` | Coder | 32 | `2.38` | да |
| `coldfusion` | Adobe ColdFusion | 3 | `2025` | — |
| `commvault` | Commvault | 7 | `11.46` | да |
| `confluence` | Confluence | 44 | `11.0` | да |
| `consul` | HashiCorp Consul | 18 | `2.0` | да |
| `contour` | Contour | 10 | `1.33` | — |
| `controlm` | Control-M | 4 | `9.0.22` | — |
| `cortex_xdr` | Palo Alto Networks Cortex XDR agent | 30 | `9.3` | да |
| `couchbase` | Couchbase Server | 8 | `8.0` | — |
| `couchdb` | Apache CouchDB | 4 | `3.5` | — |
| `craft_cms` | Craft CMS | 2 | `5` | — |
| `dapr` | Dapr | 11 | `1.18` | — |
| `dbt_core` | dbt Core | 14 | `2.0` | — |
| `dce` | DaoCloud Enterprise | 2 | `5.0` | — |
| `debian` | Debian | 4 | `13` | — |
| `dependency_track` | Dependency-Track | 10 | `5.1` | — |
| `discourse` | Discourse | 13 | `2026.9` | да |
| `django` | Django | 12 | `6.1` | да |
| `docker_engine` | Docker Engine | 11 | `29` | — |
| `dotnet` | .NET | 9 | `10` | да |
| `dovecot` | Dovecot | 2 | `2.4` | — |
| `drupal` | Drupal | 20 | `11.4` | — |
| `duckdb` | DuckDB | 7 | `2.0` | да |
| `eclipse-temurin` | Eclipse Temurin (Java) | 14 | `27` | да |
| `eclipse_jetty` | Eclipse Jetty | 4 | `12.1` | — |
| `elasticsearch` | Elasticsearch | 11 | `9.5` | — |
| `envoy` | Envoy | 32 | `1.40` | — |
| `esxi` | VMware ESXi | 4 | `9.1` | — |
| `etcd` | etcd | 4 | `3.7` | — |
| `exim` | Exim | 9 | `4.100` | — |
| `external_dns` | ExternalDNS | 1 | `0` | — |
| `flink` | Apache Flink | 8 | `2.3` | да |
| `fluent_bit` | Fluent Bit | 21 | `5.1` | — |
| `flux` | Flux | 11 | `2.9` | — |
| `foreman` | Foreman | 32 | `5.0` | — |
| `forgejo` | Forgejo | 11 | `17.0` | да |
| `fortigate` | FortiOS (FortiGate) | 7 | `8.0` | — |
| `foundationdb` | FoundationDB | 3 | `7.3` | — |
| `freebsd` | FreeBSD | 25 | `14.5` | — |
| `gatekeeper` | OPA Gatekeeper | 13 | `3.23` | — |
| `gerrit` | Gerrit | 16 | `3.14` | — |
| `gitlab` | GitLab | 68 | `19.5` | — |
| `gitlab_runner` | GitLab Runner | 18 | `19.5` | — |
| `go` | Go | 16 | `1.27` | — |
| `google-kubernetes-engine` | Google Kubernetes Engine | 20 | `1.36` | — |
| `grafana` | Grafana | 29 | `13.2` | — |
| `graylog` | Graylog | 18 | `7.1` | — |
| `greenlight` | Greenlight | 1 | `3` | — |
| `haproxy` | HAProxy | 16 | `3.4` | да |
| `haproxy_ingress` | HAProxy Ingress | 5 | `0.16` | — |
| `harbor` | Harbor | 18 | `2.15` | — |
| `hashicorp_packer` | Hashicorp Packer | 9 | `1.16` | — |
| `hbase` | Apache HBase | 7 | `3.0` | — |
| `heroku_postgres` | Heroku Postgres | 8 | `18` | — |
| `horizon` | Omnissa Horizon | 22 | `8.2606` | да |
| `ibm_aix` | IBM AIX | 7 | `7.3.4` | — |
| `ibm_db2` | IBM Db2 | 2 | `12.1` | — |
| `ibm_i` | IBM iSeries | 3 | `7.6` | — |
| `ibm_mq` | IBM MQ | 4 | `10.0` | — |
| `icinga` | Icinga | 6 | `2.16` | — |
| `icinga_web` | Icinga Web | 8 | `2.14` | — |
| `influxdb` | InfluxDB | 15 | `3.12` | — |
| `istio` | Istio | 25 | `1.31` | — |
| `jaeger` | Jaeger | 1 | `2` | — |
| `jenkins` | Jenkins | 19 | `2.568` | да |
| `jfrog_xray` | JFrog Xray | 36 | `3.150` | — |
| `jira_service_management` | Jira Service Management | 54 | `11.3` | да |
| `jira_software` | Jira Software | 54 | `11.3` | да |
| `joomla` | Joomla! | 3 | `6` | — |
| `kafka` | Apache Kafka | 22 | `4.3` | — |
| `karpenter` | Karpenter | 16 | `1.15` | — |
| `keda` | KEDA | 23 | `2.21` | — |
| `keycloak` | Keycloak | 28 | `26.8` | — |
| `kiali` | Kiali | 34 | `2.31` | да |
| `kibana` | Kibana | 13 | `9.5` | — |
| `knative` | Knative | 38 | `1.23` | — |
| `kong_gateway` | Kong Gateway | 11 | `3.9` | да |
| `kubernetes` | Kubernetes | 22 | `1.37` | — |
| `kuma` | Kuma | 23 | `3.0` | да |
| `kyverno` | Kyverno | 12 | `1.19` | — |
| `laravel` | Laravel | 9 | `13` | да |
| `liquibase` | Liquibase | 2 | `5` | — |
| `logstash` | Logstash | 10 | `9.5` | — |
| `loki` | Grafana Loki | 25 | `3.7` | — |
| `longhorn` | Longhorn | 13 | `1.13` | — |
| `looker` | Looker | 69 | `26.22` | да |
| `magento` | Magento | 11 | `2.4.9` | — |
| `mariadb` | MariaDB | 22 | `13.0` | да |
| `matomo` | Matomo | 2 | `5` | — |
| `mattermost` | Mattermost | 58 | `12.0` | да |
| `mautic` | Mautic | 18 | `7.2` | да |
| `mediawiki` | MediaWiki | 15 | `1.46` | да |
| `meilisearch` | Meilisearch | 56 | `1.55` | — |
| `memcached` | Memcached | 1 | `1.6` | — |
| `metabase` | Metabase | 34 | `0.64` | да |
| `metallb` | MetalLB | 4 | `0.16` | — |
| `micronaut` | Micronaut Framework | 4 | `5` | — |
| `mongodb` | MongoDB | 19 | `9.0` | — |
| `moodle` | Moodle | 14 | `5.3` | да |
| `msexchange` | Microsoft Exchange | 1 | `subscription` | — |
| `mssqlserver` | Microsoft SQL Server | 7 | `17.0` | — |
| `mulesoft_runtime` | Mule Runtime | 13 | `4.12` | да |
| `mysql` | MySQL | 13 | `26.7` | да |
| `neo4j` | Neo4j | 53 | `2026.09` | да |
| `netapp-ontap` | NetApp ONTAP | 15 | `9.19.1` | — |
| `nextcloud` | Nextcloud | 21 | `35` | — |
| `nextjs` | Next.js | 8 | `16` | да |
| `nexus` | Sonatype Nexus Repository | 69 | `3.97` | — |
| `nginx` | NGINX | 16 | `1.31` | — |
| `nifi` | Apache NiFi | 23 | `2.12` | — |
| `nix` | nix | 34 | `2.35` | — |
| `nodejs` | Node.js | 15 | `26` | да |
| `nomad` | HashiCorp Nomad | 14 | `2.0` | да |
| `nutanix-aos` | Nutanix AOS | 19 | `7.6` | да |
| `nutanix_files` | Nutanix Files | 14 | `5.4` | — |
| `nutanix_prism` | Nutanix Prism Central | 27 | `pc.7.6` | — |
| `openbao` | OpenBao | 8 | `2.7` | — |
| `opensearch` | OpenSearch | 3 | `3` | — |
| `openshift` | Red Hat OpenShift | 22 | `4.22` | — |
| `opentofu` | OpenTofu | 8 | `1.13` | — |
| `openvpn` | OpenVPN | 3 | `2.7` | — |
| `openwrt` | OpenWrt | 6 | `25.12` | — |
| `openzfs` | OpenZFS | 6 | `2.4` | — |
| `opnsense` | OPNsense | 16 | `26.7` | — |
| `oracle-linux` | Oracle Linux | 3 | `10` | — |
| `oracle_database` | Oracle Database | 3 | `23` | да |
| `oracle_solaris` | Oracle Solaris | 1 | `11.4` | — |
| `otobo` | OTOBO | 3 | `11.0` | — |
| `ovirt` | oVirt | 3 | `4.5` | — |
| `palo_alto` | PAN-OS (Palo Alto Networks) | 11 | `12.2` | — |
| `pangp` | Palo Alto Networks GlobalProtect App | 8 | `6.3` | — |
| `phoenix_framework` | Phoenix Framework | 5 | `1.8` | — |
| `photon` | VMware Photon | 3 | `5.0` | — |
| `php` | PHP | 8 | `8.5` | — |
| `pigeonhole` | Pigeonhole | 2 | `2.4` | — |
| `plesk` | Plesk | 33 | `18.0.81` | — |
| `podman` | Podman | 26 | `6.1` | — |
| `postfix` | Postfix | 8 | `3.11` | — |
| `postgresql` | PostgreSQL | 7 | `18` | — |
| `proftpd` | ProFTPD | 3 | `1.3.9` | — |
| `prometheus` | Prometheus | 36 | `3.15` | да |
| `proxmox_backup` | Proxmox Backup Server | 4 | `4` | — |
| `proxmox_datacenter_manager` | Proxmox Datacenter Manager | 2 | `1` | — |
| `proxmox_mail_gateway` | Proxmox Mail Gateway | 4 | `9` | — |
| `proxmox_ve` | Proxmox VE | 4 | `9` | — |
| `pulsar` | Apache Pulsar | 15 | `5.0` | да |
| `puppet` | Puppet | 2 | `8` | — |
| `python` | Python | 8 | `3.15` | — |
| `quarkus_framework` | Quarkus | 60 | `3.40` | да |
| `rabbitmq` | RabbitMQ | 10 | `4.3` | — |
| `rails` | Ruby on Rails | 7 | `8.1` | — |
| `rancher` | Rancher | 14 | `2.15` | — |
| `red_hat_ansible_automation_platform` | Red Hat Ansible Automation Platform | 4 | `2.6` | — |
| `redhat_jboss_eap` | Red Hat JBoss Enterprise Application Platform | 1 | `8` | — |
| `redhat_satellite` | Red Hat Satellite | 15 | `6.19` | — |
| `redis` | Redis | 11 | `8.10` | — |
| `redmine` | Redmine | 8 | `7.0` | — |
| `rhel` | Red Hat Enterprise Linux | 3 | `10` | да |
| `rocket_chat` | Rocket.Chat | 80 | `8.9` | да |
| `rocky-linux` | Rocky Linux | 3 | `10` | — |
| `roundcube` | Roundcube Webmail | 4 | `1.7` | да |
| `routeros` | RouterOS | 30 | `7.24` | да |
| `rtpengine` | rtpengine | 26 | `26.2` | да |
| `ruby` | Ruby | 8 | `4.0` | — |
| `salt` | Salt | 10 | `3008` | да |
| `samba` | Samba | 16 | `4.25` | — |
| `sharepoint` | Microsoft SharePoint | 1 | `subscription` | — |
| `shopware` | Shopware | 9 | `6.7` | — |
| `sles` | SUSE Linux Enterprise | 10 | `16.0` | — |
| `solr` | Apache Solr | 3 | `10` | — |
| `sonarqube` | SonarQube | 14 | `2026.5` | да |
| `sonarqube_community` | SonarQube Community Build | 15 | `26.10` | — |
| `sourcegraph` | Sourcegraph | 5 | `8` | — |
| `spark` | Apache Spark | 10 | `4.2` | да |
| `splunk` | Splunk | 12 | `10.4` | — |
| `spring-boot` | Spring Boot | 15 | `4.1` | — |
| `spring_framework` | Spring Framework | 6 | `7.0` | да |
| `sqlite` | SQLite | 1 | `3` | — |
| `squid` | Squid | 3 | `7` | — |
| `suse_manager` | SUSE Multi-Linux Manager | 7 | `5.2` | да |
| `symfony` | Symfony | 20 | `8.1` | да |
| `tarantool` | Tarantool | 18 | `3.8` | да |
| `telegraf` | Telegraf | 32 | `1.40` | — |
| `teleport` | Teleport | 3 | `18` | — |
| `terraform` | Hashicorp Terraform | 17 | `1.16` | — |
| `tomcat` | Apache Tomcat | 4 | `11.0` | — |
| `traefik` | Traefik | 20 | `3.7` | — |
| `truenas` | TrueNAS | 5 | `25.10` | — |
| `typo3` | TYPO3 | 5 | `14` | да |
| `ubuntu` | Ubuntu | 16 | `26.10` | да |
| `umbraco` | Umbraco CMS | 11 | `18` | да |
| `valkey` | Valkey | 5 | `9.1` | — |
| `varnish` | Varnish Cache | 12 | `9.1` | да |
| `vault` | HashiCorp Vault | 15 | `2.1` | да |
| `vcenter` | VMware vCenter Server | 4 | `9.1` | — |
| `veeam_backup` | Veeam Backup & Replication | 4 | `13` | — |
| `veeam_backup_for_microsoft_365` | Veeam Backup for Microsoft 365 | 6 | `8` | — |
| `veeam_one` | Veeam ONE | 4 | `13` | — |
| `vinyl_cache` | Vinyl Cache | 12 | `9.1` | да |
| `virtualbox` | VirtualBox | 5 | `7.2` | — |
| `vitess` | Vitess | 12 | `24` | — |
| `vmware_cloud_foundation` | VMware Cloud Foundation | 15 | `9.1` | — |
| `vmware_harbor_registry` | VMware Harbor Registry | 13 | `2.10` | — |
| `vmware_srm` | VMware Site Recovery Manager | 9 | `9.1` | — |
| `windows-server` | Windows Server | 10 | `2025` | да |
| `wordpress` | WordPress | 25 | `7.1` | — |
| `xcp_ng` | XCP-ng | 5 | `8.3` | да |
| `youtrack` | YouTrack | 24 | `2026.2` | — |
| `zabbix` | Zabbix | 8 | `7.4` | да |
| `zentyal` | Zentyal | 3 | `8` | — |
| `zerto` | HPE Zerto | 18 | `10.9` | — |
| `zookeeper` | Apache ZooKeeper | 5 | `3.9` | — |
<!-- END GENERATED: versions -->

## Подводные камни

| Камень | Решение |
|---|---|
| Версия «16» из каталога давно закончила поддержку | дата — аргумент движка; таблицы обновляются кнопкой, расписанием или из CI; устаревшая таблица сама превращает вердикт в «нет данных» |
| Таблицы не получилось скачать (закрытая сеть, источник упал) | поставляемые таблицы работают офлайн; `sourceBases` указывает на внутреннее зеркало; неудача одной таблицы не трогает остальные |
| Админ подменил `source` на внутренний адрес (SSRF), или разрешённый хост ответил перенаправлением на него | только префиксы из `versions.sourceBases`, без логина и пароля в адресе; каждое перенаправление сверяется с ними заново, не более 5 |
| Реплики обновляют таблицы одновременно и спамят источник | слот обновления захватывается записью в `meta`; свежая таблица не перезаписывается старой при посеве |
| Управляемый сервис наследовал «сообщественные» даты PostgreSQL | у управляемого варианта своя таблица (RDS, Aurora, Azure) или пустая (`""`): чужие даты не подставляются |
| RDS или MSK помечены «split-brain: нет арбитра» | признак `managed` → власть `Provider`: провайдер сам держит кворум, изолированный primary останавливается |
| Redis Sentinel / Cluster требует etcd | признак `self-quorum`: голоса считаются внутри юнита |
| Выбор варианта затирает ваши настройки | значения по умолчанию варианта действуют только там, где свойство не задано; заданное вручную остаётся |
| ИИ придумал вариант, которого нет | `unknown_variant` при валидации; в промпте перечислены только существующие `id` |
| Новая версия продукта вышла вчера и ещё сырая | рекомендуется линия, которой больше `matureDays` дней; LTS предпочитается |
| Продукту нечего показывать в таблице версий | `versions: ""` явно отключает проверку; `VER-01` не создаёт шум там, где данных нет |
