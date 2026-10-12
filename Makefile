# Everything runs in containers; `make help` lists the targets.
COMPOSE ?= docker compose
TEST    ?= $(COMPOSE) -f docker-compose.test.yml

.DEFAULT_GOAL := help
.PHONY: help up ha down logs build test test-backend test-frontend smoke fmt lint clean

help: ## show this help
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "  %-14s %s\n", $$1, $$2}'

.env:
	@cp .env.example .env && echo "created .env from .env.example – set APP_SECRET and POSTGRES_PASSWORD"

up: .env ## build and start the application and PostgreSQL
	$(COMPOSE) up -d --build

ha: .env ## start two replicas behind a load balancer
	$(COMPOSE) -f docker-compose.yml -f docker-compose.ha.yml up -d --build

down: ## stop everything (data volume is kept)
	$(COMPOSE) down

logs: ## follow the application log
	$(COMPOSE) logs -f app

build: ## build the image only
	$(COMPOSE) build app

test: test-backend test-frontend ## run all tests in containers

test-backend: ## cargo test (engine + server against a real PostgreSQL)
	$(TEST) run --rm backend

test-frontend: ## typecheck, unit tests and production build of the web app
	$(TEST) run --rm frontend

smoke: ## end-to-end check of a running stack (reads ADMIN_* from .env)
	@set -a; . ./.env; set +a; ./deploy/smoke.sh http://localhost:$${APP_PORT:-8080} "$${ADMIN_EMAIL:-admin@example.com}" "$$ADMIN_PASSWORD"

fmt: ## format Rust code
	cd backend && cargo fmt --all

lint: ## rustfmt + clippy
	cd backend && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings

clean: ## remove containers, volumes and test caches
	$(COMPOSE) down -v
	$(TEST) down -v
