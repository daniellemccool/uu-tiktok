# Plan B kickoff prompt — paste into a fresh Claude Code session when ready

> **Author note:** Plan A's walking skeleton is alive and produces real transcripts. Before kicking off Plan B planning, the operator should run the prototype against actual donations to surface what's worth prioritizing (rough edges, scaling pain, missing diagnostics). The triggers in `docs/FOLLOWUPS.md` are the deferred-work backlog; the operator's hands-on experience refines what's truly load-bearing for Plan B vs what can wait for Plan C.

---

## Prompt to paste

I want to begin planning **Plan B** for the UU TikTok donation-data transcription pipeline (`/home/dmm/src/uu-tiktok`). Plan A — the walking skeleton — is complete on `feat/plan-a-walking-skeleton` (now merged or pending PR). I have spent some time running the prototype against real DDP exports and have observations about what's load-bearing for Plan B.

### Step 1: Orient yourself before discussing scope

Read these in order:

1. `docs/superpowers/plans/2026-04-16-plan-a/00-overview.md` — Plan A's design and "What Plan A Deliberately Omits" list
2. `docs/superpowers/plans/2026-04-16-plan-a/RETRO.md` — what worked, what didn't, recommendations for Plan B
3. `docs/FOLLOWUPS.md` — every deferred concern from Plan A reviews (~20 entries with trigger conditions)
4. `docs/decisions/AD0001-*` through `AD0008-*` — every architectural decision from Plan A
5. The current state of `src/` — what actually exists today

Do NOT yet read the original design spec at `docs/superpowers/specs/2026-04-16-uu-tiktok-pipeline-design.md` — it's the long version of Plans A+B+C and will pollute context for the Plan B planning conversation. Plan A's overview file already extracts what Plan B needs to know about the design.

### Step 2: Brainstorm Plan B's actual scope with me

Use the `superpowers:brainstorming` skill. The brainstorm should answer:

1. **What does Plan B prioritize?** Rank the FOLLOWUPS clusters and my hands-on observations. Likely candidates (do not assume — confirm with me):
   - Failure classification (5 FOLLOWUPS entries cluster here: T6 RunError mapping, T11 YtDlpFetcher mapping, T12 TranscribeError mapping, T10 mark_succeeded predicate, T14 typed errors not anyhow). A `RetryableKind` / `UnavailableReason` / `ClassifiedFailure` design is the natural starting ADR.
   - Stale-claim recovery and concurrent workers (today the pipeline is strictly serial; a worker crash leaves rows in_progress forever).
   - Time-window filter on `watch_history.watched_at` (will surface the DDP-timezone-UTC-assumption FOLLOWUPS as a correctness blocker).
   - Multi-fetcher provenance (the hardcoded `"ytdlp"` lie becomes load-bearing if Plan B introduces a Research API fetcher).
   - Sync IO inside async fns sweep (5 FOLLOWUPS entries; cosmetic individually, costly collectively for concurrency).
   - Bin/lib structural reassessment per AD0002's deferred decision.

2. **What does Plan B explicitly defer to Plan C?** Plan A's overview already lists Plan C's scope (short-link resolution, API-direct path, statistical analysis on transcripts). Confirm Plan B doesn't drift into Plan C territory.

3. **What's the smallest Plan B that makes Plan A operationally trustworthy?** Plan A "works" but failure modes leave rows stuck and operators have no recovery path. Plan B's MVP should answer "what does an operator do when something goes wrong."

### Step 3: Re-confirm process inheritance from Plan A

Plan A's process worked well (per RETRO.md). Inherit it for Plan B:

- **Per-task file split** (AD0001) — one self-contained brief per task, plus an `00-overview.md`. Plan B's directory will be `docs/superpowers/plans/<date>-plan-b/`.
- **ADR discipline** — `adg` tool; meta/process ADRs on main, feature-derived ADRs on the feat branch. Read AD0001-AD0008 and respect them unless explicitly superseding (in which case write a new ADR with `succeeds: ["NNNN"]`).
- **AD0002 cleanup-on-consumption** — every dispatch enumerates allows to ADD/REMOVE; reviewer verifies.
- **AD0003 deviation-honesty** — clippy fixes, brief bug corrections, and structural deviations all get prominent commit-message disclosure.
- **AD0006 mutator signature convention** — every new Store mutator returns `Result<usize>` carrying the row-change count (especially `mark_failed_terminal`, `mark_failed_retryable`, `update_progress`).
- **AD0007 stats counter convention** — input-side counters with verb-named fields; HashSet pattern for uniques; `_processed` and `_skipped`/`_failures` are PARALLEL counters.
- **AD0008 artifact-write ordering** — DB acknowledges success ONLY after artifacts are durable. Any new pipeline mutator (e.g., partial-result persistence, retry-state writes) MUST preserve this invariant.
- **AD0005 test-helpers feature** — every new integration test file gets `[[test]] required-features = ["test-helpers"]` in Cargo.toml.
- **FOLLOWUPS discipline** — `docs/FOLLOWUPS.md` already exists; resolved entries get removed (git history retains them); new entries added during Plan B reviews.

### Step 3a: Three meta-process improvements identified after Plan A — apply during this planning session

Per `docs/superpowers/plans/2026-04-16-plan-a/RETRO.md`'s "Meta-process improvements" section, three Plan A pain points have explicit fixes for Plan B. The brainstorm in Step 2 must apply all three.

1. **Capture coherence decisions during brainstorm, not retrospectively.** As we discuss Plan B's design, maintain a running list of decision-candidates — not only architectural choices but coherence-maintaining choices (path layout, error mapping, test discipline, naming conventions, branch hygiene). Capture each as an ADR draft via `adg add` BEFORE writing the per-task plan files. The Plan A failure mode was AD0001-3 backfilled retroactively after patterns emerged across multiple tasks; AD0004's branch-placement reversal happened because the placement rule wasn't recorded when first articulated. By the time Plan B's per-task files exist, the relevant ADRs should already exist alongside them.

2. **Structure tasks for early MVP.** Plan A produced a runnable end-to-end thing only at T14/T15 (the last 13% of the work). The user couldn't critically evaluate the prototype until everything was done. For Plan B, structure the task list as small epics where Epic 1 produces a runnable thin-slice MVP the operator can use — even if rough. Subsequent epics harden it. Recommended Plan A re-imagining for reference: Epic 1 = `transcribe-one URL` hardcoded path (no DB, no ingest, no sharding); Epic 2 = state DB + claim/process loop; Epic 3 = DDP-JSON ingest; Epic 4 = polish (init, e2e, sharding, atomic writes). Apply the same "MVP first, harden second" shape to whatever Plan B's actual scope turns out to be.

3. **Curated per-task ADR dispatch.** When dispatching implementer or reviewer subagents during Plan B, the orchestrator (you, the controller) should pre-select which ADRs are directly relevant to each task and tell the subagent to read THOSE plus the overview — not all of them. The orchestrator has the full context to curate; subagents shouldn't waste tokens scanning ADRs that don't apply. Lightweight form: dispatch prompts include an explicit "ADRs directly relevant: AD000X, AD000Y" line; the rest are listed under "background, available if needed." Heavier form (only if Plan B grows beyond ~12 ADRs): tag-based ADR query via `adg query --tags`. Start with the lightweight form; escalate only if cost-benefit demands.

### Step 4: Subagent dispatch model

Per Plan A's RETRO.md:
- **Sonnet** for spec compliance reviews (mechanical "does this match the brief verbatim modulo documented deviations").
- **Opus** for code quality reviews (qualitative correctness; opus catches structural issues sonnet misses — Tokio runtime panics, provenance lies, timezone assumptions).
- **Sonnet** for most implementer dispatches; **opus** for tasks with multiple subtle interactions (heavy AD0002 cleanup, async closure-Future capture, cross-module signature changes).
- **Single-flight Agent dispatch** is enforced by `~/.claude/hooks/agent-lock-acquire.sh` — laptop is thermally constrained.

### Step 5: Hand control back to me before dispatching anything

After the brainstorm and before writing the per-task plan files: surface your understanding of Plan B's scope, scoped omissions, and proposed task-list structure. Wait for my approval. THEN use `superpowers:writing-plans` to draft the plan.

Plan A took ~15 tasks; Plan B may be smaller (failure classification + recovery is one major chunk; multi-worker is another) or larger (depending on how much sync-IO sweep work it absorbs). Right-size based on the brainstorm.

### What NOT to do

- Do NOT immediately start implementing. Plan B's scope hasn't been confirmed with me.
- Do NOT read the original design spec yet — defer until you need it for a specific task.
- Do NOT propose merging Plan A to main without my explicit go-ahead. Plan A may live on its branch as a reference implementation while Plan B builds on top.
- Do NOT skip the FOLLOWUPS audit. Every deferred concern is a candidate for Plan B; some are blockers, some are deferrable to Plan C.
- Do NOT re-litigate Plan A decisions captured in AD0001-AD0008. If you think one needs revisiting, propose superseding it via a new ADR with explicit reasoning.

### Done state for this kickoff session

By end of session:
- Brainstorm conversation captured (what Plan B does and doesn't do)
- Plan B's per-task file structure drafted at `docs/superpowers/plans/<date>-plan-b/`
- Updated FOLLOWUPS.md if any entries are explicitly resolved-by-Plan-B-scope or moved to Plan C
- I have a clear next-action: either approve and start dispatching, or iterate on the plan
