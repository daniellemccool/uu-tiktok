# Plan A retrospective — UU TikTok pipeline walking skeleton

**Branch:** `feat/plan-a-walking-skeleton`
**Date completed:** 2026-04-17
**Tasks:** 15 of 15
**ADRs created:** 8 (AD0001–AD0008)
**FOLLOWUPS recorded:** ~20 deferred items for Plan B/C
**Tests:** 76 passing + 1 ignored (e2e_real_tools, requires real tools)
**Commits:** 33 on feat, 1 standalone on main (AD0004)
**Implementation hours:** ~8 hours of subagent-driven development across multiple sessions

## What worked

### Per-task file split (AD0001)

The first ADR was the most important one. Splitting the original 3347-line plan into 16 per-task files (`00-overview.md` + `01` through `15`) cut subagent context dramatically. Each implementer dispatch read only its own brief plus the overview plus the ADRs — typically under 1500 tokens of plan material per task. The original "subagent reads the whole plan" approach would have blown the context budget after T3 or T4.

The convention "self-contained per-task file" was load-bearing for the entire run.

### ADR discipline

Eight ADRs captured every cross-cutting structural decision with rejected alternatives. AD0002 (dead-code suppression) and AD0003 (test discipline) became the most-referenced; reviewers consulted them on every dispatch. AD0006 (Store mutator signature) and AD0007 (input-side counter semantics) were both surfaced retroactively from T13 deviations and immediately codified — preventing future tasks from re-litigating the same questions.

The `adg` tool worked well after two minor argparse quirks were learned (commas in titles split on `,`; characters like `/` create directories instead of files).

### Opus for heavy implementation tasks

Sonnet implementers handled T1-T13 well but hit PAUSEs on T7, T9, T12, T13 for brief bugs they correctly flagged but couldn't resolve unilaterally. T14 (the heaviest task — 17 allows to remove, three pre-authorized deviations, async closure-Future capture lifetimes, mark_succeeded signature propagation) was implemented by opus and ran without any PAUSEs. Opus also implemented T15 cleanly.

The cost-quality trade-off: opus dispatches are more token-expensive but avoid the controller-resolves-blocker fixup cycles that sonnet hit. For tasks with multiple subtle interactions, opus pays for itself.

### Two-tier review model

Sonnet for spec-compliance review (mechanical "does this match the brief verbatim modulo documented deviations"), opus for code-quality review (qualitative "is this code actually correct"). Opus reviews caught:

- The `block_on` inside async runtime panic (T14)
- The provenance lie (`fetcher: "ytdlp"` regardless of actual fetcher) (T14)
- The DDP timestamp UTC assumption with no documentary basis (T13)
- The empirically-unnecessary `parse_language` allow (T12)
- The `concurrent_claim_serializes_via_begin_immediate` test that doesn't actually race (T10)
- The `mark_succeeded` missing `WHERE status='in_progress'` predicate (T10)

Several of these would have been silent latent bugs that Plan B would inherit.

### Single-flight thermal lock

The hook at `~/.claude/hooks/agent-lock-acquire.sh` (mkdir-based atomic lock at `/tmp/claude-agent.lock`, 30-min stale threshold) kept the dual-core 2015 laptop usable across the entire run. Dispatching two agents in parallel would have spiked CPU above 80°C; serializing kept everything at workable temperatures. One stale-lock incident occurred when the user interrupted a dispatch mid-flight (PostToolUse hook didn't fire); manual `rm -rf /tmp/claude-agent.lock` resolved it.

### AD0002 cleanup-on-consumption discipline

Every implementer dispatch included an explicit list of `#[allow(dead_code)]` annotations to ADD (for new items) or REMOVE (for items now consumed). T13 and T14 in particular had heavy cleanup lists that the controller pre-computed by tracing main()'s reachability graph. T15's close-out audit confirmed three more allows (`Config`, `SCHEMA_VERSION`, `SCHEMA_SQL`) had become dead post-T15 and removed them empirically (verified by removing the allow and confirming clippy stayed clean).

### AD0003 deviation-honesty in commit messages

Every brief deviation — clippy-driven cosmetic fixes, structural choices that diverged from verbatim brief, AD0002 cleanups — got prominent disclosure in commit message bodies. This kept the deviation history readable in `git log`; future contributors investigating "why does the code look like this" can see the reasoning without spelunking through code review threads.

### FOLLOWUPS file discipline

`docs/FOLLOWUPS.md` accumulated ~20 entries during the run, each with **Found in**, **Disposition**, and **Trigger to revisit** metadata. Categories that emerged:

- Failure classification (T6, T11, T12 all flagged related concerns) — Plan B
- Sync IO inside async fns (T11, T12, T13, T14 all flagged) — Plan B
- DDP timestamp timezone (T13) — Plan B/C
- Multi-fetcher provenance (T14) — Plan B
- Stale-claim recovery (T10) — Plan B
- Test coverage gaps for actual concurrency (T10), .json content (T14) — Plan B
- Stale `conn`/`conn_mut` accessors (T10/T11) — AD0002 reassessment
- `output::shard_dir` unused (T15) — VideoId newtype work in Plan A → Plan B reassessment

The FOLLOWUPS file replaced what would otherwise have been "I'll remember this" or scattered TODO comments. Nothing was lost.

## What didn't work / needed correction

### Initial sonnet brief-verbatim hits

Several T-tasks had subtle brief bugs that sonnet implementers correctly flagged as PAUSEs but couldn't resolve unilaterally:

- **T9:** clippy `bool_assert_comparison` on `assert_eq!(row.canonical, true)` (verbatim test code)
- **T12:** unused `use anyhow::Context;` + redundant `.trim()` before `.split_whitespace()`
- **T13:** TWO real bugs — production code calling cfg-gated test helper `get_video_for_test`; idempotence test mathematically inconsistent with brief implementation semantics
- **T14:** `tokio::runtime::Handle::block_on` inside async closure (runtime panic)

The fix evolved to pre-authorizing known brief bugs in dispatch prompts. By T14 dispatch, the controller pre-flagged three deviations with explicit fix instructions; opus applied them all without PAUSE.

### AD0004 branch placement reversal

AD0004 was originally placed on `main` (following the early "ADRs go on main" rule). Mid-stream, the user refined the rule: feature-derived ADRs should ride with the feature on the branch where the work happens; only meta/process ADRs (AD0001-3) belong on main. AD0004 stayed on main as a one-time exception (cost of moving exceeded the cost of leaving). AD0005-8 all landed on feat per the refined rule.

This required a one-time merge of main into feat to make AD0004 visible to feat-branch reviewers, and a documented branch-hygiene insight for future runs.

### Conn/conn_mut accessor stale comments

`Store::conn` and `Store::conn_mut` were declared in T7 with `#[allow(dead_code)]` comments predicting "T9 (store-ingest) and T10 (store-claims) are the first consumers." Neither task actually consumed them — both used direct `self.conn` field access. The comments stayed factually wrong across multiple reviews and accumulated FOLLOWUPS entries. T15's close-out audit moved them to a clear "delete `conn_mut`; refresh `conn` comment" recommendation but didn't execute it (would have expanded T15 scope).

Pattern: predictive allow comments based on planned consumers age poorly when the consumers don't materialize as predicted. Better convention: write the allow without prediction ("currently no bin consumer") and let cleanup happen empirically when something actually consumes it.

### Stale "T13/T14/T15 will" comment accumulation

A related pattern: T7, T8, T9, T10, T11, T12 all added `#[allow(dead_code)]` comments naming future T-tasks as consumers. By T15, several of these had become factually wrong (the named task didn't consume the item; or the item became dead because nothing materialized). T15's close-out audit caught and fixed three (`Config`, `SCHEMA_VERSION`, `SCHEMA_SQL`) but the pattern appeared throughout the run. Same fix as above: don't predict consumers; describe the current state.

### Single brief had a TWO-bug compound (T13)

T13 was the single hardest task to dispatch correctly because the brief had two independent real bugs that only became visible together: `process_watch_entry` called a cfg-gated test helper from production code, AND the idempotence test asserted input-side semantics while the implementation tracked DB-side counters. Either bug alone would have been a single PAUSE; both together required Option-A-vs-B-vs-C user authorization and resulted in two coupled deviations (`Result<usize>` signature change + counter semantic + field rename). The fix produced AD0006 and AD0007 retroactively.

This was the run's strongest argument for "the controller should pre-read the brief and flag known bugs" rather than "trust the brief verbatim."

## Process refinements that emerged mid-run

1. **Pre-dispatch brief audit.** By T13, the controller was reading the brief before dispatch and pre-flagging known bugs with explicit pre-authorized fixes. T14 and T15 dispatches both included pre-authorized deviation lists.

2. **AD0002 cleanup lists in dispatch prompts.** From T9 onward, every dispatch enumerated which allows to REMOVE (now consumed transitively from main) and which to KEEP (still no bin consumer). This prevented misses; the spec reviewer verified completeness.

3. **Sonnet for spec compliance, opus for code quality.** Established by T6/T7 fixup; held for the rest of the run. Cost-effective division.

4. **FOLLOWUPS entries with three-line metadata.** Each entry has **Found in**, **Disposition**, **Trigger to revisit** at the top. Easy to scan when prioritizing Plan B work.

5. **Inline review-derived fixes vs FOLLOWUPS entries.** Trivial one-line cleanups (stale comment text, redundant import) get fixed inline in the followups commit. Anything that requires real thought or scope expansion goes into FOLLOWUPS.

6. **`adg edit` for ADR clarifications.** AD0007 got a wording amendment after T13 review surfaced a precision issue. The decision didn't change; the prose got tighter. This is acceptable for "decided" ADRs as long as the change is honestly disclosed in the commit body.

## Stats and ADR placement

| ADR | Title | Branch | Trigger |
|---|---|---|---|
| AD0001 | Per-task plan split | main | meta/process |
| AD0002 | Dead-code suppression strategy | main | meta/process |
| AD0003 | Test discipline | main | meta/process |
| AD0004 | Transcript output sharding | main (one-time) | T8 feature |
| AD0005 | Cargo test-helpers feature | feat | T9 feature |
| AD0006 | Store mutator Result\<usize\> | feat | T13 feature |
| AD0007 | Stats input-side counters | feat | T13 feature |
| AD0008 | Pipeline artifact-write ordering | feat | T14 feature |

## Recommendations for Plan B planning

1. **Read FOLLOWUPS.md first.** Plan B's scope is largely defined by what Plan A deliberately deferred. The ~20 entries cluster naturally into work areas.

2. **Inherit Plan A's process, refine where needed.** Per-task file split, ADR discipline, sonnet/opus review tier, single-flight lock, FOLLOWUPS file — keep all of these.

3. **Decide bin/lib structural reassessment early.** AD0002's deferred decision (Option 4 thin-binary fat-library, etc.) gates whether several FOLLOWUPS items even need fixing. If Plan B picks Option 4, conn/conn_mut/shard_dir/CommandOutcome.stdout-elapsed all go away naturally.

4. **Failure classification will likely be the load-bearing Plan B work.** Five FOLLOWUPS entries cluster here (T6 RunError mapping, T11 YtDlpFetcher mapping, T12 TranscribeError mapping, T10 mark_succeeded WHERE predicate, T14 Plan B retry/backoff classifier needs typed errors not anyhow). A `RetryableKind` / `UnavailableReason` / `ClassifiedFailure` design is the natural starting ADR.

5. **DDP timestamp timezone is a real correctness gap.** Plan B's first time-window filter will hit it. Either empirically test (compare a known donor's wall-clock to parsed UTC) or document the assumption explicitly. Don't ship Plan B without resolving.

6. **The walking skeleton is alive.** `cargo build` produces a real binary that runs `init → ingest → process` end-to-end. Operators can poke at it now to find what Plan B should prioritize.

## Meta-process improvements identified after Plan A

Three improvements to the planning + dispatch model itself, surfaced by stepping back from the execution mechanics. These should be tried in Plan B and become process ADRs if they prove out.

### 1. Capture coherence decisions at design time, not retrospectively

Plan A's first three ADRs (AD0001 plan-split, AD0002 dead-code strategy, AD0003 test discipline) were all backfilled retroactively after the patterns had emerged across multiple tasks. AD0004 (sharding) and the AD0004-branch-placement reversal happened because the placement rule wasn't recorded when the design was first discussed — only after AD0001/2/3 had landed on main did the user articulate "feature-derived ADRs ride with the feature." By then AD0004 was already on main as a misplaced precedent.

Pattern: when working through the design at session start, the orchestrator should maintain a running list of decision-candidates as they surface — not just architectural decisions, but coherence-maintaining choices like path layout conventions, error handling philosophy, test discipline expectations, dead-code policies, branch hygiene, naming conventions. Capture them as ADRs immediately, before implementation diverges. The "design diagram" (or in Plan A's case, the 3347-line spec) loses fidelity once code starts diverging from intent. A contributor reading only the code can't reconstruct WHY; they can only see WHAT.

The fix is structural: design sessions emit a stack of ADRs alongside the per-task plan files. The plan structure inherits the discipline rather than discovering it.

### 2. Structure tasks for early MVP, not late MVP

Plan A produced an end-to-end runnable thing only at T14/T15 — the last 13% of the work. The actual core value ("scrape one URL, run whisper, output a transcript") was deferred to the very end. Until then, every artifact was scaffolding the operator couldn't critically evaluate.

A better task ordering would have produced a thin-slice MVP first:

- **Epic 1 (3-4 tasks):** Hardcoded `cargo run -- transcribe-one https://...` that takes one URL, calls yt-dlp, calls whisper.cpp, prints the transcript. No database, no ingestion, no sharding. Operator can run this against real videos and form opinions immediately.
- **Epic 2 (3-4 tasks):** Add the state DB and the claim/process loop. Now the prototype processes a list of URLs durably.
- **Epic 3 (3-4 tasks):** Add the DDP-JSON ingest. Now the prototype consumes real donations end-to-end.
- **Epic 4 (3-4 tasks):** Polish (init subcommand, e2e test, init-script, sharding, atomic writes).

Same total work; very different operator-feedback timeline. The user could have run Epic 1's output against real DDP-extracted URLs and surfaced timezone/short-link/provenance/format concerns at week one rather than after the entire foundation was built.

This is the "tracer bullet" / "thin slice" pattern from agile orthodoxy. It also reduces wasted work: foundation built before MVP often turns out to be the wrong shape once real usage starts.

### 3. Curated per-task ADR dispatch, not "read all of them"

Every Plan A implementer/reviewer dispatch told the agent to read ALL the existing ADRs. By T15 that was 8 ADRs per dispatch. Realistically, T13 had no relevance to AD0004 (sharding); T14 had heavy AD0002/0006/0007/0008 relevance but not much AD0001 (plan split). The orchestrator IS the one with full context to know which ADRs apply to each task; it should curate.

Two operational forms:

**Lightweight (start with this):** dispatch prompts include an explicit "ADRs directly relevant to this task" list (e.g., "AD0002 + AD0006; the rest are background"). Reviewer dispatches likewise — the spec reviewer needs to verify compliance against the directly-relevant ADRs; checking all 8 against every task is wasted scan.

**Heavier (only if the lightweight version proves the cost-benefit):** a `docs/decisions/index.yaml` tag scheme (`tags: [bin-lib-asymmetry, error-handling, persistence, observability, ...]`) plus a small `adg query --tags X,Y` lookup, where each per-task brief declares the tags it touches and the dispatch reads only matching ADRs. More machinery, more risk of mis-tagging; only worth it if Plan B grows beyond ~12 ADRs.

The cost in Plan A was real: by T15, opus reviews were spending material context budget on ADR scanning that produced no findings against irrelevant decisions. Plan B will accumulate more ADRs, making the curated approach increasingly valuable.
