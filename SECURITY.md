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

Argon2id для паролей, блокировка после серии неудачных входов и лимит попыток по IP, подписанные HttpOnly-сессии, проверка `Origin` для изменяющих запросов, строгий CSP без inline-скриптов,
роли `viewer < editor < admin`, секреты в базе шифруются AES-256-GCM и не возвращаются API и экспорту, поставляемые compose- и Kubernetes-конфигурации запускают контейнер без привилегий, с read-only корневой ФС,
зависимости проверяются на допустимые лицензии (`deploy/licenses.py`). Подробности — [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) и [docs/PITFALLS.md](docs/PITFALLS.md).

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
AES-256-GCM encrypted secrets that are never returned by the API or the export, the shipped compose and Kubernetes configurations run the container unprivileged with a read-only root filesystem, and a license check of every dependency (`deploy/licenses.py`).
