# Как внести вклад

Спасибо, что хотите улучшить Infra Architect. Здесь — как быстро запустить проект, какие правила у кодовой базы и что проверяется до слияния.
*English summary is at the end of this file.*

Проект открыт по лицензии [Apache-2.0](LICENSE). Отправляя вклад (issue, pull request, правку каталога), вы соглашаетесь, что он распространяется на тех же условиях
(раздел 5 лицензии); отдельное соглашение подписывать не нужно.

## С чего начать

| Хочу… | Куда |
|---|---|
| сообщить об ошибке | [issue «Ошибка»](../../issues/new/choose): что делали, что ожидали, что получили, версия (**Помощь → О программе**) |
| поправить факт о продукте (HA, версия, вариант, функция) | [issue «Исправление каталога»](../../issues/new/choose) или сразу pull request в `deploy/catalog/` |
| предложить возможность | [issue «Идея»](../../issues/new/choose): какую задачу архитектора это решает |
| сообщить об уязвимости | **не** issue: [SECURITY.md](SECURITY.md) |
| поправить код или документацию | pull request, см. ниже |

## Запуск

Всё собирается и проверяется в контейнерах — на хост ничего ставить не нужно:

```bash
make up              # приложение + PostgreSQL: http://localhost:8080
make test            # cargo test --workspace (с настоящим PostgreSQL) + typecheck, vitest и сборка фронта
make lint            # rustfmt + clippy -D warnings
make iac-check       # IaC эталонных архитектур против схем Kubernetes/CRD, Helm и Terraform
```

Без Docker — Rust 1.89+ (stable), Node 22, PostgreSQL 14+: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) (запуск, тесты, браузерные проверки, «как добавить правило / компонент / настройку»).

## Правила кодовой базы

Они не формальность — на них держится качество продукта:

* **Никаких захардкоженных чисел и текстов.** Тайминги, пороги, лимиты, модели, промпты, правила, каталог, шаблоны — данные: настройки (`backend/crates/server/defaults/settings.json`),
  правила (`rules.json`), каталог (`deploy/catalog/`), шаблоны. Новый параметр — это запись в реестре с подписью и описанием на двух языках, а не константа.
* **Никаких моков и заглушек.** Тесты ходят в настоящий PostgreSQL, настоящий движок и настоящий браузер; ответы внешних сервисов — записанные живые прогоны
  ([docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#тесты-без-моков-и-заглушек)). Экспорт IaC — настоящие манифесты, а не «TODO».
* **Движок детерминирован.** Модель ИИ только *предлагает*; всё, что выглядит как факт об архитектуре (находка, RTO, доступность), считает Rust-движок без обращений к сети и часам.
* **Каталог собирается из исходников.** Продукты правят в `deploy/catalog/*.py` и собирают `python3 deploy/catalog/build.py`; `catalog.json` руками не правят
  ([README каталога](deploy/catalog/README.md)). Неверное утверждение об отказоустойчивости хуже отсутствующего продукта: пишите только то, в чём уверены.
* **Два языка.** Каждый видимый текст — `ru` и `en`.
* **Документация — часть изменения.** Таблицы в `docs/` генерируются (`python3 deploy/gen-docs.py`), остальное правится вместе с кодом в том же коммите.
* **Интерфейс — продукт.** Цвета и размеры — токены (`frontend/src/styles/tokens.css`), движение подчиняется режиму анимаций, всё доступно с клавиатуры (проверяет axe-core).

## Перед pull request

```bash
cd backend  && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
cd frontend && npm run typecheck && npm test && npm run build      # браузерные проверки: docs/DEVELOPMENT.md, «Проверка в браузере»
python3 deploy/catalog/build.py --check && python3 deploy/gen-docs.py --check && python3 deploy/licenses.py --check
```

* Добавили или убрали зависимость, или у неё сменилась лицензия — выполните `python3 deploy/licenses.py`: он перепишет `THIRD_PARTY_LICENSES.md` и откажет в лицензии, которую проект не принимает
  (`deploy/licenses.json`). Обычное обновление версии список не меняет: в нём нет версий (они в `Cargo.lock` и `package-lock.json`; `python3 deploy/licenses.py --versions` их печатает).
* Новая логика — с тестом на реальных данных; ошибка, которую вы исправили, — с тестом, который на ней падал.
* Один pull request — одна мысль. В описании — зачем (какую проблему архитектора решает), а не только что.
* Сообщения коммитов — по-английски или по-русски, в повелительном или описательном стиле, главное — ясно, *почему*.

## Кодекс поведения

Участвуя, вы соглашаетесь с [Кодексом поведения](CODE_OF_CONDUCT.md).

---

## English summary

Infra Architect is open source under the [Apache License 2.0](LICENSE); contributions are accepted under the same terms (section 5), no separate agreement needed.

* **Run:** `make up` (app + PostgreSQL on :8080), `make test`, `make lint`, `make iac-check`; without Docker you need Rust 1.89+ (stable), Node 22 and PostgreSQL 14+ — see [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) (in Russian).
* **House rules:** no hard-coded numbers or texts (timings, thresholds, rules, catalog, templates are data and settings); no mocks or stubs in tests (real PostgreSQL, real engine, recorded live responses); the Rust engine is deterministic and the LLM only proposes;
  the product catalog is built from `deploy/catalog/` sources (never edit `catalog.json` by hand, and only state what you are sure of about a product's failure behaviour); every visible text has `ru` and `en`; docs change in the same commit.
* **Before a pull request:** `cargo fmt/clippy/test`, `npm run typecheck && npm test && npm run build`, `python3 deploy/catalog/build.py --check`, `python3 deploy/gen-docs.py --check`, `python3 deploy/licenses.py --check` (run it without `--check` after changing dependencies).
* **Security issues:** please do not open a public issue — see [SECURITY.md](SECURITY.md).
