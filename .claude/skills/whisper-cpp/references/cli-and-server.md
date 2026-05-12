# whisper.cpp — Front-ends: whisper-cli, whisper-server, whisper-bench

Read this when answering questions about: which CLI flags exist, what the JSON output format looks like, the server's HTTP API and concurrency, what bench measures, choosing between CLI and server for an integration.

## `whisper-cli` (`examples/cli/cli.cpp`, 1314 lines)

**Model loading is amortized across files.** Loads model **once** (`cli.cpp:1039`), iterates `params.fname_inp` (`cli.cpp:1072`), frees once (`cli.cpp:1311`). Passing many `-f` flags in a single invocation is a real (if limited) optimization vs. one CLI call per file.

### Flag categories

- **Decoding**: `-bo/-bs/-tp/-tpi/-et/-lpt/-nth/-nf/-mc`
  - `-bo N` (best-of, default 5), `-bs N` (beam size, default 5)
  - `-tp` (temperature), `-tpi` (temperature increment, default 0.2)
  - `-et` (entropy_thold, default 2.4), `-lpt` (logprob_thold, default -1.0), `-nth` (no_speech_thold, default 0.6)
  - `-nf` disables temperature fallback ladder entirely
  - `-mc N` limits text history (0 = no rolling context)
- **Output**: `-otxt/-ovtt/-osrt/-olrc/-ocsv/-oj/-ojf/-of/-owts`
  - `-oj` basic JSON, `-ojf` JSON-full (per-token data — see schema below)
  - `-of FNAME` output basename (without extension)
- **Display**: `-pc` color by token confidence (uses `whisper_full_get_token_p`); `--print-confidence` styled (high/medium/low at <0.33, <0.66 thresholds); `-ps`, `-pp`, `-np`, `-ls` (log decoder scores)
- **GPU**: `-ng` (no-gpu), `-dev N` (gpu_device index), `-fa` / `-nfa` (flash attention on/off)
- **Audio**: `-l LANG` (or `auto`), `-dl` (detect language and exit), `-ac N` (overwrite encoder audio_ctx — smaller = faster, less accurate), `-d/-ot/-on` (offset/duration)
- **Prompt**: `--prompt TEXT`, `--carry-initial-prompt` (always prepend initial prompt)
- **VAD**: `--vad/-vm/-vt/-vspd/-vsd/-vmsd/-vp/-vo` — see `vad.md`
- **Other**: `-tdrz` (tinydiarize), `-di` (energy-based stereo diarize), `-tr` (translate to English), `-dtw MODEL` (DTW alignment for accurate token timestamps), `-ml N` (max segment length in chars), `-sow` (split on word boundary), `-sns` (suppress non-speech tokens), `--suppress-regex`, `--grammar`

### `--output-json-full` schema (`cli.cpp:611-769`)

```json
{
  "systeminfo": "...",
  "model": {"type":"...", "multilingual":true, "vocab":N, "audio":{"ctx":N,"state":N,"head":N,"layer":N}, "text":{...}, "mels":N, "ftype":N},
  "params": {"model":"path", "language":"en", "translate":false},
  "result": {"language": "en"},
  "transcription": [
    {
      "timestamps": {"from":"00:00:00.000","to":"00:00:01.000"},
      "offsets": {"from":0,"to":1000},
      "text": "...",
      "tokens": [{"text":"...", "id":N, "p":0.95, "t_dtw":-1, "timestamps":{...}, "offsets":{...}}],
      "speaker": "...",            // if -di
      "speaker_turn_next": false   // if -tdrz
    }
  ]
}
```

**Critical omission**: the CLI's JSON-full does NOT include `no_speech_prob` per segment, nor `avg_logprob`. Those come only from the C API or the server's `verbose_json`. If you want them from the CLI, you patch the JSON writer or compute `avg_logprob` yourself by averaging `token.p` (well, `token.plog` if you also patch the writer). See `confidence-and-sampling.md`.

## `whisper-server` (`examples/server/server.cpp`, 1262 lines)

cpp-httplib + nlohmann/json. Endpoints:

- `POST /inference` — multipart form with `file` field plus all wparam fields as form fields
- `POST /load` — hot-swap model (lock mutex during swap)
- `GET /health` — `{"status":"ok"}` 200 or `{"status":"loading model"}` 503
- `OPTIONS /inference` — CORS preflight

### Concurrency

A single `std::mutex whisper_mutex` (`server.cpp:627, 807`) wraps every inference request. **One inference at a time per server.** Multi-GPU scaling = run multiple server processes on different ports, each pinned to a different `gpu_device`.

The server *does* accept `n_processors` and routes to `whisper_full_parallel` (`server.cpp:978`) — but that splits one audio across N states with documented quality loss at 30s-window boundaries (`whisper.cpp:7891`). It's not a tool for parallel inference of independent files.

### Response formats (`response_format` field)

- `json` (default) — same shape as CLI's `output_json`
- `text`, `srt`, `vtt`
- `verbose_json` — OpenAI-compatible. **The only output that exposes `no_speech_prob` per segment** (`server.cpp:1106`), plus optional full language-probability distribution (`server.cpp:1054-1067`). Set `no_language_probabilities=false` to compute the language probs (one extra encode+decode).

### `verbose_json` segment shape

```json
{
  "id": 0,
  "text": "...",
  "start": 0.0,
  "end": 1.5,
  "tokens": [token_ids…],
  "words": [{"word":"...","start":0.0,"end":0.5,"t_dtw":-1,"probability":0.9}],
  "temperature": 0.0,
  "avg_logprob": -0.234,
  "no_speech_prob": 0.05
}
```

Top-level adds `task`, `language`, `duration`, `text`, `segments`, plus `detected_language`, `detected_language_probability`, `language_probabilities` if requested.

### HTTP abort

The server registers `wparams.abort_callback` to detect client disconnect (`server.cpp:971-976`) — inference is cancellable. Returns 499 (nginx convention) on disconnect.

### Server defaults differ from CLI

`best_of = 2` (vs. CLI's 5), `beam_size = -1` (greedy unless explicitly set), `max_len` defaults to 60 if 0 was passed, `token_timestamps = !no_timestamps` (ON by default).

## `whisper-bench` (`examples/bench/bench.cpp`)

Microbenchmark of raw encoder + 3 decoder modes (single-token gen ×256, batched-5 ×64, prompt 256-tokens ×16). Doubles the workload as warmup (CUDA graph capture, comment at `bench.cpp:92-94`). Reports `Enc.`/`Dec.`/`Bch5`/`PP` as ms-per-call.

**Important caveat**: this measures encoder/decoder primitives only — no mel computation, no sampling, no fallback. Real-world `whisper_full` is slower than these numbers suggest. Use bench for relative comparisons (model A vs. model B, FA on vs. off, quant Q5_0 vs. F16), not absolute throughput predictions.
