# Task 1 — Draft and decide all 9 Epic 1 ADRs via adg

**Goal:** Create the nine ADRs that capture Plan B Epic 1's architectural decisions, with explicit rejected alternatives. Land before any code so downstream tasks reference real ADR IDs.

**ADRs touched (all written):** AD0009 through AD0017, optionally a meta-process ADR on the pass-through rule.

**Why this is task 1:** Per the RETRO meta-process improvement #1, coherence decisions are captured *during* design — not retrospectively. The Plan B spec at `docs/superpowers/specs/2026-05-12-uu-tiktok-pipeline-plan-b-design.md` enumerates the decisions and their rejected alternatives; this task transcribes them into `adg`-managed files.

**Branch convention:** Per the AD0004 branch-placement reversal, feature-derived ADRs (AD0009–AD0017) ride with the feature on `feat/plan-b-...`. The optional meta-process ADR for the pass-through rule, if written, lands on `main`.

**Files:**
- Create: `docs/decisions/AD0009-use-whisper-rs-for-whisper-cpp-embedding-with-version-pin-and-fallback-policy.md`
- Create: `docs/decisions/AD0010-json-artifact-raw-signals-schema-pass-through-with-schema-version.md`
- Create: `docs/decisions/AD0011-spin-down-operational-practice-for-dev-workspace.md`
- Create: `docs/decisions/AD0012-cooperative-cancellation-via-per-request-arc-atomic-bool-and-abort-callback.md`
- Create: `docs/decisions/AD0013-gpu-verification-at-startup-assert-backend-and-log-device-name.md`
- Create: `docs/decisions/AD0014-audio-input-invariant-float32-pcm-16khz-mono-via-hound.md`
- Create: `docs/decisions/AD0015-explicit-non-use-of-whisper-full-parallel.md`
- Create: `docs/decisions/AD0016-architecture-for-parallelism-engine-api-stable-across-single-and-multi-state.md`
- Create: `docs/decisions/AD0017-operational-done-contract-for-batch-validation.md`
- Optionally create on `main`: `docs/decisions/AD00XX-pass-through-not-pre-aggregation-meta-process-rule.md`

---

- [ ] **Step 1: Verify `adg` is available and check current ADR state**

Run:
```bash
which adg && adg list --model docs/decisions
```

Expected: `adg` resolves to a path on `PATH`, and the list shows AD0001 through AD0008 as `decided`.

If `adg` is not on PATH: it's likely installable via `pip install adg`. Check the project's existing ADRs at `docs/decisions/` for the conventional invocation — Plan A used `adg` exclusively.

- [ ] **Step 2: Create AD0009 — Use whisper-rs (out-of-tree binding) for whisper.cpp embedding**

Run:
```bash
adg add --model docs/decisions --title "Use whisper-rs for whisper.cpp embedding with version-pin and fallback policy"
```

Then edit the generated AD0009 file. The decision text should capture:

- **Question:** How do we embed whisper.cpp in the Rust binary? Plan A shells out to `whisper-cli`, which reloads the model per invocation — the dominant inefficiency. Plan B requires per-video confidence signals (token p/plog, segment no_speech_prob) that `whisper-cli`'s `--output-json-full` does not emit (sharp-edges.md:39). We need a path that exposes the C API.
- **Considered options:**
  1. `whisper-rs` (out-of-tree Rust binding, recommended by whisper.cpp README at line 703) with the `cuda` feature
  2. Custom CGO/FFI binding written in this repo
  3. Other Rust bindings (none currently mature or actively maintained)
  4. Patch `whisper-cli`'s JSON writer to emit `no_speech_prob` (a few lines) and stay with the subprocess pattern
  5. Run `whisper-server` locally and call it over HTTP
- **Decision:** Option 1 (whisper-rs). Pin both the crate version *and* the whisper.cpp commit it tracks (whisper-rs exposes this via `WHISPER_VERSION` constant; record the exact value).
- **Fallback decision rule (explicit, not vague):** If the `whisper-rs` CUDA build fails on the A10 workspace after one debugging cycle (≤1 day of investigation), fall back to Option 4 (patch `whisper-cli` for confidence signals and stay with the subprocess approach). Document the fallback as a separate ADR superseding AD0009 if invoked. Do NOT fall back to Options 2, 3, or 5 — they're worse engineering bets.
- **Rejected alternatives** rationale: Option 2 (custom FFI) — duplicates work that whisper-rs already does well; high maintenance burden. Option 3 — none mature enough as of 2026-05-12; whisper.cpp README explicitly points to whisper-rs. Option 4 — fork maintenance of whisper.cpp is strictly worse than depending on a maintained binding. Option 5 — adds HTTP framing overhead and a separate daemon to supervise; the deepdive's `concurrency.md` notes the server serializes inference via `std::mutex`, so it offers no concurrency benefit on one GPU.

Use `adg edit` and `adg decide` to fill the fields:

```bash
adg edit --model docs/decisions --id 0009 \
  --question "How do we embed whisper.cpp in the Rust binary, capture per-video confidence signals not emitted by whisper-cli's JSON output, and avoid the per-invocation model load that dominates Plan A's CPU runtime?" \
  --option "whisper-rs (out-of-tree Rust binding) with cuda feature, version-pinned to crate version + whisper.cpp commit" \
  --option "Custom CGO/FFI binding written in this repo" \
  --option "Other Rust bindings" \
  --option "Patch whisper-cli's JSON writer for no_speech_prob and stay with subprocess pattern (fallback only)" \
  --option "Run whisper-server locally and call over HTTP" \
  --criteria "Per-video confidence signals must be captured (token p/plog, no_speech_prob). Model load amortized across a batch. Maintenance cost manageable on a 1-developer project. Fallback path identified if CUDA build fights us. Architecture future-proofs multi-state and multi-GPU per AD0016."

adg decide --model docs/decisions --id 0009 \
  --option "whisper-rs (out-of-tree Rust binding) with cuda feature, version-pinned to crate version + whisper.cpp commit" \
  --rationale "The C API exposes everything we need (token p/plog, no_speech_prob, language); whisper-rs wraps it 1:1; the README points at it; it actively tracks upstream. Pin the version (both crate and whisper.cpp commit) to keep behavior reproducible across SRC workspace re-provisions. If CUDA build fails after one day of investigation, fall back to Option 4 (patch whisper-cli) as documented in a superseding ADR — do not fall back to custom FFI or HTTP."
```

- [ ] **Step 3: Create AD0010 — JSON artifact raw_signals schema (pass-through + schema_version)**

Run:
```bash
adg add --model docs/decisions --title "JSON artifact raw_signals schema pass-through with schema-version"
```

Decision text captures:

- **Question:** Plan B captures per-video confidence signals (token p/plog, segment no_speech_prob, language) for downstream analysis. How do we shape the JSON artifact: pre-aggregate to scalars, or pass through whisper.cpp's raw output as-is? And how do we make the schema evolvable?
- **Considered options:**
  1. **Pass-through** raw signals as whisper.cpp emits them (per-segment arrays of per-token data); add `schema_version: "1"` to a new `raw_signals` object on the existing `{video_id}.json`
  2. **Aggregate** to per-video scalars (mean log-p, fraction-below-threshold, language confidence)
  3. **Both** (aggregate scalars alongside raw data) for query convenience
  4. **Separate file** (`{video_id}.raw_signals.json`) instead of extending the existing metadata file
- **Decision:** Option 1. Strict pass-through; `schema_version: "1"` on a new `raw_signals` sub-object of `{video_id}.json` (additive — existing fields preserved).
- **Pass-through rule (formalized):** *Raw pass-through is canonical for research signals; only compute summaries needed for pipeline operation, indexing, or cheap sanity checks.* This rule applies to Plan B and Plan C. Operational metrics (e.g., a `transcript_empty: bool` for `status` subcommand efficiency) are permissible; speculative research aggregation is not.
- **`lang_probs` is opt-in** via `--compute-lang-probs` config flag (verified against whisper-rs API; the `WhisperState::lang_detect` call re-encodes audio per sharp-edges.md:13). Default null; opt-in pays one extra encoder pass per video.
- **Rejected alternatives:** Option 2 — speculative; downstream consumers haven't asked for any specific aggregation; YAGNI applied to data. Option 3 — doubles field count without value at current scope. Option 4 — fragments the per-video artifact set; complicates `output::shard_path` callers; no benefit.

Use `adg edit` and `adg decide`:

```bash
adg edit --model docs/decisions --id 0010 \
  --question "How do we shape the per-video JSON artifact to carry whisper.cpp's confidence signals without speculative aggregation?" \
  --option "Pass-through raw signals (per-segment arrays of per-token data); schema_version on a new raw_signals sub-object" \
  --option "Aggregate to per-video scalars (mean log-p, fraction-below-threshold, language confidence)" \
  --option "Both aggregate scalars alongside raw data" \
  --option "Separate file (raw_signals.json) instead of extending metadata.json" \
  --criteria "Don't speculatively compute downstream-derived metrics. Per-video confidence required. lang_probs not freely available from whisper_full (sharp-edges.md:13); needs opt-in. Schema must be evolvable. Per-video artifact set stays compact and sharded."

adg decide --model docs/decisions --id 0010 \
  --option "Pass-through raw signals (per-segment arrays of per-token data); schema_version on a new raw_signals sub-object" \
  --rationale "The user explicitly stated 'pass-through, not pre-aggregation' for research signals; this ADR codifies the rule. Downstream consumers compute aggregations if they want them. schema_version starts at 1 and extends additively. lang_probs is opt-in because the call re-encodes the audio."
```

- [ ] **Step 4: Create AD0011 — Spin-down operational practice for dev workspace**

```bash
adg add --model docs/decisions --title "Spin-down operational practice for dev workspace"
```

Decision text captures:

- **Question:** Plan A's prior SRC deployment burned ~133 CPU-core-hours over 2.5 idle days. The dev grant's 15K CPU-core-h budget cannot accommodate continuous workspace running over 12 months. What is the canonical operational practice for stopping the workspace between batches?
- **Considered options:**
  1. **Pause** via SRC portal between every working session
  2. **Delete** between every session; re-provision when needed
  3. **Always-on** workspace; accept the burn rate
  4. **Auto-pause** via SRC's scheduled actions (if available)
- **Decision:** Option 1 (Pause). On grant-based wallets (which Workstream 1's wallet is), pause charges zero CPU/GPU AND zero storage (`workspace-lifecycle.md:17-20`). Resume reattaches storage and restarts the workspace.
- **Operational checklist** (documented in this ADR):
  - Before pause: stop all active batches (`Ctrl+C` the running `process` command); verify no users logged in; confirm no `in_progress` rows remain (Epic 4's `status` subcommand will validate this). Without these checks the pause may leave the workspace in unpredictable state.
  - After resume: verify SSH works; verify `nvidia-smi` shows the A10; verify the external storage volume is mounted at `~/data/<volume-name>`; rebuild any state lost from local disk (state.sqlite if it lived locally).
- **Rejected alternatives:** Option 2 (Delete) — would force re-provisioning every session (~10–15 min) and reinstalling all software; high friction. Option 3 (Always-on) — incompatible with the budget math. Option 4 (Auto-pause) — investigate later; manual pause is sufficient and simpler for Epic 1.
- **Cross-reference to AD0017** (Operational "done" contract): the `status` subcommand from Epic 4 implements the "safe to pause" check.

- [ ] **Step 5: Create AD0012 — Cooperative cancellation via per-request Arc<AtomicBool>**

```bash
adg add --model docs/decisions --title "Cooperative cancellation via per-request Arc AtomicBool and abort callback"
```

Decision text:

- **Question:** Plan A's `whisper-cli` subprocess could be killed via SIGTERM/SIGKILL with bounded latency. Embedded whisper-rs runs inside our process and cannot be killed externally. How do we implement per-call timeout and operator-initiated cancellation?
- **Considered options:**
  1. **Per-request `Arc<AtomicBool>` flag** built fresh per transcribe call, dropped with the request; polled by `FullParams::abort_callback`. Worker thread sets the flag when the deadline elapses.
  2. **Engine-level cancellation flag** (single `Arc<AtomicBool>` on the `WhisperEngine` struct, reset per call)
  3. **No cancellation in Epic 1**; accept that inference can hang past its budget
- **Decision:** Option 1. Per-request flag eliminates the cross-request leak codex-advisor flagged in the second-pass review: a late timeout from request A cannot cancel request B if the flag belongs to A and is dropped with A.
- **Mechanics:** Each `TranscribeRequest` carries `cancel: Arc<AtomicBool>` and `deadline: Instant`. The worker thread spawns a separate timer that flips `cancel` when `Instant::now() > deadline`. The `FullParams::abort_callback` polls `cancel`. On flip-to-true, whisper-rs returns from `whisper_full_with_state` and the worker replies `Err(TranscribeError::Cancelled)`.
- **Epic 1 fail-fast posture:** `Cancelled` propagates up through `pipeline::process_one` and the process exits non-zero (matches Plan A's behavior on transcribe failure). Epic 2's state-machine work reclassifies Cancelled into proper retryable/terminal columns.
- **Rejected alternatives:** Option 2 — codex-advisor flagged the race condition; cancellation must not leak across requests. Option 3 — leaves a hung process if whisper.cpp enters a pathological state.

- [ ] **Step 6: Create AD0013 — GPU verification at startup**

```bash
adg add --model docs/decisions --title "GPU verification at startup assert backend and log device name"
```

Decision text:

- **Question:** Per sharp-edges.md:60-61, whisper.cpp silently falls back to CPU at ~100× slower throughput if GPU backend initialization fails, and `gpu_device = N` silently picks the wrong GPU if `CUDA_VISIBLE_DEVICES` is misconfigured. How do we prevent the bake from being meaningless due to silent CPU fallback?
- **Considered options:**
  1. **Assert at `WhisperEngine::new`**: scan the `tracing` log emitted by whisper.cpp's init for "using <backend> backend" and abort the process if it's not the expected CUDA backend; log the `gpu_device` index and reported device name
  2. **Defer to bake-time verification only** (operator inspects logs manually during bake)
  3. **No verification** — trust that the build flags worked
- **Decision:** Option 1 (assert at init). The cost is small (parse one log line at startup); the value is large (catches silent CPU fallback that would invalidate every benchmark and waste a workspace session).
- **Mechanics:** whisper-rs emits an init log via the C library's `whisper_log_set` callback. `WhisperEngine::new` wires a callback that captures the backend identifier and device name, asserts the backend matches the expected (CUDA when `cuda` feature is enabled), and emits a `tracing::info!` line with the captured values. If mismatch, return `WhisperInitError::BackendMismatch` and abort.

- [ ] **Step 7: Create AD0014 — Audio-input invariant: float32 PCM 16 kHz mono via hound**

```bash
adg add --model docs/decisions --title "Audio-input invariant float32 PCM 16kHz mono via hound"
```

Decision text:

- **Question:** whisper.cpp's C API takes float32 PCM at 16 kHz mono (api-and-pipeline.md:7). Plan A produces 16 kHz mono WAV via yt-dlp's ffmpeg postprocessor (`--postprocessor-args "ffmpeg:-ar 16000 -ac 1"`) and hands the file path to `whisper-cli`. Embedding requires decoding the WAV in-process. What decoder and what validation?
- **Considered options:**
  1. **`hound` crate** (small, focused on PCM WAV; no `no_std` dependency complications) with explicit format validation
  2. **`symphonia` crate** (general audio decoding; supports MP3/FLAC/etc.; heavier dependency)
  3. **Custom WAV parser** (reinvent the wheel)
  4. **`ffmpeg` via subprocess** (yt-dlp already used this; we'd shell out again)
- **Decision:** Option 1 (`hound`). Validate the WAV header on every load: `sample_rate == 16000`, `channels == 1`, sample format is `f32` (or `i16` converted to `f32`). Reject non-conforming inputs with a typed error.
- **Mechanics:** A small `decode_wav(path: &Path) -> Result<Vec<f32>, AudioDecodeError>` helper. Reads the WAV header, validates format, decodes samples. Returns owned `Vec<f32>` ready to ship across the worker boundary (per worker-thread invariants from AD0012).
- **Rejected alternatives:** Option 2 — overkill for our pinned input format. Option 3 — error-prone; hound is well-tested. Option 4 — adds subprocess overhead and re-introduces the dependency we're trying to remove.

- [ ] **Step 8: Create AD0015 — Explicit non-use of whisper_full_parallel**

```bash
adg add --model docs/decisions --title "Explicit non-use of whisper-full-parallel"
```

Decision text:

- **Question:** whisper.cpp's `whisper_full_parallel` (whisper.cpp:7891) is named as if it parallelizes inference. Should we use it?
- **Considered options:**
  1. **No** — it splits one audio across N states with documented quality loss at chunk boundaries (sharp-edges.md:45); not a parallel-transcription tool
  2. **Yes** — use it for short audio where chunk-boundary quality loss is acceptable
- **Decision:** Option 1. Documented as non-decision so future readers don't reach for it under the assumption it's the right tool. For per-video parallelism we use multiple `WhisperState`s on one context (concurrency.md); for multi-video parallelism we use channel-based orchestration (Epic 2).
- **Rationale:** Cited verbatim from sharp-edges.md:45 — "the transcription quality may be degraded near these boundaries." For research data we cannot accept this quality loss.

- [ ] **Step 9: Create AD0016 — Architecture for parallelism (Engine API stable across single/multi-state)**

```bash
adg add --model docs/decisions --title "Architecture for parallelism Engine API stable across single and multi-state"
```

Decision text:

- **Question:** Plan B targets a single A10 for dev grant cost. Downstream production (researcher's separate grant) will use multi-state and/or multi-GPU. How do we architect Epic 1's `WhisperEngine` so the production upgrade is a swap-in change, not a rewrite?
- **Considered options:**
  1. **Stable public API; mutable internals.** `engine.transcribe(samples, cfg).await` returns one result per call regardless of internal parallelism. Epic 1 ships single (context, state, worker thread). Plan C either (a) upgrades Engine internals to `(context, Vec<state>, Vec<worker thread>, dispatcher)` or (b) wraps `WhisperPool` of N Engines with routing dispatcher.
  2. **Pool from day one** — implement `WhisperPool` in Epic 1 with N=1 trivially routed
  3. **Defer entirely** — single-Engine for Plan B; rewrite when production needs multi
- **Decision:** Option 1. The public API stays stable across single/multi-state internals; configuration plumbing anticipates with `gpu_devices: Vec<i32>` and `states_per_gpu: usize` (Epic 1 defaults `gpu_devices=[0]`, `states_per_gpu=1`).
- **Worker-thread invariants** (relevant to the upgrade path):
  - Only owned data crosses the worker boundary (`Vec<f32>` samples, owned config, owned output structs)
  - `WhisperContext` / `WhisperState` and any reference types stay inside the worker
  - A closed oneshot reply is Bug-class
- **Rejected alternatives:** Option 2 — premature; YAGNI for Epic 1. The pool abstraction has cost (routing logic, fairness) we don't need at N=1. Option 3 — would force a rewrite later, violating "architect for parallelism" guidance from the user.

- [ ] **Step 10: Create AD0017 — Operational "done" contract for batch validation**

```bash
adg add --model docs/decisions --title "Operational done contract for batch validation"
```

Decision text:

- **Question:** When can an operator declare a batch "done" and safe to spin down the workspace? Plan A's exit-3 mechanism (process returned 3 = nothing to claim) is insufficient — it doesn't verify artifacts on disk or schema compliance.
- **Considered options:**
  1. **Define the contract in this ADR; implement in Epic 4's `status` subcommand.** Contract = counts by status (all terminal), all `succeeded` rows have artifacts on disk, all `raw_signals.schema_version` match expected.
  2. **Implement in Epic 1.** Adds scope to Epic 1.
  3. **Don't define until Epic 4.** Risk: implementer of Epic 4 has no contract to fulfill.
- **Decision:** Option 1. The ADR is drafted now (Epic 1) so the `status` subcommand has a clear contract to implement. The subcommand itself lands in Epic 4.
- **Contract (formal):**
  - **Counts by status**: every row in `videos` has terminal status (no `in_progress`, no `pending` unless explicitly skipped via `--max-videos`)
  - **Artifact existence**: every `succeeded` row has `.txt` and `.json` at the sharded path
  - **Schema-version check**: every `.json`'s `raw_signals.schema_version` matches `EXPECTED_RAW_SIGNALS_SCHEMA_VERSION`
  - **Optional**: artifact backup to Research Drive completed (if configured)
  - **Pause-safe**: all the above pass AND no `in_progress` rows pending recovery
- **Cross-references:** AD0011 (spin-down practice) consumes the pause-safe check.

- [ ] **Step 11: (Optional) Create the pass-through meta-process ADR**

Decide whether the "pass-through, not pre-aggregation" rule deserves its own meta-process ADR alongside AD0001–3 (which are also meta/process). Arguments for:
- The rule generalizes beyond Plan B Epic 1 (Plan C will face the same speculative-aggregation pressure for comments, video metadata, etc.).
- Captured in the spec but easy to lose track of if it's not an ADR.

Arguments against:
- Already documented in AD0010 (raw_signals schema). May be redundant.

If yes, create on `main` (not feat). Title: `Pass-through not pre-aggregation rule for derived data`. Decision content mirrors the spec's framing decision verbatim.

If no, add a TODO entry to FOLLOWUPS that says "consider promoting AD0010's pass-through rule to a meta-process ADR if it surfaces in Plan C as a recurring pattern."

The default (if unsure): skip for now; let the rule prove itself in Plan C before formalizing.

- [ ] **Step 12: Run `adg list --model docs/decisions` to verify all 9 (or 10) ADRs are decided**

Run:
```bash
adg list --model docs/decisions
```

Expected: AD0001–AD0008 still `decided`, AD0009–AD0017 now `decided`, optionally a meta-process ADR also `decided` on a separate branch.

- [ ] **Step 13: Commit**

```bash
git add docs/decisions/AD0009-*.md docs/decisions/AD0010-*.md docs/decisions/AD0011-*.md docs/decisions/AD0012-*.md docs/decisions/AD0013-*.md docs/decisions/AD0014-*.md docs/decisions/AD0015-*.md docs/decisions/AD0016-*.md docs/decisions/AD0017-*.md docs/decisions/index.yaml
git commit -m "$(cat <<'EOF'
docs(adr): draft Epic 1 ADRs AD0009-AD0017

Captures all nine Plan B Epic 1 architectural decisions before code
lands, per the RETRO meta-process improvement (capture coherence
decisions during design, not retrospectively).

- AD0009 use whisper-rs with version-pin + explicit fallback rule
- AD0010 JSON raw_signals schema pass-through with schema_version
- AD0011 spin-down operational practice (grant-wallet pause = $0)
- AD0012 cooperative cancellation via per-request Arc<AtomicBool>
- AD0013 GPU verification at startup (no silent CPU fallback)
- AD0014 audio-input invariant: float32 PCM 16kHz mono via hound
- AD0015 explicit non-use of whisper_full_parallel
- AD0016 architecture for parallelism (Engine API stable across single/multi-state)
- AD0017 operational "done" contract for batch validation (Epic 4 implements)

Cross-references each ADR to the Plan B design spec section that
motivated it. Rejected alternatives recorded for each per AD0001's
template precedent.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If the optional meta-process ADR was written, commit separately on `main` (per AD0004's branch-placement rule).

---

## Self-check before claiming completion

- [ ] All 9 (or 10) ADRs exist as `decided` status in `docs/decisions/`
- [ ] Each ADR enumerates the rejected alternatives with rationale
- [ ] AD0009 includes the explicit fallback decision rule (not vague "CLI patching")
- [ ] AD0010 includes the pass-through rule wording verbatim from the spec
- [ ] AD0016's worker-thread invariants are written so T5 (engine shell) can reference them
- [ ] Commit message lists all created files

If the implementer encountered a multi-alternative decision NOT enumerated in the spec, pause and report back — per Plan A's authorship convention, controller writes ADRs, not subagents.
