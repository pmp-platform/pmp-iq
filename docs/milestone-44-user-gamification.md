# Milestone 44 — Operator gamification (levels, XP, skills, badges)

## Goal

Reward the **operators** who improve the platform — the principals (M03/M21) who
run agentic change tasks (M22/M23), drive batch campaigns (M30), resolve drift
(M36) and raise scorecards (M43) — with **XP**, **levels**, **skills** and
**badges**. The platform already records *who did what* in the audit log (M36)
and *what changed* in the change feed; this milestone turns that history into a
lightweight progression system and leaderboards that make platform-health work
visible and a little bit fun, without inventing any new tracking.

## Scope

- XP awarded from already-recorded operator actions (merged agent-task PRs,
  campaign completions, coverage/score improvements, resolved vulns/violations).
- Derived **skills** (e.g. per language/ecosystem, "security", "refactoring")
  from the kind of work and the apps touched.
- Levels from cumulative XP; **badges** for milestones (first PR, 10 merged,
  "raised a service to gold", "zero-criticals sweep").
- A profile page per operator and a leaderboard; opt-out respected.

## Deliverables

### Award model

XP is derived, not hand-assigned: an **award rule** maps an audit/change event
(M36) to points, evaluated by a job so awards are reproducible and idempotent
(keyed by the source event so re-runs don't double-count):

```sql
-- migrate:up
CREATE TABLE xp_awards (
    id         UUID PRIMARY KEY,
    actor      TEXT NOT NULL,             -- principal username (M03)
    reason     TEXT NOT NULL,             -- agent_task.merged | campaign.completed | coverage.raised | ...
    points     INT  NOT NULL,
    skill      TEXT,                       -- optional skill tag (rust | security | refactoring | ...)
    source     TEXT,                       -- dedup key (audit_event id / change id / PR url)
    awarded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (actor, reason, source)
);
CREATE TABLE badges (
    actor      TEXT NOT NULL,
    badge      TEXT NOT NULL,              -- first_pr | ten_merged | gold_maker | zero_crit | ...
    awarded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (actor, badge)
);
-- migrate:down
```

### Engine

Pure, unit-testable functions:

- `level_for(total_xp) -> (level, xp_into_level, xp_to_next)` over a configurable
  curve.
- `skills(awards) -> Vec<(skill, points, rank)>` aggregating skill XP into ranks.
- `badges_earned(stats) -> Vec<Badge>` evaluating badge conditions from counters.

Skill tags are inferred from the work: an agent task / campaign PR on a Rust app
grants `rust` XP; a remediation that resolved a vuln grants `security`; a
coverage/scorecard improvement grants `quality`. The actor and the touched
application/skill come from the existing audit event metadata (M36) — no new
instrumentation.

### Awarding job

A `gamification` job (cron) replays new audit + change events through the award
rules and upserts `xp_awards`/`badges` (idempotent via the `source` dedup key),
so a backfill over historical events is a single run. LLM cost is not involved.

### UI

- A **profile** page per principal: level + progress bar, skill ranks, badges,
  recent awards, and the apps they own/contributed to (linking M37 teams).
- A **leaderboard** (overall + per skill + per team) on a new gamification tab.
- A small level/XP chip in the header for the logged-in operator.

## Tasks

- [ ] `xp_awards` + `badges` migrations (both engines) + dual-engine repository;
      idempotent upsert keyed by `source`.
- [ ] Award rules mapping audit/change events → points + skill; the
      `gamification` replay job (cron, backfillable).
- [ ] Pure `level_for` / `skills` / `badges_earned` engine.
- [ ] Profile page, leaderboard tab, header XP chip; opt-out flag honoured.
- [ ] Unit tests: award rules grant the right points/skill once per source (no
      double-count on replay); level/skill/badge math; opt-out hides a principal.

## Acceptance criteria

- Operators accrue XP, levels, skills and badges purely from recorded actions
  (agent tasks, campaigns, drift/vuln/score improvements); replaying events never
  double-counts.
- A profile and a leaderboard (overall, per skill, per team) are viewable;
  individuals can opt out.
- The progression math and award rules are unit-tested with mocked storage on both
  engines; no new tracking is introduced beyond the existing audit/change feeds.

## Dependencies

Milestones 36 (audit + change feed = the event source), 22/23/30 (agent tasks +
campaigns = the rewarded work), 37 (principals/teams), 43 (scorecard improvements
as an XP source), 06/13 (the cron job + replay). Optional link to discovered
`users` (M08) when a principal matches a code-owner.

## Out of scope

Real-money rewards, cross-organisation competitive ladders, and manual XP
granting — XP is derived from real work only. Gamifying the discovered git
`users` (vs. operators) and external profile sharing are also out of scope.
