# Everything runs in containers; `make help` lists the targets.
COMPOSE ?= docker compose
TEST    ?= $(COMPOSE) -f docker-compose.test.yml $(if $(CA_BUNDLE),-f docker-compose.ca.yml)

.DEFAULT_GOAL := help
.PHONY: help try up ha down logs build test test-backend test-frontend iac-check smoke fmt lint clean

# make try: the image that is pulled (IA_IMAGE), and when (TRY_PULL: always | missing | never; CI uses an image it has just built, which no registry has)
IA_IMAGE ?= ghcr.io/paffin/infra-architect:latest
TRY_PULL ?= always
export IA_IMAGE

# what to say once the stack is up: the address and who signs in (read from .env without sourcing it: a value with a space would break a shell)
ANNOUNCE = port=$${APP_PORT:-$$(sed -n 's/^APP_PORT=//p' .env | head -n 1)}; email=$${ADMIN_EMAIL:-$$(sed -n 's/^ADMIN_EMAIL=//p' .env | head -n 1)}; echo "Infra Architect: http://localhost:$${port:-8080}  (sign in as $${email:-admin@example.com} with the ADMIN_PASSWORD from .env)"

# a .env kept from before the secrets were generated (the old .env.example shipped `change-me...`) holds the published APP_SECRET, which the server refuses to start with
# in production; compose then only shows an unhealthy or exited container, so say it here, where the operator is looking (not fatal: APP_ENV=development may keep it)
WARN_PLACEHOLDER = if grep -qiE '^APP_SECRET=change[-_]?me' .env; then echo "warning: APP_SECRET in .env is the placeholder from .env.example and the application will refuse to start with it (see: docker compose logs app). Move this .env aside and run 'make .env' for a new one with generated secrets, or put your own APP_SECRET in it (openssl rand -hex 32)." >&2; fi

help: ## show this help
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "  %-14s %s\n", $$1, $$2}'

# the secrets are generated, never copied from the example: see deploy/init-env.sh (does nothing when .env exists)
.env:
	@sh deploy/init-env.sh

try: .env ## start from the published image, nothing is built (needs only Docker)
	@$(WARN_PLACEHOLDER)
	@$(COMPOSE) -f docker-compose.yml -f deploy/compose.try.yml up -d --pull $(TRY_PULL) --wait --wait-timeout 180 || { echo "could not start from $(IA_IMAGE) - is it published yet? 'make up' builds the image from the source instead" >&2; exit 1; }
	@$(ANNOUNCE)

up: .env ## build the image and start the application and PostgreSQL
	@$(WARN_PLACEHOLDER)
	$(COMPOSE) up -d --build --wait --wait-timeout 180
	@$(ANNOUNCE)

ha: .env ## start two replicas behind a load balancer
	@$(WARN_PLACEHOLDER)
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

iac-check: ## generate IaC for the reference architectures and validate it (Kubernetes/CRD schemas, Helm, Terraform)
	$(TEST) run --rm iac-export
	$(TEST) run --rm --build iac-check

smoke: ## end-to-end check of a running stack (reads ADMIN_* from .env)
	@set -a; . ./.env; set +a; ./deploy/smoke.sh http://localhost:$${APP_PORT:-8080} "$${ADMIN_EMAIL:-admin@example.com}" "$$ADMIN_PASSWORD"

fmt: ## format Rust code
	cd backend && cargo fmt --all

lint: ## rustfmt + clippy
	cd backend && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings

clean: ## remove containers, volumes and test caches
	$(COMPOSE) down -v
	$(TEST) down -v
