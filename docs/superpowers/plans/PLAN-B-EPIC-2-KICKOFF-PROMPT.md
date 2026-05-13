# Plan B Epic 2 kickoff prompt — paste into a fresh Claude Code session when ready

> **Author note:** Plan B Epic 1 (efficiency-first refactor: embedded whisper-rs, raw-signals pass-through, CUDA bake) is merged to `main`, and two post-bake corrections also landed (yt-dlp format selector — PR #2; curl-cffi impersonation resolution — commit `ac321e4`). A second short A10 run on 2026-05-13 verified the pipeline flows end-to-end on real Dutch/English/Tagalog donor content at 8/8 success with `large-v3-turbo-q5_0`. The empirical state of `main` is now properly grounded rather than nominally asserted. Epic 2 builds the minimum state machine + pipelined orchestrator on top.

---

## Prompt to paste

I want to begin planning **Plan B Epic 2** for the UU TikTok donation-data transcription pipeline (`/home/dmm/src/uu-tiktok`). Plan B Epic 1 — the efficiency-first refactor (embedded whisper-rs + raw signals + CUDA bake) — is complete on `main`, with two post-bake corrections also landed. I have empirical numbers from a fresh post-fix A10 run and observations about what's load-bearing for Epic 2.

### Step 1: Orient yourself before discussing scope

Read these in order:

1. `docs/superpowers/plans/2026-05-12-plan-b/00-overview.md` — Plan B Epic 1's scope and "What Epic 1 Deliberately Omits" list (Epic 2-deferred items appear there)
2. `docs/superpowers/plans/2026-05-12-plan-b/EPIC-2-SKETCH.md` — three-sub-phase sketch (schema-version → state-machine → pipelined orchestrator) + anticipated ADRs AD0018-AD0023
3. `docs/SRC-BAKE-NOTES.md` — Epic 1's empirical baseline including the 2026-05-13 update footnote on Finding 1 and Finding 2 resolutions
4. `docs/FOLLOWUPS.md` — current state with post-bake corrections applied; identify which entries are Epic 2 territory vs. Epic 3+ (the yt-dlp residual-retry entry is explicitly Epic 3 even though it tempts Epic 2 scope)
5. `docs/decisions/AD0001-*` through `AD0017-*` — every architectural decision from Plan A through Plan B Epic 1. Mark which are touched / composed-with / superseded by Epic 2.
6. Current state of `src/` on `main` (5d0e44d or later) — what actually exists today, especially `src/state/`, `src/pipeline.rs`, `src/process.rs`, and the `WhisperEngine` API surface in `src/transcribe.rs`

Do NOT yet read the full Plan B design spec at `docs/superpowers/specs/2026-05-12-uu-tiktok-pipeline-plan-b-design.md` — Epic 1's overview file already extracts what Epic 2 needs. The spec is too long for the planning conversation's context budget.

### Step 2: Brainstorm Epic 2's actual scope with me

Use the `superpowers:brainstorming` skill. Anchor questions for the brainstorm:

1. **Does the EPIC-2-SKETCH's three-sub-phase ordering still hold?** Schema-version → state-machine → pipelined orchestrator. The sketch was written before Epic 1's bake; the empirical evidence and post-bake fixes may have refined the urgency ordering. Confirm or revise.

2. **What's the smallest Epic 2 that makes Plan B operationally recoverable?** Epic 1 produces transcripts but its failure modes leave rows stuck — no `mark_succeeded` WHERE-predicate, no stale-claim sweep, no retryable/terminal classification. Epic 2's MVP should answer the operator question: *"what do I do when a fetch fails mid-batch?"*

3. **What does the bake's empirical evidence tell us about orchestrator sizing?** See the "Empirical anchors" section below. Fetch is dominant (avg ~5.5s, with 21s outliers); transcribe is sub-2s on `large-v3-turbo-q5_0`. The naive read says "2-3 fetch workers + 1 transcribe worker." Don't pre-decide — let the brainstorm derive the right configuration and document the rationale. The 21s outlier in particular matters for bounded-mpsc capacity and backpressure policy.

4. **Which FOLLOWUPS entries land as Epic 2 sub-tasks vs. defer further?** The sketch claims schema-version handling (T7 FOLLOWUPS), bounded `process::run` capture (T6), `concurrent_claim` test fix, `mark_succeeded` WHERE-predicate (T10) are all Epic 2 scope. Confirm. The yt-dlp retry-on-no-audio FOLLOWUPS entry is NOT — it's Epic 3's failure-classification scope, and Epic 2's mutator design must compose cleanly with Epic 3's typed-enum work without locking it in.

5. **What does Plan B Epic 2 explicitly defer to Epic 3+?** Per the sketch and EPIC-3/4/5 sketches: typed-enum failure classification (Epic 3), time-window filter on `watch_history.watched_at` (Epic 4), `status` subcommand (Epic 4), bin/lib reassessment per AD0002 (Epic 5). Confirm none of these slip into Epic 2 by accident.

### Step 3: Re-confirm process inheritance from Epic 1

Same disciplines as Plan B Epic 1 (per `PLAN-B-KICKOFF-PROMPT.md` Step 3 + the Plan A → Plan B inheritance it documents):

- **Per-task file split** (AD0001) — Epic 2's directory: `docs/superpowers/plans/<date>-plan-b-epic-2/`. One self-contained brief per task plus `00-overview.md`.
- **ADR discipline** — `adg` tool; meta/process ADRs on `main`, feature-derived ADRs on the feat branch. Read AD0009-AD0017 and respect them unless explicitly superseding (in which case a new ADR with `succeeds: ["NNNN"]`).
- **AD0002 cleanup-on-consumption** — every dispatch enumerates `#[allow(dead_code)]` items to add or remove.
- **AD0003 deviation-honesty** — clippy fixes, brief-verbatim deviations, and structural choices get prominent commit-message disclosure.
- **AD0006 mutator signature convention** — every new `Store` mutator returns `Result<usize>` with row-change count (especially `mark_retryable_failure`, `mark_terminal_failure`, the stale-sweep mutator, `mark_succeeded`'s tightened predicate variant).
- **AD0007 stats counter convention** — input-side counters with verb-named fields; HashSet pattern for uniques.
- **AD0008 artifact-write ordering** — DB acknowledges success ONLY after artifacts are durable. Any new pipeline mutator (partial-result persistence, retry-state writes) MUST preserve this invariant.
- **AD0005 test-helpers feature** — every new integration test in Cargo.toml's `[[test]] required-features = ["test-helpers"]`.
- **FOLLOWUPS discipline** — resolved entries deleted (git history retains); new entries added during reviews.

### Step 3a: New meta-process discipline captured from Epic 1's post-bake close-out

In addition to the three Plan B meta-process improvements (capture ADRs during brainstorm not retroactively; structure tasks for early MVP; curated per-task ADR dispatch — see `PLAN-B-KICKOFF-PROMPT.md` Step 3a), Epic 2 should apply one more discipline that emerged from Epic 1's post-bake close-out (2026-05-13):

**Mark unverified hypotheses explicitly in FOLLOWUPS.** When a FOLLOWUPS entry records a hypothesis that was not empirically verified at write-time, prefix the hypothesis with `**Hypothesis (unverified):**`. The post-bake close-out caught two cases where bake-time hypotheses were carried forward as if confirmed: applying the proposed fix would have either failed (yt-dlp `-f "ba/b"` was structurally wrong against `tiktok.py`'s `_extract_web_formats`) or solved the wrong problem (`libcurl4-openssl-dev` install was a red herring; real cause was curl-cffi 0.15.0 outside yt-dlp 2026.03.17's supported range). The structural fix is diagnostic-before-fix discipline at FOLLOWUPS-write time: when the bake doesn't run the diagnostic, mark the entry so the next operator knows to verify before acting.

### Step 4: Subagent dispatch model

Same as Plan B Epic 1 — refer to that kickoff's Step 4 for the full discipline. Specifically for Epic 2:

- **Opus implementer** for the state-machine + orchestrator tasks (high multi-subtle-interaction surface — tokio `JoinSet`, mpsc backpressure, schema migration semantics, stale-claim race conditions).
- **Sonnet implementer** for mechanically tractable tasks (schema-version check at `Store::open`, individual mutator additions with the AD0006 signature, bounded-buffer test additions).
- **Sonnet spec-compliance reviewer** across the board (mechanical brief-conformance check).
- **codex-advisor code-quality reviewer** per-task via the pinned session (independent intra-model perspective on race conditions, lifetime/Send/Sync hazards, perf footguns, testing gaps).
- **Single-flight Agent dispatch** still enforced by `~/.claude/hooks/agent-lock-acquire.sh` (the laptop's thermal lock).

### Step 5: Hand control back to me before dispatching anything

After the brainstorm and before writing the per-task plan files: surface your understanding of Epic 2's scope, scoped omissions, and proposed task-list structure. Wait for my approval. THEN use `superpowers:writing-plans` to draft the per-task expansion.

The sketch projects ~8-10 tasks; the brainstorm may right-size up or down based on empirical anchors and operational MVP-first logic.

### What NOT to do

- Do NOT pre-decide the orchestrator topology (worker count, channel capacity, polling/backpressure policy) before the brainstorm derives it from the bake numbers. Generic-good-defaults intuition is not a substitute for the data.
- Do NOT reorder the sub-phases without explicit justification. The sketch's order (schema-version → state-machine → orchestrator) has reasoning: schema-version handling must precede the schema-changing state-machine work, and the state-machine's mutator surface must exist before the orchestrator depends on retry/terminal classification.
- Do NOT fold Epic 3 retry-on-no-audio work into Epic 2 even though the yt-dlp FOLLOWUPS entry tempts it. Epic 3 owns the typed-enum failure-classification work; Epic 2 introduces the minimum mutator signatures (`mark_retryable_failure` / `mark_terminal_failure` with `(kind: &str, message: &str)` shape) that Epic 3 composes typed enums on top of.
- Do NOT re-litigate AD0009-AD0017. If one needs revisiting, propose superseding it via a new ADR with `succeeds: ["NNNN"]` and explicit reasoning.
- Do NOT skip the FOLLOWUPS audit, but note that Epic 1's post-bake work already resolved several entries (yt-dlp format-selector, curl-cffi install). The audit should focus on what's left for Epic 2 vs. Epic 3+.
- Do NOT propose merging Epic 2 to main without explicit go-ahead. Epic 2 likely lives on a feat branch through completion, mirroring Plan A → Plan B Epic 1.

### Done state for this kickoff session

By end of session:

- Brainstorm conversation captured (what Epic 2 does and doesn't do, anchored in bake empirics)
- ADR drafts started for major Epic 2 decisions (roughly AD0018-AD0023 per the sketch; refine during brainstorm — some may merge, others may split)
- Epic 2's per-task file structure drafted at `docs/superpowers/plans/<date>-plan-b-epic-2/`
- Updated `docs/FOLLOWUPS.md` if any entries are explicitly resolved-by-Epic-2-scope or moved to Epic 3+
- I have a clear next-action: either approve and start dispatching, or iterate on the plan

### Empirical anchors (so the brainstorm doesn't fish for them)

These are the load-bearing numbers Epic 2's orchestrator design should reference. Captured from the 2026-05-13 post-fix A10 run on the `news_orgs` fixture (commit `ac321e4` then `5d0e44d` for the verification record):

**Sequential per-video budget (large-v3-turbo-q5_0, n=8):**
- Total wallclock: 54 s for 8 unique URLs ≈ 6.75 s/video average
- Fetch range: 1.7–21 s (single 21s outlier on `@gmanews/7636791907133164808` — 39% of total run wallclock on one fetch)
- Transcribe range: 0.27–2.0 s, mostly sub-second; near the GPU floor at 49× realtime
- Model load (large-v3-turbo): 6.1 s — one-time per process invocation; amortizes to zero in a long-running daemon
- DB ops + artifact writes per video: ~50–200 ms

**Selector + impersonation state (post-fix):**
- yt-dlp format selector: `download/b[vcodec=h264]/b` — 20/20 `download` hit rate on news_orgs fixture
- Working impersonation: 25+ curl_cffi targets `(available)` on the A10 workspace
- Both fixes verified on real Dutch / English / Tagalog content with high-confidence language detection (7/8 ≥ 98%)

**Resource envelope per A10 workspace:**
- 1 NVIDIA A10, 24 GB VRAM, compute capability 8.6
- 4–8 CPU cores typical
- Memory per WhisperState: ~250 MB; model VRAM for large-v3-turbo-q5_0: 573 MB; total per-engine instance ~1 GB single-state, ~1.25 GB with lang_state per AD0012
- Plenty of VRAM headroom for multi-state or multi-engine experiments at production grant time (Plan C)

**What this implies for Epic 2's orchestrator (not a decision, an input):**

Fetch-dominant per-video budget + transcribe-near-floor on the recommended production model means parallelism on fetch is the larger lever than parallelism on transcribe. Two or three concurrent fetch workers feeding a single transcribe worker would amortize most of the 21s-outlier-class behavior without touching whisper's single-state assumption (AD0015 / AD0016). The brainstorm should derive the actual numbers, but this framing is the empirical starting point — not received wisdom.

---

## Related artifacts to reference if questions arise

- The original Plan B kickoff prompt: `docs/superpowers/plans/PLAN-B-KICKOFF-PROMPT.md` — the structural template this prompt mirrors, plus the Plan A → Plan B inheritance documentation.
- Plan A's RETRO (referenced from the original kickoff): captures the per-task-file-split, the bin/lib reassessment deferral, the dispatch-cost-tier model.
- The Epic 1 overview's "Self-Review Checklist": the shape Epic 2's per-task plan should pass at the end.

---

[End of kickoff prompt]
