# Milestone 46 — Auto-remediation tasks

## Goal

Close the loop from **insight to action**: when a finding appears — a metric
crosses a threshold (M31/M33), a scorecard check fails (M43), an EOL/outdated
dependency is detected (M45), a policy/drift event fires (M36), or a new
vulnerability is found — automatically open an **AI Agent change task** (M22/M23)
or a **campaign** (M30) to fix it, gated by configurable rules and an approval
step. pmp-iq already discovers problems and can already make cross-repo changes;
this milestone wires the two together so the fleet self-heals (with a human in
the loop) instead of relying on someone noticing a dashboard.

## Scope

- A rule store: **trigger condition → remediation action** (an agent-task /
  campaign template), with scope (which apps) and an enabled flag.
- An evaluator job that finds matching findings, **deduplicates** against open
  remediations, and enqueues the agent task / campaign.
- An **approval gate**: remediations open in a `proposed` state; an authorised
  operator approves before the PR-opening turn runs.
- A remediation queue/history view.

## Deliverables

### Rules & remediations

```sql
-- migrate:up
-- A trigger (a built-in finding evaluator + params) → an action template.
CREATE TABLE remediation_rules (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    trigger       TEXT NOT NULL,           -- metric_below | scorecard_failed | dep_eol | new_vuln | policy_violation
    params        JSONB NOT NULL DEFAULT '{}',
    action        TEXT NOT NULL,           -- agent_task | campaign
    prompt        TEXT NOT NULL,           -- the change instruction given to the agent
    scope         JSONB NOT NULL DEFAULT '{}', -- app filter (blank = whole fleet, like campaigns M30)
    auto_approve  BOOLEAN NOT NULL DEFAULT FALSE,
    enabled       BOOLEAN NOT NULL DEFAULT TRUE
);
-- One emitted remediation, linked to the work it drives.
CREATE TABLE remediations (
    id             UUID PRIMARY KEY,
    rule_id        UUID REFERENCES remediation_rules(id) ON DELETE SET NULL,
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    finding_key    TEXT NOT NULL,          -- stable dedup key (rule + app + finding)
    status         TEXT NOT NULL,          -- proposed | approved | running | done | dismissed
    agent_task_id  UUID,                   -- the M22/M23 task it opened
    campaign_id    UUID,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (rule_id, finding_key)
);
-- migrate:down
```

### Evaluator job

A `remediation` job (cron, leader-elected like the controller) that, for each
enabled rule, evaluates its built-in trigger against current state:

- `metric_below` / `scorecard_failed` — read latest metrics (M31) / scorecard
  results (M43).
- `dep_eol` — read currency (M45).
- `new_vuln` — read security metrics / SBOM findings.
- `policy_violation` — read change/drift signals (M36).

For each new finding (not already an open `remediations` row, deduped by
`finding_key`) it creates a `proposed` remediation. The job records LLM usage
through the recorder so M39 prices it; budgets (M39) and ownership (M37) are
honoured — a remediation on an app a maintainer doesn't own needs an admin.

### Approval & execution

- `proposed` remediations are listed for review; **approve** flips them to
  `approved` and enqueues the M22/M23 agent task or M30 campaign with the rule's
  prompt (e.g. "add unit tests to raise coverage above 70%", "bump axum to the
  latest minor"). `auto_approve` rules skip the gate.
- The PR watcher (M24) already drives the resulting PR to completion; the
  remediation status mirrors the task/campaign target status.
- Dismissing a remediation suppresses re-proposal for that finding until it
  recurs after being resolved.

### UI

- A **Remediation** section: the rules editor (Settings), the queue (`proposed` →
  `done`) with approve/dismiss, and history; per-app open remediations on the
  application detail. Reuses the campaigns/agent-task progress UI (M23/M30).

## Tasks

- [ ] `remediation_rules` + `remediations` migrations (both engines) + dual-engine
      repository; dedup by `finding_key`.
- [ ] `remediation` evaluator job (built-in triggers over metrics/scorecards/
      currency/vulns/drift); proposes deduped remediations; budget + ownership
      honoured.
- [ ] Approval gate + enqueue of agent task (M22/M23) / campaign (M30); status
      mirrors the task targets; dismiss/suppress.
- [ ] Rules editor + remediation queue/history UI; per-app open remediations.
- [ ] Unit tests (mocked repos/triggers): a finding proposes exactly one
      remediation (no dup on re-eval); approval enqueues the right action; a
      maintainer can only approve owned apps; dismissed findings don't re-propose.

## Acceptance criteria

- Findings from metrics, scorecards, currency, vulns and drift automatically
  propose deduplicated remediation tasks/campaigns; nothing opens a PR before
  approval (unless a rule is explicitly `auto_approve`).
- Remediations drive real M22/M23/M30 work, mirror its status, respect M37
  ownership and M39 budgets, and never re-propose a dismissed/already-open finding.
- Trigger evaluation and dedup/approval flow are unit-tested with mocked storage
  on both engines.

## Dependencies

Milestones 22/23 (agent tasks), 30 (campaigns), 24 (PR watcher drives the PRs),
31/33 (metrics), 43 (scorecards), 45 (currency), 36 (drift/policy), 37 (ownership
gating), 39 (cost/budgets). The single highest-leverage feature: it makes every
detector actionable.

## Out of scope

Fully autonomous merge without any human approval as the default (opt-in per rule
only), arbitrary user-scripted triggers (built-in evaluators + params), and
remediations that touch infrastructure outside the analysed repositories.
