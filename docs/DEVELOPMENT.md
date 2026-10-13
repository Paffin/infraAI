# Разработка

Всё запускается в контейнерах, ничего ставить на хост не нужно:

```bash
make test            # cargo test --workspace (с PostgreSQL) + typecheck/vitest/build фронтенда
make up              # приложение + БД: http://localhost:8080
make iac-check       # IaC эталонных архитектур против схем Kubernetes/CRD, Helm и Terraform (см. IAC.md)
```

Без Docker: Rust (stable), Node 22, PostgreSQL 14+.

## Бэкенд

```bash
cd backend
export TEST_DATABASE_ADMIN_URL=postgres://postgres@localhost:5432/postgres   # для интеграционных тестов: каждому тесту — своя БД
cargo test --workspace
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
```

Запуск сервера для разработки:

```bash
export APP_ENV=development DATABASE_URL=postgres://postgres@localhost:5432/infra_dev ADMIN_EMAIL=admin@dev.local ADMIN_PASSWORD='dev password 123456'
cargo run -p server          # слушает :8080; без APP_SECRET в development используется временный
```

Структура: `crates/engine` — чистая логика (без I/O), `crates/server` — HTTP, БД, ИИ.

### Тесты без моков и заглушек

В тестах нет двойников внешних сервисов:

* **чистые функции** — формирование запроса к OpenRouter (`build_body`), разбор ответа (`interpret`), разбор и проверка ответа модели
  (`parse_generate`, `parse_patch`) тестируются на **настоящих** данных: ответы OpenRouter и модели записаны живыми прогонами
  (`crates/server/tests/fixtures/llm`, запись — `LIVE_LLM_RECORD=<каталог>`);
* **интеграционные тесты** (`crates/server/tests/api.rs`) поднимают настоящий роутер на настоящем PostgreSQL (своя БД на тест): ошибки сети,
  бюджет, лимиты, предложения (просмотр, применение, устаревание, откат) — без обращения к модели;
* **живые тесты** (`crates/server/tests/live.rs`, `#[ignore]`) — настоящий OpenRouter: связь и каталог моделей, чат, патч с применением,
  генерация. Стоят центы, идут около минуты:

```bash
OPENROUTER_API_KEY=sk-or-… cargo test -p server --test live -- --ignored --nocapture     # LIVE_LLM_MODEL=… выбирает модель
```

* **IaC** проверяется настоящими инструментами: `make iac-check` ([IAC.md](IAC.md)).

## Фронтенд

```bash
cd frontend
npm ci
API_TARGET=http://localhost:8080 npm run dev      # Vite на :5173 с прокси /api
npm run typecheck && npm test && npm run build
```

* Цвета и размеры — только из `src/styles/tokens.css`; компоненты используют CSS-переменные, hex-значений в компонентах нет.
* Тексты — только через `t()` / `lt()`; ключи в `src/i18n/{ru,en}.ts` (редактор) и `{ru,en}.admin.ts` (админка). Тест
  `tests/i18n-keys.test.ts` проверяет, что все ключи есть в обоих языках и что нет неопределённых.
* Предметная область (виды, свойства, тексты замечаний) приходит из `/bootstrap`, а не из кода.
* **Тесты фронтенда тоже без заглушек.** Чистая логика (`lib/needs`, `lib/objectives`, `lib/capacity`, `lib/diagram`, раскладка) проверяется
  на **настоящем выводе движка**: `tests/fixtures/*.json` — это `{architecture, report}`, записанные примером движка
  (`deploy/record-fixture.sh <шаблон | файл.json> <имя>`), и настоящий каталог из `backend/crates/engine/defaults`
  ([README фикстур](../frontend/tests/fixtures/README.md)). Изменился JSON-ответ движка — тест падает здесь, а не в браузере. Тест
  «каждая потребность (`needs`), которую выдаёт движок, ведёт на редактируемое свойство» ловит рассинхрон каталога и правил.
* Подводные камни интерфейса: глобальные классы `.row`, `.field`, `.empty`, `.kbd` из `styles/` перебивают одноимённые классы
  компонента (называйте свои иначе); клик по карточке должен доходить до обёртки Vue Flow — иначе Shift+клик не добавит узел в выделение
  (`ArchNode.vue`); в `content-visibility: auto` у карточек за экраном нет `innerText` (в тестах — `textContent`).

## Документация живёт вместе с кодом

* Таблицы правил и настроек генерируются: `python3 deploy/gen-docs.py` (в CI — `--check`).
* Меняете API — правьте `docs/API.md` в том же коммите; меняете поведение симулятора/правил — `SIMULATION.md` / `RULES.md`.
* Новое архитектурное решение — короткий ADR в `docs/adr`.

## Как добавить…

**Компонент (вид)** — без кода: Администрирование → Каталог → «Новый элемент» (`catalog.kind`): поведение, признаки (`traits`),
роли, свойства. Чтобы он попал во встроенные значения — добавьте его в `backend/crates/engine/defaults/catalog.json`.

**Правило** — без кода, если подходит один из типов проверок ([RULES.md](RULES.md)): `rules.json` или админ-панель; поля `check`,
`params`, `severity`, тексты на каждом языке и `fixLabels`. Для нового *типа* проверки: вариант в `CheckType`, реализация в `checks.rs`,
тест на эталонной архитектуре в `crates/engine/tests/engine.rs`.

**Эталонную архитектуру** — `defaults/templates/*.json` или админ-панель; шаблон не должен нарушать собственные правила — это проверяет тест
`all_reference_templates_have_no_critical_findings`.

**Настройку** — запись в `crates/server/defaults/settings.json` (ключ, группа, тип, значение по умолчанию, подписи); чтение —
`cfg.settings.str|num|boolean|list(...)`. Форма в админ-панели и таблица в документации появятся сами.

**Язык интерфейса** — словари `src/i18n/<lang>.ts` и `<lang>.admin.ts`, регистрация в `i18n/index.ts`, язык в настройке `app.languages`;
тексты из конфигурации (`LText`) заполняются на новом языке в админ-панели.

## Проверка в браузере

Playwright (Chromium) используется для сквозных проверок интерфейса против запущенного стека (`frontend/e2e`):

```bash
cd frontend && E2E_BASE=http://localhost:8080 E2E_EMAIL=admin@example.com E2E_PASSWORD='…' npx playwright test
```

Наборы: `editor` (редактирование, версии, конфликты, дрейф), `admin`, `export` (отчёт IaC «сгенерировано / не сгенерировано», закрытие
пробела через «Заполнить», SVG-файл), `ux` (что заполнить для «нет данных», цели и план ёмкости, палитра команд, выделение нескольких
узлов, семантический зум, миниатюры и копирование проектов).
