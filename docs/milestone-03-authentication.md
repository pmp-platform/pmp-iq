# Milestone 03 — Authentication & login strategies

## Goal

Gate the application behind login. Ship a **pluggable** auth design (so SSO/OIDC
can be added later) with the first concrete strategy: a single admin user from
`ADMIN_USER` / `ADMIN_PASS`, or an auto-generated `admin` + random password
printed once at boot.

## Scope

- `LoginStrategy` trait + a `StaticAdminStrategy` implementation.
- Secure password handling (hashing/verification).
- Session management with signed cookies.
- Login / logout pages and auth middleware protecting all sections.

## Deliverables

### Strategy abstraction

- `LoginStrategy` trait with `authenticate(credentials) -> Result<Principal,
  AuthError>` and an identifier/name.
- A registry that selects enabled strategies; for now only `StaticAdminStrategy`.
- `Principal` carries identity (username, display name, roles) — note this is the
  **operator** logging in, distinct from the discovered platform `users` table in
  M08.

### Static admin strategy

- If `ADMIN_USER` and `ADMIN_PASS` are set, use them.
- If unset, generate username `admin` and a strong random password at boot,
  **log it once** (warn level), and use it for this run. Document this behaviour.
- Passwords are verified with a memory-hard hash (`argon2`). The configured/seed
  password is hashed at startup; plaintext is never stored.

### Sessions

- Server-side or signed-cookie sessions (e.g. `tower-sessions`) keyed by the
  session secret from config.
- Session creation on login, invalidation on logout, configurable TTL.

### Middleware & pages

- Auth middleware redirects unauthenticated users to `/login` for pages and
  returns `401` for API routes.
- `GET /login` (form, jQuery-enhanced), `POST /login`, `POST /logout`.
- Nav shows the current user and a logout control.

## Tasks

- [ ] Add `argon2`, a session crate, and a CSRF guard for the login form.
- [ ] Define `LoginStrategy`, `Principal`, `AuthError`; implement
      `StaticAdminStrategy`.
- [ ] Implement boot logic: env creds or generated admin (random password via a
      `PasswordGenerator` trait so tests are deterministic with a mock).
- [ ] Add session layer + auth middleware.
- [ ] Build login/logout pages and protect all sections.
- [ ] Unit-test the strategy (correct/incorrect/missing creds) with mocked hasher
      and generator — no env or real randomness in tests.

## Acceptance criteria

- With `ADMIN_USER`/`ADMIN_PASS` set, those credentials log in; wrong ones fail.
- With them unset, boot logs a generated `admin` password once and it works.
- Unauthenticated page requests redirect to `/login`; API requests get `401`.
- Logout clears the session. Strategy logic is fully unit-tested with mocks.

## Dependencies

Milestones 00–02.

## Out of scope

Additional strategies (OIDC/SSO), multi-user management, RBAC beyond a basic
admin role.
