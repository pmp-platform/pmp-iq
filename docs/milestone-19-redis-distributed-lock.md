# Milestone 19 — Redis distributed-lock backend

## Goal

Add a **Redis-backed** implementation of the existing `DistributedLock` trait
(`src/locks/`, M12), so multi-instance deployments can serialise work through a
shared Redis instead of the database. The trait, lease model, and all consumers
(controller leader election, the `sync-repositories` and `llm-repository-request`
jobs, M22 agent tasks) are unchanged — only a new backend and its wiring are
added. Redis is **disabled by default** (M18 `redis.enabled: false`); when
enabled it becomes the lock backend.

## Scope

- A `RedisLock` implementing `DistributedLock` (`acquire`/`refresh`/`release`)
  with correct TTL and token-ownership semantics.
- A thin `RedisClient` trait wrapping the few Redis operations the lock needs, so
  unit tests mock Redis and never touch a real server.
- A concrete client over the `redis` crate (async, pooled).
- Backend selection in `store::distributed_lock`: Redis when configured, else the
  existing SQL/in-memory backends.

## Deliverables

### `RedisClient` trait (mockable)

Keep the lock logic testable by isolating the two atomic Redis primitives behind
a trait — the lock owns the algorithm, the client owns the I/O:

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RedisClient: Send + Sync {
    /// SET key value NX PX ttl_ms — returns true when the key was set (acquired).
    async fn set_nx_px(&self, key: &str, token: &str, ttl_ms: u64) -> LockResult<bool>;

    /// Token-checked operation via a Lua script: act only if GET key == token.
    /// `Op::Pexpire(ttl_ms)` for refresh, `Op::Del` for release. Returns whether
    /// the script matched the token (i.e. we still hold the lease).
    async fn run_if_owner(&self, key: &str, token: &str, op: Op) -> LockResult<bool>;
}
```

The token-checked refresh/release use the classic single-instance Redlock Lua
scripts so they are atomic (no check-then-act race):

```lua
-- refresh: if redis.call('get', KEYS[1]) == ARGV[1]
--          then return redis.call('pexpire', KEYS[1], ARGV[2]) else return 0 end
-- release: if redis.call('get', KEYS[1]) == ARGV[1]
--          then return redis.call('del', KEYS[1]) else return 0 end
```

### `RedisLock` (the backend)

`src/locks/redis.rs`, implementing `DistributedLock`:

- `acquire(key, ttl)`: generate a `token` (uuid), `set_nx_px`. On success return
  `Some(Lease { key, token, expires_at: now + ttl })`; on failure `None`. Time
  comes from the injected `jobs::clock::Clock` (as the other backends do), never
  `Utc::now()` directly, so `expires_at` is unit-testable.
- `refresh(lease, ttl)`: `run_if_owner(Pexpire)`; on `true` return a fresh lease
  with the new `expires_at`, on `false` return `LockError::Lost(key)`.
- `release(lease)`: `run_if_owner(Del)`; a non-match is a no-op (already lost).
- A configurable key prefix (e.g. `pi:lock:`) so locks namespace cleanly in a
  shared Redis; format it through `lock_keys` helpers (DRY).

### Concrete client + dependency

`RedisClientImpl` over the `redis` crate (async, with a small connection pool —
`deadpool-redis` or the crate's `MultiplexedConnection`). Add the `redis` (and
pool) crate to `Cargo.toml`. The client only does `set_nx_px` and the two Lua
scripts — keep it under the file/function size limits.

### Wiring

`store::distributed_lock(db, config)` chooses the backend (extend its signature
to take the new `RedisConfig`/`Config`):

```text
redis.enabled            -> RedisLock(RedisClientImpl::connect(redis.url), clock)
else Postgres            -> PgSqlLock         (today's behaviour)
else SQLite              -> SqliteSqlLock      (today's behaviour)
```

Connection failure when `redis.enabled` is set is a hard startup error (fail
fast rather than silently degrade to a DB lock). Document in the README that
horizontal scaling across instances should enable Redis (or use the Postgres SQL
lock) — the in-memory backend is single-instance only.

## Tasks

- [ ] `RedisClient` trait + `Op` enum + `mockall` mock.
- [ ] `RedisClientImpl` over the `redis` crate (pooled) — `set_nx_px` + the two
      token-checked Lua scripts.
- [ ] `RedisLock` implementing `DistributedLock` with clock-driven `expires_at`.
- [ ] Backend selection in `store::distributed_lock` from `RedisConfig`.
- [ ] Unit tests (mocked `RedisClient` + mocked `Clock`): a held key returns
      `None`; `refresh` extends and a non-owner `refresh` yields `Lost`; `release`
      only deletes when the token matches; `expires_at` is computed from the
      injected clock.

## Acceptance criteria

- `RedisLock` satisfies `DistributedLock` with the same acquire/refresh/release
  contract as the in-memory and SQL backends, verified against a mocked client.
- With `redis.enabled: true` the app uses Redis for all locks (leader election +
  per-repo/per-job serialisation); with it false, behaviour is unchanged.
- No unit test touches a real Redis server or the real clock.
- The two-instance distributed compose topology (M20) coordinates correctly
  through the shared Redis lock.

## Dependencies

Milestones 12 (`DistributedLock` trait, `lock_keys`, `Clock`) and 18 (`redis`
config section). M20 exercises it across instances.

## Out of scope

Multi-node Redlock quorum across independent Redis servers (single shared Redis
is assumed), lock fairness/queueing, and pub/sub notifications — TTL leases only,
matching the existing backends.
