# Milestone 12 — Distributed locks abstraction

## Goal

Introduce a general-purpose **distributed lock** abstraction — a named lock with
a **TTL** that can be **refreshed** — and ship an **in-memory** implementation as
the first backend. Today the project only has a single-purpose, DB-backed leader
lock (`LeaderLock` over `controller_locks`) used for controller election. This
milestone generalises that into a reusable `DistributedLock` trait that any
subsystem (leader election, the `sync-repositories` job, the future
`llm-repository-request` job) can use to serialise work by an arbitrary key.

## Scope

- A `DistributedLock` trait: acquire a key with a TTL, refresh (extend) the
  lease, release it.
- An **in-memory** implementation (the first/default backend) with deterministic,
  clock-driven expiry.
- A DB-backed implementation of the same trait, reusing the existing
  `controller_locks` storage, so multi-instance deployments keep working.
- Migrate the existing leader election (`JobController`) onto the new trait.
- A shared lock-key helper so keys are formatted consistently across the project.

## Deliverables

### The trait

A new `src/locks/` module (trait in `mod.rs`/`lock.rs`):

```rust
/// An opaque, granted lease. Carries the holder token used to refresh/release.
#[derive(Debug, Clone)]
pub struct Lease {
    pub key: String,
    pub token: String,            // unique per acquisition (uuid)
    pub expires_at: DateTime<Utc>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DistributedLock: Send + Sync {
    /// Try to take `key` for `ttl`. Returns `Some(Lease)` when granted (the key
    /// is free, expired, or already held by the same caller), `None` when it is
    /// currently held by someone else.
    async fn acquire(&self, key: &str, ttl: Duration) -> LockResult<Option<Lease>>;

    /// Extend an existing lease by `ttl` from now. Errors if the lease is no
    /// longer held by this token (lost/expired).
    async fn refresh(&self, lease: &Lease, ttl: Duration) -> LockResult<Lease>;

    /// Release the lease (no-op if already lost).
    async fn release(&self, lease: &Lease) -> LockResult<()>;
}
```

- `LockError`/`LockResult` are typed (`thiserror`). `refresh` returns a fresh
  `Lease` with the new `expires_at`.
- Time is read from the existing `jobs::clock::Clock` trait (injected), never
  `Utc::now()` directly, so expiry is unit-testable.

### In-memory implementation (first backend)

`InMemoryLock`:
- Backs the lock with `Mutex<HashMap<String, Entry>>`, `Entry { token, expires_at }`.
- `acquire` grants when the key is absent, the stored entry's `expires_at <= now`,
  or the stored `token` matches the caller; otherwise returns `None`.
- `refresh` and `release` verify token ownership before acting.
- Process-local — correct for the **default single-instance / SQLite** deployment.
  Document that horizontal scaling requires the DB-backed backend.

### DB-backed implementation (multi-instance)

`SqlLock` (Pg + SQLite, behind the same trait):
- Reuses the existing `controller_locks` table (`name`, `holder`, `expires_at`) —
  it is already a generic key/holder/expiry store, so **no schema change** is
  required; the table now backs all keys, not just the controller.
- `acquire` is the existing upsert: `INSERT … ON CONFLICT (name) DO UPDATE …
  WHERE holder = $token OR expires_at < $now`, returning `Some(Lease)` when a row
  was affected.
- `refresh` re-runs the same conditional upsert with a new `expires_at`;
  `release` deletes the row only when `holder = $token`.

### Lock keys

A small `lock_keys` helper module so keys are uniform and collision-free
(reused everywhere — DRY string rule):
- `lock_keys::controller()` → `"job-controller"` (preserves today's name).
- `lock_keys::job(job_id)` → `"job:{id}"`.
- `lock_keys::repository(full_name)` → `"repo:{sanitised full_name}"`.

### Migrate leader election

- `JobController` (`src/jobs/controller.rs`) depends on `Arc<dyn DistributedLock>`
  instead of `Arc<dyn LeaderLock>`; `is_leader` becomes `acquire(controller_key,
  lease_ttl)` and the loop refreshes the held lease each tick.
- Remove the now-redundant `LeaderLock` trait (or keep it as a thin deprecated
  alias) once `JobController` is on `DistributedLock`.
- Default wiring in `AppState::build`: the in-memory backend for SQLite/single
  instance, the SQL backend when configured for multi-instance Postgres.

## Tasks

- [ ] `DistributedLock` trait, `Lease`, `LockError`/`LockResult` + mock.
- [ ] `InMemoryLock` with clock-driven expiry.
- [ ] `SqlLock` (Pg + SQLite) over `controller_locks`.
- [ ] `lock_keys` helpers.
- [ ] Move `JobController` onto `DistributedLock`; retire `LeaderLock`.
- [ ] Wire a default backend in `AppState::build`.
- [ ] Unit tests (mocked clock): a second holder is blocked until the first
      lease expires; `refresh` extends expiry; `release` frees immediately; an
      expired lease is reclaimable; token mismatch can't refresh/release.

## Acceptance criteria

- `DistributedLock` exists with acquire/refresh/release and a working in-memory
  backend whose TTL expiry and refresh are unit-tested with a mocked clock.
- The same trait has a DB-backed backend reusing `controller_locks`.
- Controller leader election runs through `DistributedLock`; the build is green
  and no test touches a real clock or database.

## Dependencies

Milestone 06 (jobs infrastructure — `Clock`, controller, `controller_locks`).

## Out of scope

Lock fairness/queueing, reentrancy beyond same-token re-acquire, and any
cross-process consensus stronger than TTL leases. Consumers of the lock (the
jobs that take it) land in M13–M14.
