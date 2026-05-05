# SRC bake checklist

The walking-skeleton's first deployment to a real SURF Research Cloud
workspace. Goal: surface environment surprises (network policy, mounts,
tool availability, GPU/CPU mix, model storage) before committing to Plan
B refinements that may turn out to be incompatible.

**Output of this checklist:** a short notes file (`SRC-BAKE-NOTES.md`)
capturing what worked, what didn't, what you had to change, and the
per-clip wallclock numbers. That notes file becomes input to Plan B.

**Skill to invoke:** `d3i-claude-skills:src-workspace-ops` for anything
workspace-management-shaped (provisioning, SSH, storage, exports). Don't
reinvent its muscle memory inline.

---

## Phase 0 — Workspace and access

| ✓ | Step | Action | What to capture |
|---|------|--------|-----------------|
|   | 0.1  | Provision a SRC workspace via the d3i SRC skill. Pick an image you'd plausibly run a study on. | Image name, node type (CPU/GPU), core/RAM specs |
|   | 0.2  | SSH in. Confirm shell, `whoami`, `pwd`. | — |
|   | 0.3  | `df -h` and `mount`. Identify: local disk path(s), Research Drive mount path, scratch path (if any). | Each mount: path, FS type, size, free |
|   | 0.4  | `lscpu`; if GPU node: `nvidia-smi`. | CPU model, core count, GPU model + VRAM |

Stop and write down each capture before moving on. If 0.1 fails, none of
the rest matters; surface the blocker and stop.

---

## Phase 1 — Pre-flight: tools

```sh
for t in yt-dlp ffmpeg whisper-cli rustc cargo curl git sqlite3; do
  printf '%-12s ' "$t"; command -v "$t" || echo "MISSING"
done
yt-dlp --version
ffmpeg -version | head -1
whisper-cli --help 2>&1 | head -1
rustc --version
```

| ✓ | Step | Action | Pass criterion |
|---|------|--------|----------------|
|   | 1.1  | Run the script above. | All seven tools resolve to a path |
|   | 1.2  | If `whisper-cli` is missing: decide install path — `pacman`/`apt`/`conda`/build-from-source. Document. | binary on `PATH` |
|   | 1.3  | If `yt-dlp` is missing: same. Note that `yt-dlp` updates frequently and a stale system package may not handle current TikTok pages. Prefer `pip install yt-dlp` into a venv if available. | `yt-dlp --version` is recent (within a few months) |
|   | 1.4  | If anything else missing: capture, decide, document. | — |

A missing tool is not an SRC blocker — it's a packaging task for Plan B.
The point of this phase is to know which tools the operator must
provide vs which the image provides.

---

## Phase 2 — Pre-flight: network egress to TikTok

This is the highest-risk failure. SRC workspaces commonly restrict
outbound traffic; if TikTok's CDN isn't reachable, the entire
fetch-from-URL architecture has to change.

```sh
# Pick a known-public TikTok URL. The DDP fixture URLs work.
URL="https://www.tiktokv.com/share/video/7530600748577492257/"

# 2.1 — Just metadata, no download. Cheap. Tests DNS + TLS + page fetch.
yt-dlp --no-playlist --no-warnings --skip-download --print "%(id)s %(duration)s" "$URL"

# 2.2 — Actual audio download (small). Tests CDN reachability.
mkdir -p /tmp/egress-test && cd /tmp/egress-test
yt-dlp --no-playlist --no-warnings --quiet -x --audio-format wav \
  --postprocessor-args "ffmpeg:-ar 16000 -ac 1" \
  -o "%(id)s.%(ext)s" "$URL"
ls -la *.wav
```

| ✓ | Step | Action | Pass | Fail signal |
|---|------|--------|------|-------------|
|   | 2.1  | Run the metadata-only command. | Prints `7530600748577492257 <seconds>` | `Unable to download webpage`, DNS error, TLS error, or hang |
|   | 2.2  | Run the audio download. | `*.wav` exists, ~1-3 MB | curl/HTTPS error to a `*.tiktokcdn.com` host, or 403 |

**If 2.2 fails:** stop. Capture exact error. This is a Plan-B-shape
question, not a Plan-B-detail question. Discuss with platform admins
whether outbound egress can be opened to TikTok's domains, or whether
the architecture must shift (e.g., audio collected outside SRC,
transferred in via Research Drive, pipeline stripped of fetcher and run
purely on local files). Either resolution invalidates a chunk of
current code; that's exactly why we're testing now.

---

## Phase 3 — Pre-flight: SQLite WAL on each storage backend

SQLite's WAL journal mode does not play nicely with NFS-style mounts
(Research Drive is one such). Behavior ranges from "works" through
"`database is locked` under contention" to "silent corruption."

```sh
probe() {
  local dir="$1"
  local db="$dir/wal-probe.sqlite"
  rm -f "$db" "$db-wal" "$db-shm"
  sqlite3 "$db" "PRAGMA journal_mode=WAL; CREATE TABLE t(x); INSERT INTO t VALUES (1);"
  printf '%-40s ' "$dir"
  sqlite3 "$db" "PRAGMA journal_mode;"
  rm -f "$db" "$db-wal" "$db-shm"
}

probe "$HOME"                    # local home (usually local disk)
probe "/scratch/$USER"           # if scratch exists
probe "/path/to/research-drive"  # the actual Research Drive mount
```

| ✓ | Step | Action | Pass | Fail signal |
|---|------|--------|------|-------------|
|   | 3.1  | Run probes for each mount. | Each prints `wal` | One prints `delete` (silent fallback) or errors |
|   | 3.2  | If Research Drive returns `delete`: that's the silent fallback — SQLite refused WAL. Note. | — | — |
|   | 3.3  | Decision (capture in notes): which mount holds `state.sqlite`? Recommend: local-disk-only for the DB; Research Drive only for transcript artifacts (which are write-once). | Decision recorded | — |

The walking skeleton's `Store::open` sets WAL unconditionally; a notes
entry here is fine for now, code change deferred to Plan B if needed.

---

## Phase 4 — Pre-flight: model storage

```sh
# Where does ./models/ resolve in the workspace? Is the parent persistent?
df -h .
du -sh ~ 2>/dev/null || true
```

| ✓ | Step | Action | Capture |
|---|------|--------|---------|
|   | 4.1  | Decide: local disk (fast, may not persist across workspace restart) vs Research Drive (persistent, slower, may have semantic quirks). | Chosen path |
|   | 4.2  | Download `ggml-tiny.en.bin` (~75 MB) via `./scripts/fetch-tiny-model.sh`. Time it. | Wallclock seconds |
|   | 4.3  | Download `ggml-small.bin` (~466 MB). | Wallclock seconds |
|   | 4.4  | If GPU node: also download `ggml-medium.bin` (~1.5 GB) — Plan-B-sized but worth knowing it fits and is reachable. Skip on CPU-only. | Wallclock seconds |

The new `--whisper-model` / `UU_TIKTOK_WHISPER_MODEL` override is
exactly the lever for "model lives at `/data/shared/whisper-models/...`".
Use it. Don't hand-edit `config.rs`.

---

## Phase 5 — Functional bake: English (tiny.en)

```sh
git clone <this repo> uu-tiktok && cd uu-tiktok
cargo build --release
mkdir -p inbox transcripts
cp tests/fixtures/ddp/20260416_test/*.json inbox/
./target/release/uu-tiktok init
./target/release/uu-tiktok ingest
time ./target/release/uu-tiktok process --max-videos 1
```

| ✓ | Step | Action | Pass | Fail signal |
|---|------|--------|------|-------------|
|   | 5.1  | Build release. | `target/release/uu-tiktok` exists | linker error → tool/library gap |
|   | 5.2  | `init` succeeds. | `state.sqlite` exists, schema applied | `database is locked` → Phase 3 followup |
|   | 5.3  | `ingest` succeeds. | Log shows N videos parsed and upserted | DDP parse error |
|   | 5.4  | `process --max-videos 1` succeeds. | Exit 0; transcript .txt + .json under `transcripts/<shard>/` | Any non-zero exit (other than 3 = nothing claimed) |
|   | 5.5  | Capture `time` wallclock. | — | — |

**Deliverable:** one English transcript on disk + a wallclock number.

---

## Phase 6 — Functional bake: Dutch (small, override path)

```sh
# Use the new override flag.
UU_TIKTOK_WHISPER_MODEL=./models/ggml-small.bin \
  time ./target/release/uu-tiktok process --max-videos 1
# (Or use --whisper-model ./models/ggml-small.bin)
```

Pre-condition: a Dutch-language video in the inbox. If the shipped
fixture has none, add one DDP JSON containing a known Dutch URL (e.g.
`https://www.tiktokv.com/share/video/7530600748577492257/`) before
running.

| ✓ | Step | Action | Pass |
|---|------|--------|------|
|   | 6.1  | Run process with the env var override. | Exit 0; transcript looks like Dutch text |
|   | 6.2  | Inspect `transcripts/<shard>/<id>.json`: `language_detected` should be `"nl"` (no longer `null`) | `language_detected: "nl"` |
|   | 6.3  | Capture `time` wallclock. | — |

**Deliverable:** one Dutch transcript on disk + the override is proven
on the target + a per-clip wallclock for the multilingual model.

---

## Phase 7 — Performance characterization

Pick the GPU node type that matches the production target from the
design spec (A10 or A100). For each, process N=10 videos with each
model. `large-v3` is the production model per the design spec; the
smaller models are cost-comparison datapoints.

| Model        | Node    | Total wallclock | Per-clip | × audio-realtime |
|--------------|---------|-----------------|----------|------------------|
| tiny.en      | CPU     |                 |          |                  |
| small        | GPU     |                 |          |                  |
| large-v3     | GPU     |                 |          |                  |

Compute: per-clip × number-of-respondents × videos-per-respondent =
total CPU-hours (or GPU-hours) for the study. This is what the grant
application needs; see grant-sizing notes alongside this checklist.

---

## Phase 8 — Capture and document

Write `SRC-BAKE-NOTES.md` (alongside this checklist or in `docs/`)
covering:

1. Environment surprises (each fail-signal in any phase).
2. Tool versions actually installed.
3. Mount-and-WAL decisions made.
4. Where the model file lives.
5. Per-clip wallclock numbers from Phase 7.
6. Anything that broke that Plan B has to address.

That document, plus the existing
`docs/superpowers/plans/PLAN-B-KICKOFF-PROMPT.md`, is the input to the
Plan B kickoff. Plan B planning should not start without it.

---

## What this checklist does NOT do

- It does not exercise the donation kit's *front-end* — that's a
  separate D3I component (the per-platform researcher fork). Out of
  scope; this is post-donation pipeline only.
- It does not test concurrency / multi-worker behavior. Plan A's
  `claim_next` is single-claimer-friendly; Plan B parallelism is its
  own bake.
- It does not measure end-to-end donation-to-transcript latency for
  participants. That's a study-design concern.
