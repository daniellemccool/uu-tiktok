# Task 13 — A10 bake runbook (extends SRC-BAKE-CHECKLIST.md Phase 7) + SRC-BAKE-NOTES.md

**Goal:** Operator runbook for the SRC A10 bake. Verifies the whisper-rs pipeline works on real hardware, captures performance numbers across model sizes, exercises the 1-state vs 2-state measurement, validates GPU backend (no silent CPU fallback), and writes `docs/SRC-BAKE-NOTES.md`. Operational task; no source-code change beyond the test invocation.

**ADRs touched:** AD0011 (spin-down), AD0013 (GPU verification at startup), AD0015 (no whisper_full_parallel), AD0016 (parallelism architecture — bake measures 1-state vs 2-state), AD0017 ("done" contract — partial implementation via bake notes).

**Where this runs:** On the provisioned SRC A10 workspace (not the dev laptop). Operator follows the steps SSH'd into the A10.

**Pre-conditions:**
- SRC A10 workspace provisioned and SSH-accessible (separate task; see `SRC-BAKE-CHECKLIST.md` Phase 0)
- External storage volume attached at `~/data/<volume-name>`
- `cargo`, `cmake`, `gcc`/`clang`, CUDA toolkit on the workspace (Phase 1)
- Network egress to TikTok CDN works (Phase 2 of SRC-BAKE-CHECKLIST.md)
- `state.sqlite` mountpoint chosen per Phase 3 (likely local disk)
- Models accessible (Phase 4)

**Files:**
- Create: `docs/SRC-BAKE-NOTES.md`
- Modify (optional): `docs/SRC-BAKE-CHECKLIST.md` (mark Phase 7 items checked off after bake)

---

## Setup on the A10 workspace

- [ ] **Step 1: Clone the repo to the external storage volume**

SSH into the A10 workspace. Replace `<volume>` with the actual external storage volume name (visible via `mount | grep data`).

```bash
cd ~/data/<volume>
git clone <repo-url> uu-tiktok
cd uu-tiktok
git checkout feat/plan-a-walking-skeleton  # or whichever branch holds Plan B Epic 1
```

- [ ] **Step 2: Download the model files**

```bash
mkdir -p models
./scripts/fetch-tiny-model.sh
# Plus larger models — download to ~/data/<volume>/uu-tiktok/models/
curl -L -o models/ggml-small.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
curl -L -o models/ggml-medium.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin
curl -L -o models/ggml-large-v3-turbo-q5_0.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin

ls -lh models/
```

Expected sizes: tiny.en ~75 MB, small ~466 MB, medium.en ~1.5 GB, large-v3-turbo-q5_0 ~547 MB.

- [ ] **Step 3: Build with the cuda feature**

```bash
cargo build --release --features cuda 2>&1 | tee /tmp/build.log
```

Watch for:
- CMake errors → verify CUDA toolkit is installed: `nvcc --version`
- Linker errors → verify CUDA libraries are on `LD_LIBRARY_PATH`
- Whisper.cpp build log mentions CUDA: grep `/tmp/build.log` for "Found CUDA" or "GGML_CUDA=ON"

Expected: clean build. First-time build takes 5-15 min (compiling whisper.cpp with CUDA kernels).

**Per AD0009 fallback rule**: if the cuda build fails after one debugging cycle (~1 day), invoke the Approach 0 fallback path (whisper-cli + JSON patch). Do not silently degrade.

## GPU verification

- [ ] **Step 4: Smoke-test the binary runs and reports GPU backend**

```bash
./scripts/fetch-tiny-model.sh  # ensure tiny.en is present
./target/release/uu-tiktok init
./target/release/uu-tiktok ingest --inbox tests/fixtures/ddp/news_orgs
RUST_LOG=info ./target/release/uu-tiktok process --max-videos 1 2>&1 | tee /tmp/bake-tiny.log
```

Look for the AD0013 init log line. Expected:

```
INFO uu_tiktok::transcribe: WhisperEngine: model loaded gpu_device=0 flash_attn=true model_path=./models/ggml-tiny.en.bin
INFO whisper_rs: ...whisper_init_state: using CUDA backend...
INFO whisper_rs: ...selected device: NVIDIA A10...
```

**If the backend log line says "using CPU backend"** instead of CUDA: silent CPU fallback (sharp-edges.md:61). Abort the bake and debug. Do NOT proceed with measurements — they'd be meaningless.

- [ ] **Step 5: Verify GPU is actually under load during transcription**

In a second SSH session to the same workspace:

```bash
watch -n 0.5 nvidia-smi
```

Re-run the smoke test from Step 4. The A10 should show GPU utilization > 0% during inference and a process running under the workspace user's UID.

## Per-model wallclock measurements

- [ ] **Step 6: Bake measurements per model size**

For each model: tiny.en, small, large-v3-turbo-q5_0, medium.en. Use the same fixture set (~5 videos) for each:

```bash
# Reset DB between runs to ensure fresh processing
rm -f state.sqlite state.sqlite-shm state.sqlite-wal

# Repeat per model
for MODEL in tiny.en small medium.en large-v3-turbo-q5_0; do
  rm -f state.sqlite*
  ./target/release/uu-tiktok init
  ./target/release/uu-tiktok ingest --inbox tests/fixtures/ddp/news_orgs

  UU_TIKTOK_WHISPER_MODEL=./models/ggml-$MODEL.bin \
    time ./target/release/uu-tiktok process --max-videos 5 2>&1 | tee /tmp/bake-$MODEL.log
done
```

Record per video the wallclock breakdown. Plan A's logs already emit per-stage `tracing::info!` lines (fetch + transcribe); confirm they're visible at `RUST_LOG=info`.

Extract per-stage timing into a CSV:

```bash
grep -E "fetch (succeeded|elapsed)" /tmp/bake-*.log
grep -E "transcribe (succeeded|elapsed)" /tmp/bake-*.log
```

If per-stage timing is not yet emitted, T6/T7 should have added `tracing::info!(elapsed_ms = ..., "transcribe complete")` lines. If absent, add them as a small follow-up edit before the bake (or accept coarser numbers from `time`).

- [ ] **Step 7: Measure --compute-lang-probs overhead**

```bash
# With opt-in
UU_TIKTOK_WHISPER_MODEL=./models/ggml-large-v3-turbo-q5_0.bin \
UU_TIKTOK_COMPUTE_LANG_PROBS=1 \
  time ./target/release/uu-tiktok process --max-videos 5 2>&1 | tee /tmp/bake-lang-probs.log
```

Compare wallclock vs the same model without the flag (from Step 6). Record the delta. Expected: ~1.5–2× slower per video (one extra encoder pass).

- [ ] **Step 8: 1-state vs 2-state measurement (architectural future-proofing per AD0016)**

This requires running TWO `process` instances concurrently against the same database, each on the SAME GPU. Plan B Epic 1 ships single-state; this bake measures whether 2 states per A10 would actually speed up production.

```bash
# Reset DB
rm -f state.sqlite*
./target/release/uu-tiktok init
./target/release/uu-tiktok ingest --inbox tests/fixtures/ddp/news_orgs

# Single-instance baseline (5 videos)
UU_TIKTOK_WHISPER_MODEL=./models/ggml-large-v3-turbo-q5_0.bin \
  time ./target/release/uu-tiktok process --max-videos 5 2>&1 | tee /tmp/bake-1state.log

# Reset
rm -f state.sqlite*
./target/release/uu-tiktok init
./target/release/uu-tiktok ingest --inbox tests/fixtures/ddp/news_orgs

# Two concurrent instances, same database (Plan A's claim_next serializes claim atomically)
UU_TIKTOK_WHISPER_MODEL=./models/ggml-large-v3-turbo-q5_0.bin \
  ./target/release/uu-tiktok process --max-videos 5 --worker-id gpu0a &
INSTANCE_A=$!

UU_TIKTOK_WHISPER_MODEL=./models/ggml-large-v3-turbo-q5_0.bin \
  ./target/release/uu-tiktok process --max-videos 5 --worker-id gpu0b &
INSTANCE_B=$!

time wait $INSTANCE_A $INSTANCE_B
```

Note: this is **multi-process** parallelism (two binaries sharing SQLite), which is the "two-state per A10" pattern from `concurrency.md` approximated at the process boundary. True intra-process two-state lands when Plan C upgrades WhisperEngine internals. For the dev-grant bake, this approximation is good enough to characterize the throughput delta.

Record wallclock for 1-instance vs 2-instances. Expected per concurrency.md:34-40: 1.3–1.6× speedup for 2 states; diminishing past that.

If 2-instances throughput is close to 2× the single-instance throughput, GPU saturation isn't the bottleneck — fetch or VRAM pressure is.

## Validate the artifact contract (partial AD0017 implementation)

- [ ] **Step 9: Verify JSON artifact has raw_signals**

```bash
ls transcripts/*/[0-9]*.json | head -3 | while read f; do
  echo "=== $f ==="
  jq '.raw_signals | { schema_version, language, n_segments: (.segments|length), n_tokens_seg0: (.segments[0].tokens|length // 0) }' "$f"
done
```

Expected output per file: schema_version "1", language "en" (or "nl"), n_segments > 0 for non-silent audio.

- [ ] **Step 10: Verify the GPU backend log was captured in stderr**

```bash
grep -E "using.*backend|selected device" /tmp/bake-*.log
```

Expected: every bake run shows "using CUDA backend" and "selected device: NVIDIA A10" (or similar) consistently. Any CPU appearance is a failure to flag in SRC-BAKE-NOTES.md and re-bake.

## Document the bake

- [ ] **Step 11: Create docs/SRC-BAKE-NOTES.md**

Back on the dev machine (or any machine with the repo cloned), create `docs/SRC-BAKE-NOTES.md`:

```markdown
# SRC A10 Bake Notes — Plan B Epic 1

Date: YYYY-MM-DD
Workspace: <SRC workspace name>
Image / catalog item: <name>
Hardware: 1× A10, 11 cores, 88 GB RAM (per the dev grant)
Operator: Danielle McCool

## Environment surprises

- ...any surprises encountered during Phase 0–4 of SRC-BAKE-CHECKLIST.md...

## Tools and versions

- yt-dlp: <version>
- ffmpeg: <version>
- rustc: <version>
- whisper-rs crate: <pinned version, recorded in AD0009>
- whisper.cpp commit: <SHA, recorded in AD0009>
- CUDA toolkit: <version>

## GPU verification

GPU backend log line (every bake run):
```
INFO whisper_rs: ... using CUDA backend ... selected device: NVIDIA A10
```

`nvidia-smi` showed sustained GPU utilization during inference. No silent CPU fallback observed.

## Per-model wallclock (5 videos, per-clip in seconds)

| Model | Total wallclock | Per-clip (mean) | × audio realtime | GPU VRAM peak |
|-------|-----------------|------------------|-------------------|----------------|
| tiny.en | | | | |
| small | | | | |
| medium.en | | | | |
| large-v3-turbo-q5_0 | | | | |

## --compute-lang-probs overhead

| Model | Default per-clip | With --compute-lang-probs | Overhead × |
|-------|-------------------|----------------------------|--------------|
| large-v3-turbo-q5_0 | | | |

## 1-state vs 2-state (multi-process approximation)

| Configuration | Total wallclock for 5 videos | Throughput vs 1-state |
|---------------|--------------------------------|--------------------------|
| 1 process (large-v3-turbo-q5_0) | | 1.0× |
| 2 concurrent processes | | <factor>× |

## Raw artifact size growth

- Per-video transcript JSON without raw_signals: <bytes>
- Per-video transcript JSON with raw_signals: <bytes>
- Growth: <factor>×

Estimate for 1M-video production: <total GB>

## Cold-start vs steady-state

- First-inference latency (cold): <ms>
- Steady-state per-inference latency: <ms>
- CUDA graph warmup penalty per process restart: ~<seconds>

## Bake notes

Anything that broke, took longer than expected, or surprised the operator. Feed into Epic 2 planning.
```

Fill in the values from the actual bake run.

- [ ] **Step 12: Commit the bake notes**

Back on the dev machine, pull the notes file (or write it directly on the workspace and push):

```bash
git add docs/SRC-BAKE-NOTES.md
git commit -m "$(cat <<'EOF'
docs(bake): A10 bake measurements for Plan B Epic 1

Captures the first measurements of Plan B's whisper-rs embedding on
the SRC A10 workspace:

- Per-model wallclocks across tiny.en, small, medium.en, large-v3-turbo-q5_0
- --compute-lang-probs overhead measurement (extra encoder pass cost)
- 1-state vs 2-state throughput delta (multi-process approximation
  for production planning)
- GPU backend verification (no silent CPU fallback)
- Raw artifact size growth from the raw_signals JSON addition
- Cold-start vs steady-state latency

Feeds Epic 2 planning: justifies (or pre-empties) the pipelined
orchestrator's fetch-transcribe overlap engineering.

Refs: AD0011, AD0013, AD0015, AD0016, AD0017

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

## Operational habit: spin down after bake

- [ ] **Step 13: Pause the workspace**

Per AD0011: on grant-based wallets, pausing the workspace charges zero CPU/GPU and zero storage.

1. Stop any running batches (`Ctrl+C` if `process` is running)
2. Verify no `in_progress` rows: `sqlite3 state.sqlite "SELECT COUNT(*) FROM videos WHERE status='in_progress'"` — should be 0
3. In the SRC portal: workspace → Actions → Pause

The workspace state and external storage volume both persist; the next session resumes within minutes.

---

## Self-check

- [ ] `SRC-BAKE-NOTES.md` exists with values filled in (no TBD)
- [ ] GPU backend confirmed (not silent CPU)
- [ ] All four models measured
- [ ] 1-state vs 2-state recorded
- [ ] --compute-lang-probs overhead recorded
- [ ] At least one raw_signals object inspected via jq, confirmed structure
- [ ] Workspace paused after the bake (no idle burn)

## What Epic 2 inherits from this bake

- A baseline of per-clip wallclocks → justifies (or de-justifies) the pipelined orchestrator's engineering
- A measured 1-state vs 2-state ratio → informs whether Plan C should add multi-state to WhisperEngine internals or stay process-level
- The GPU verification log line format → Epic 2 can assert against it in tests
- The exact whisper-rs crate version + whisper.cpp commit pinned → Plan B is reproducible
