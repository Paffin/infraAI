# Политика безопасности

*English version is below.*

## Как сообщить об уязвимости

**Не открывайте публичный issue.** Используйте закрытое сообщение GitHub: вкладка **Security → Report a vulnerability** этого репозитория.
Если вкладки нет, напишите владельцу репозитория ([@Paffin](https://github.com/Paffin)) через профиль GitHub, без подробностей, и попросите закрытый канал.

В сообщении укажите:

* что именно уязвимо и чем это грозит (чтение чужих проектов, обход ролей, выполнение кода, утечка секретов, отказ в обслуживании…);
* версию (**Помощь → О программе**) и как запущено приложение (Docker, Kubernetes, `cargo run`);
* шаги воспроизведения или короткий пример — чем проще повторить, тем быстрее исправим.

Что можно ожидать: подтверждение получения — в течение пяти рабочих дней (это цель, а не гарантия: проект ведут люди); затем оценка серьёзности и план исправления.
Мы просим не раскрывать подробности до выхода исправления и по желанию упоминаем автора находки в [CHANGELOG](CHANGELOG.md).

## Какие версии поддерживаются

Пока проект в стадии `0.x`, исправления безопасности выходят только для последнего состояния ветки `main` и последнего образа.

## Что считается уязвимостью

В области: сервер (HTTP API, аутентификация и роли, хранение секретов, конвейер ИИ), веб-приложение (XSS, обход CSP), CLI `infra-architect`, образ Docker и манифесты в `deploy/`.

Вне области: уязвимость стороннего пакета, которую нельзя использовать через Infra Architect (сообщите автору пакета; если можно — напишите и нам, подключим исправление),
ошибки конфигурации развёртывания (слабый `APP_SECRET`, публичный доступ к БД) и перебор паролей грубой силой в обход настроенных лимитов входа.

## Что уже сделано

Argon2id для паролей (вход стоит одинаково и отвечает одинаково для несуществующего, отключённого и существующего аккаунта; хэширование идёт вне потоков, обслуживающих запросы, и не больше заданного числа сразу),
блокировка после серии неудачных входов отдельно для пары «аккаунт, адрес» и с потолком на аккаунт, лимит попыток по IP — оба счётчика в БД, общие для всех реплик, адрес за прокси считается справа (`TRUST_PROXY_HOPS`),
сервер не стартует в production с заготовкой `APP_SECRET` из шаблона, подписанные HttpOnly-сессии, проверка `Origin` для изменяющих запросов, строгий CSP без inline-скриптов,
роли `viewer < editor < admin`, секреты в базе шифруются AES-256-GCM и не возвращаются API и экспорту, поставляемые compose- и Kubernetes-конфигурации запускают контейнер без привилегий, с read-only корневой ФС,
зависимости проверяются на допустимые лицензии (`deploy/licenses.py`) и на известные уязвимости (ниже), `/metrics` можно вынести на отдельный порт (`METRICS_PORT`: в манифестах Kubernetes он не публикуется ни Service, ни Ingress, а сетевая политика пускает на него только мониторинг), токен метрик сравнивается за постоянное время, а метки метрик не растут от того, что присылает клиент,
а `GET /api/admin/system` отдаёт отчёт о безопасности установки (`posture`: что в конфигурации небезопасно по умолчанию, только в production). Подробности — [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) и [docs/PITFALLS.md](docs/PITFALLS.md).

## Зависимости: чем проверяются и что делать с находкой

| Что | Чем | Что проверяется |
|---|---|---|
| крейты Rust, которые линкуются в сервер | `cargo deny` (`backend/deny.toml`, `.github/workflows/security.yml`) | известные уязвимости (база RustSec), снятые автором (yanked) выпуски, источники кроме crates.io; повторяющиеся версии — только в отчёте |
| пакеты npm, которые собирает веб-приложение | `npm audit --omit=dev --audit-level=high` | известные уязвимости высокой и критической серьёзности (инструменты разработки в образ не попадают) |
| пакеты операционной системы в образе (Debian bookworm-slim) | Trivy в задании `image` (`.github/workflows/ci.yml`) | исправленная в Debian уязвимость CRITICAL блокирует `main`, HIGH только показывается |
| лицензии всего перечисленного | `python3 deploy/licenses.py --check` | [CONTRIBUTING.md](CONTRIBUTING.md) |
| сам `cargo deny` | задание `selftest`: замок с уязвимым выпуском обязан его провалить | проверка, которая ничего не находит, не доказывает, что находить нечего |

**Кого что останавливает.** Источники и запреты (`bans`, `sources`) и самопроверка блокируют всегда. Проверки уязвимостей блокируют `main` (а значит, и публикацию образа: `publish` ждёт `security`) и ежедневный запуск (он открывает issue «Security gate failed (daily run)»,
пока красный), а на pull request только **сообщают** — аннотацией и сводкой задания: база уязвимостей меняется без вашего изменения, и pull request не должен краснеть из-за уязвимости, которую он не добавлял (так уже было с Dependabot, [docs/PITFALLS.md](docs/PITFALLS.md)).

**Как судить находку.** Сначала — достижима ли она отсюда, и только потом — насколько она «серьёзна» по оценке. Пример: 14.10.2026 в `rustls` 0.23.45 нашли четыре уязвимости (RUSTSEC-2026-0346…0349). Ни одна отсюда недостижима: `rustls` собран с `ring`, `std`, `tls12`, без `brotli`
(0348), без ECH (0347), без собственного `ClientSessionStore` (0346), а гибридной группы X25519MLKEM768 у провайдера `ring` нет (0349). Но обновление бесплатно (`cd backend && cargo update -p rustls --precise 0.23.46`), поэтому правило такое: **сначала обновить**.
Игнор (`[advisories] ignore` в `backend/deny.toml`) — только для того, чего исправить нельзя и что недостижимо, и только записью `{ id = "RUSTSEC-…", reason = "почему недостижимо… review-by ГГГГ-ММ-ДД" }`:
`deploy/check-deny.py` (в задании `deny`) проваливает запись без причины, без даты или с прошедшей датой, так что игнор — это заём, а не решение. В `.trivyignore` то же делает `exp:ГГГГ-ММ-ДД`, которое Trivy понимает сам. Сейчас не игнорируется ничего.

**Чужие сканеры говорят больше.** OSV, Dependabot и другие читают *текст* `Cargo.lock` и называют пакеты, которых в сборке нет. Так `rsa` 0.9.10 (RUSTSEC-2023-0071) значится в замке только потому, что замок `sqlx` перечисляет `sqlx-mysql`, который не компилируется:
`cargo tree --locked -i rsa` отвечает «nothing to print». `cargo deny` судит собранный граф и молчит; игнор для такого пакета был бы мёртвой записью (cargo-deny сообщит «advisory was not encountered»). Trivy, в свою очередь, не видит крейтов вовсе: бинарник собран без списка зависимостей, замков в образе нет, — их охраняет `cargo deny`.

---

# Security policy

## Reporting a vulnerability

**Please do not open a public issue.** Use GitHub's private reporting: the **Security → Report a vulnerability** tab of this repository.
If there is no such tab, contact the repository owner ([@Paffin](https://github.com/Paffin)) through the GitHub profile, without details, and ask for a private channel.

Include what is vulnerable and what it allows (reading other people's projects, bypassing roles, code execution, leaking secrets, denial of service…), the version (**Help → About**), how it is run (Docker, Kubernetes, `cargo run`)
and steps to reproduce. We aim to acknowledge a report within five working days (a target, not a guarantee), then assess the severity and plan the fix. Please keep the details private until a fix is out; we credit reporters in the [CHANGELOG](CHANGELOG.md) if they wish.

**Supported versions:** while the project is `0.x`, security fixes are made for the current `main` and the latest image only.

**In scope:** the server (HTTP API, authentication and roles, secret storage, the LLM pipeline), the web application (XSS, CSP bypass), the `infra-architect` CLI, the Docker image and the manifests in `deploy/`.
**Out of scope:** vulnerabilities of third-party packages that cannot be exploited through Infra Architect (report them upstream; tell us too and we will take the fix), deployment misconfiguration (a weak `APP_SECRET`, a public database) and brute-forcing past the configured login limits.

**Already in place:** Argon2id passwords, lockout after repeated failures and a per-IP limit, signed HttpOnly sessions, `Origin` checks for state-changing requests, a strict CSP without inline scripts, roles `viewer < editor < admin`,
AES-256-GCM encrypted secrets that are never returned by the API or the export, the shipped compose and Kubernetes configurations run the container unprivileged with a read-only root filesystem, and a license check of every dependency (`deploy/licenses.py`) and a check for known vulnerabilities (below). `/metrics` can be moved to a port of its own (`METRICS_PORT`; in the Kubernetes manifests neither the Service nor the Ingress publishes it, and the network policy admits only monitoring), its token is compared in constant time, its labels do not grow with what a client sends,
and `GET /api/admin/system` returns a report on the security posture of the installation (`posture`: what in the configuration is unsafe by default, in production only).

**Dependencies: what checks them, and what to do with a finding.** The locked Rust crates go through `cargo deny` (`backend/deny.toml`, `.github/workflows/security.yml`: RustSec advisories, yanked releases, no source but crates.io; duplicate versions are only reported),
the npm packages the web application bundles through `npm audit --omit=dev --audit-level=high`, the operating-system packages of the image through Trivy (`image` job of `ci.yml`: a CRITICAL one that Debian has fixed blocks `main`, HIGH is only shown), licenses through `deploy/licenses.py --check`.
A `selftest` job builds a throw-away crate that pins `rustls` 0.23.45 and requires `cargo deny` to fail on it and name RUSTSEC-2026-0346: a check that finds nothing proves nothing until it is seen finding something.
`bans`, `sources` and the self-test always block. The vulnerability checks block `main` (and so the publishing of the image) and the daily run (which opens the issue "Security gate failed (daily run)" while it is red), and only *report* on a pull request, by annotation and job summary:
the databases move without the change, and a pull request must not be red for a vulnerability it did not add.
Judge a finding by whether it can be reached from here, then by its score. The four `rustls` 0.23.45 advisories (RUSTSEC-2026-0346..0349, 14 October 2026) cannot: `rustls` is built with `ring`, `std` and `tls12`, without `brotli`, ECH or a custom `ClientSessionStore`, and `ring` has no X25519MLKEM768.
The update costs nothing (`cd backend && cargo update -p rustls --precise 0.23.46`), so the rule is **update first**. An ignore is for what cannot be fixed and cannot be reached, and it is an entry `{ id = "RUSTSEC-…", reason = "why… review-by YYYY-MM-DD" }`: `deploy/check-deny.py` fails an entry with no reason, no date or a date that has passed
(`.trivyignore` has Trivy's own `exp:YYYY-MM-DD`). Nothing is ignored today. Other scanners (OSV, Dependabot) read the text of `Cargo.lock` and name packages that are not built: `rsa` 0.9.10 (RUSTSEC-2023-0071) is listed only because `sqlx`'s lock lists `sqlx-mysql`, which is never compiled
(`cargo tree --locked -i rsa` prints "nothing to print"), and `cargo deny`, which judges the built graph, is right to stay quiet. Trivy cannot see crates at all (a stripped binary, no lock file in the image): that is `cargo deny`'s job.
