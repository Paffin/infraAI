-- Counters of the limits that every replica has to see the same: sign-in attempts per address, failures per account and address.
-- Before this they lived in the memory of each process, so N replicas allowed N times the limit, and a restart forgot every lock.
-- One row per key. The window starts at the first hit (not at a wall-clock minute, which a limit would straddle), and a lock lasts
-- from the hit that tripped it. Rows are deleted by the retention job once their window is over and they are not locked.
create table rate_limits (
  key          text primary key,
  window_start timestamptz not null default now(),
  count        integer not null default 0,
  locked_until timestamptz
);
create index rate_limits_window on rate_limits (window_start);
