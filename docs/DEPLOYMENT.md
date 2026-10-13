# Развёртывание и эксплуатация

Приложение — один образ (Rust-сервер + собранный фронтенд) и PostgreSQL. Сервер stateless: реплики одинаковы, состояние только в БД.

## Docker Compose (один хост)

```bash
cp .env.example .env     # APP_SECRET (openssl rand -hex 32), POSTGRES_PASSWORD, при желании ADMIN_*
docker compose up -d --build
./deploy/smoke.sh http://localhost:8080 admin@example.com '<пароль>'   # сквозная проверка
```

* Образ собирается многоэтапно: `node` (сборка фронтенда) → `rust` (release-сборка с `--locked`) → `debian:bookworm-slim` без пакетного
  менеджера, пользователь `10001`, корневая ФС только для чтения, `cap_drop: ALL`. Размер ≈ 90 МБ.
* Для публикации в интернет поставьте перед приложением TLS-терминатор (Caddy, nginx, Traefik, облачный LB) и задайте
  `COOKIE_SECURE=true`, `TRUST_PROXY=true`.
* За корпоративным зеркалом образов: `REGISTRY=registry.example.com/mirror/library docker compose build`.
  За TLS-перехватывающим прокси сборка принимает секрет с корневым сертификатом: `docker build --secret id=ca,src=ca.pem .`

### Две реплики на одном хосте

```bash
docker compose -f docker-compose.yml -f docker-compose.ha.yml up -d --build   # make ha
```

Две реплики приложения за nginx (`deploy/nginx/lb.conf`): `/readyz` каждой реплики проверяется Docker-ом, nginx повторяет запрос на
другой реплике при ошибке. Остановка одной реплики незаметна пользователю: сессии подписаны общим `APP_SECRET`, а не живут в памяти.

## Kubernetes

```bash
kubectl apply -f deploy/k8s/secret.example.yaml   # сначала замените значения; в бою — sealed-secrets / external-secrets
kubectl apply -k deploy/k8s
```

| Что | Как устроено в `deploy/k8s` |
|---|---|
| Реплики | 3, `RollingUpdate` с `maxUnavailable: 0`; HPA 3–12 по CPU (70 %) |
| Разнесение | `topologySpreadConstraints` по зонам (`topology.kubernetes.io/zone`) и по узлам: потеря зоны не останавливает сервис |
| Добровольные остановки | `PodDisruptionBudget` `minAvailable: 2` — дренаж узлов не выключит сервис |
| Пробы | `startupProbe` и `livenessProbe` → `/healthz` (процесс жив; **не** смотрит в БД — недоступность БД не должна перезапускать здоровые поды); `readinessProbe` → `/readyz` (БД доступна, под не гасится) |
| Остановка | `preStop: sleep 5`, затем SIGTERM: `/readyz` → 503, `DRAIN_SECONDS` на дренаж, задачи ИИ дорабатывают до `SHUTDOWN_GRACE_SECONDS`; `terminationGracePeriodSeconds: 45` |
| Безопасность | restricted Pod Security, `runAsNonRoot`, `readOnlyRootFilesystem`, `drop: ALL`, `seccomp: RuntimeDefault`, без токена ServiceAccount, `NetworkPolicy` (вход — только ingress-контроллер и мониторинг; выход — DNS, PostgreSQL, HTTPS) |
| Вход | `Ingress` (nginx, TLS), таймаут чтения 130 с под вызовы модели |

В `Deployment` образ — `ghcr.io/paffin/infra-architect:latest` (его публикует CI из ветки `main`); в своём overlay закрепите версию или digest.

### PostgreSQL

Единственное stateful-звено, и именно оно определяет доступность и устойчивость к катастрофе всего сервиса. Нужна PostgreSQL 14+ с
автоматическим переключением и непрерывной архивацией WAL (PITR). Готовый пример на оператор CloudNativePG —
`deploy/k8s/postgres-cnpg.example.yaml`: 3 инстанса в разных зонах, синхронная реплика, WAL и ежедневные базовые копии в S3. Подойдёт и любой
управляемый PostgreSQL с теми же свойствами.

Размер пула: `реплики × DB_MAX_CONNECTIONS` не должно превышать `max_connections` (или поставьте PgBouncer).

## HA и DR самого сервиса

| Отказ | Что происходит | Что делать |
|---|---|---|
| Под/реплика | `readinessProbe` выводит её из ротации; запросы идут на остальные. Её задачи ИИ помечаются `interrupted` через `jobs.staleSeconds` (90 с) — пользователь повторяет запрос | ничего |
| Узел / зона кластера | spread по зонам оставляет реплики в живых зонах, PDB защищает от лишних остановок | ничего |
| Primary PostgreSQL | на время переключения (десятки секунд) `/readyz` = 503, реплики выводятся из ротации, но не перезапускаются; редактор повторяет автосохранение с растущей паузой, правки не теряются | ничего (оператор переключит сам) |
| Потеря БД / региона | все данные продукта — в БД | восстановить PostgreSQL из бэкапа/WAL в резервном регионе, развернуть манифесты с **тем же `APP_SECRET`**, переключить DNS |
| Потерян `APP_SECRET` | сессии недействительны, зашифрованные секреты (ключ OpenRouter) не читаются | сгенерировать новый, войти заново, ввести ключ OpenRouter заново |

Целевые показатели определяются БД: **RPO** = лаг репликации (синхронная реплика → ≈ 0, асинхронное архивирование WAL → интервал
архивации), **RTO** = переключение реплики (секунды) или восстановление из бэкапа + развёртывание (минуты–часы). Проверяйте восстановление
регулярно — недоказанный бэкап приложение само считает риском в проектах пользователей, относитесь так же и к собственному.

Резервные копии: (1) PostgreSQL (PITR); (2) экспорт конфигурации из админ-панели (Бэкап) — удобно для переноса между стендами;
(3) значение `APP_SECRET` в менеджере секретов.

## Обновление

* Образ обновляется скользящим развёртыванием. Миграции БД выполняются **автоматически при старте** первой реплики под advisory-lock;
  остальные ждут. Миграции — только вперёд и аддитивные (расширить → перейти → сжать), поэтому предыдущая версия образа продолжает работать
  со схемой новой на время развёртывания и при откате.
* Встроенные правила, каталог, шаблоны, промпты обновляются вместе с образом, **не затирая ваши изменения** (см. [CONFIGURATION.md](CONFIGURATION.md)).
* Тот же образ проверяет схемы в конвейере: `docker run --rm -v "$PWD:/work:ro" -w /work <образ> check схема.json` — без базы, сети и ключей ([CLI.md](CLI.md)).

## Наблюдаемость

| Что | Где |
|---|---|
| Логи | JSON в stdout (`LOG_JSON`, `RUST_LOG`); у каждого запроса — реплика (`INSTANCE_ID`) |
| Метрики | `/metrics` (Prometheus): `http_requests_total{method,route,status}`, `http_request_duration_seconds`, `engine_evaluations_total`, `ai_jobs_running`, `ai_jobs_finished_total{kind,status}`, `llm_calls_total{purpose,status}`, `llm_cost_usd_total`; отключается настройкой `observability.metricsEnabled`, защищается `METRICS_TOKEN` |
| Здоровье | `/healthz`, `/readyz`; Администрирование → Система (версия, БД, миграции, ревизия конфигурации, счётчики задач) |
| Аудит | Администрирование → Аудит: входы, изменения настроек и конфигурации (какие ключи), пользователи, применения ИИ |

Полезные алерты: доля `5xx` > 1 % за 5 мин; `histogram_quantile(0.95, http_request_duration_seconds) > 2`; `ai_jobs_finished_total{status="failed"}`
растёт; расход `llm_cost_usd_total` за сутки выше ожидаемого; `up == 0` менее чем у двух реплик.

## Исходящие соединения сервера

Сервер сам ходит наружу только в двух случаях, оба — HTTPS (443):

| Куда | Зачем | Как ограничить |
|---|---|---|
| `openrouter.ai` | запросы к ИИ и каталог моделей | не задавайте ключ или выключите `llm.enabled`: всё, кроме ИИ, работает без него |
| `endoflife.date` | сроки поддержки продуктов (таблицы версий): раз в `versions.autoSyncDays` дней и по кнопке в админ-панели | `versions.autoSyncDays = 0` — не обновлять; `versions.sourceBases` — единственные разрешённые адреса (для закрытой сети — внутреннее зеркало); поставляемые таблицы работают офлайн, о старении данных скажет правило `VER-01` |

Сетевая политика в `deploy/k8s/networkpolicy.yaml` пускает HTTPS наружу (кроме частных сетей); в закрытом контуре замените правило на адреса зеркал.

## Чек-лист безопасности

* `APP_SECRET` — случайный, ≥ 32 символов, хранится в менеджере секретов; `ADMIN_PASSWORD` сменён при первом входе.
* TLS на входе, `COOKIE_SECURE=true`, `TRUST_PROXY=true` только за доверенным прокси; `/metrics` закрыт (`METRICS_TOKEN` или сетевая политика).
* `llm.provider.dataCollection = deny` (по умолчанию), при необходимости `zdr`; решите, отправлять ли заметки (`llm.sendNotes`).
* Ограничьте `llm.monthlyBudgetUsd` и `llm.perUserPerHour`; следите за расходом на странице OpenRouter.
* Роли: большинству — `editor`/`viewer`; `admin` — единицы. Блокировка после неудачных входов и лимит по IP включены по умолчанию.
