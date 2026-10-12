-- Infra Architect: initial schema. All state lives in PostgreSQL; application instances are
-- stateless, so any number of replicas can run behind a load balancer.

create table meta (
  key   text primary key,
  value text not null
);
insert into meta (key, value) values ('config_rev', '1');

create table users (
  id                   uuid primary key default gen_random_uuid(),
  email                text not null,
  name                 text not null default '',
  role                 text not null check (role in ('admin', 'editor', 'viewer')),
  password_hash        text not null,
  disabled             boolean not null default false,
  must_change_password boolean not null default false,
  session_version      integer not null default 1,
  failed_attempts      integer not null default 0,
  locked_until         timestamptz,
  created_at           timestamptz not null default now(),
  last_login_at        timestamptz
);
create unique index users_email_lower on users (lower(email));

create table projects (
  id           uuid primary key default gen_random_uuid(),
  name         text not null,
  description  text not null default '',
  owner_id     uuid references users (id) on delete set null,
  -- working copy; optimistic concurrency through draft_rev
  draft        jsonb not null,
  draft_rev    bigint not null default 1,
  head_version integer not null default 0,
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now(),
  deleted_at   timestamptz
);
create index projects_active on projects (updated_at desc) where deleted_at is null;

-- immutable snapshots
create table versions (
  project_id     uuid not null references projects (id) on delete cascade,
  n              integer not null,
  message        text not null default '',
  author_id      uuid references users (id) on delete set null,
  author         text not null default '',
  architecture   jsonb not null,
  summary        jsonb not null default '{}',
  engine_version integer not null,
  created_at     timestamptz not null default now(),
  primary key (project_id, n)
);

-- accepted risks
create table waivers (
  id          uuid primary key default gen_random_uuid(),
  project_id  uuid not null references projects (id) on delete cascade,
  fingerprint text not null,
  rule_id     text not null,
  reason      text not null,
  created_by  text not null default '',
  created_at  timestamptz not null default now(),
  expires_at  timestamptz,
  revoked_at  timestamptz,
  revoked_by  text
);
create unique index waivers_active on waivers (project_id, fingerprint) where revoked_at is null;

create table ai_jobs (
  id               uuid primary key default gen_random_uuid(),
  project_id       uuid references projects (id) on delete cascade,
  user_id          uuid references users (id) on delete set null,
  kind             text not null,
  status           text not null check (status in ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  input            jsonb not null default '{}',
  steps            jsonb not null default '[]',
  result           jsonb,
  error            text,
  error_code       text,
  instance         text not null default '',
  cancel_requested boolean not null default false,
  created_at       timestamptz not null default now(),
  started_at       timestamptz,
  finished_at      timestamptz,
  heartbeat_at     timestamptz not null default now()
);
create index ai_jobs_user_recent on ai_jobs (user_id, created_at desc);
create index ai_jobs_active on ai_jobs (status) where status in ('queued', 'running');

create table proposals (
  id         uuid primary key default gen_random_uuid(),
  project_id uuid not null references projects (id) on delete cascade,
  job_id     uuid references ai_jobs (id) on delete set null,
  base_rev   bigint not null,
  -- {mode: "ops", ops: [...]} or {mode: "replace", architecture: {...}}
  change     jsonb not null,
  rationale  text not null default '',
  preview    jsonb not null default '{}',
  status     text not null default 'pending' check (status in ('pending', 'applied', 'rejected', 'stale')),
  created_by uuid references users (id) on delete set null,
  created_at timestamptz not null default now(),
  decided_at timestamptz
);
create index proposals_project on proposals (project_id, created_at desc);

create table chat_messages (
  id          bigserial primary key,
  project_id  uuid not null references projects (id) on delete cascade,
  role        text not null check (role in ('user', 'assistant', 'system')),
  content     text not null,
  proposal_id uuid references proposals (id) on delete set null,
  job_id      uuid references ai_jobs (id) on delete set null,
  user_id     uuid references users (id) on delete set null,
  created_at  timestamptz not null default now()
);
create index chat_messages_project on chat_messages (project_id, id);

create table llm_calls (
  id                bigserial primary key,
  job_id            uuid,
  project_id        uuid,
  user_id           uuid,
  purpose           text not null,
  requested_model   text not null,
  model             text,
  status            text not null,
  http_status       integer,
  prompt_tokens     integer,
  completion_tokens integer,
  cost_usd          double precision,
  latency_ms        integer,
  error             text,
  prompt_hash       text,
  created_at        timestamptz not null default now()
);
create index llm_calls_created on llm_calls (created_at desc);

create table settings (
  key        text primary key,
  value      jsonb not null,
  encrypted  boolean not null default false,
  updated_by text not null default '',
  updated_at timestamptz not null default now()
);

-- catalog / rules / templates / prompts / engine settings. `origin = default` rows follow the
-- shipped defaults until an administrator modifies them (`modified`), after which upgrades only
-- flag `update_available` instead of overwriting.
create table config_items (
  kind             text not null,
  id               text not null,
  data             jsonb not null,
  origin           text not null default 'default' check (origin in ('default', 'custom')),
  modified         boolean not null default false,
  default_hash     text,
  update_available boolean not null default false,
  sort             integer not null default 0,
  updated_at       timestamptz not null default now(),
  updated_by       text not null default '',
  primary key (kind, id)
);

create table audit_log (
  id          bigserial primary key,
  at          timestamptz not null default now(),
  actor_id    uuid,
  actor       text not null default '',
  action      text not null,
  target_type text not null default '',
  target_id   text not null default '',
  details     jsonb not null default '{}',
  ip          text not null default ''
);
create index audit_log_at on audit_log (at desc);
