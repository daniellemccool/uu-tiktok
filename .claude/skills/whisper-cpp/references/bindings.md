# whisper.cpp — Language bindings

Read this when answering questions about: how to call whisper.cpp from $LANGUAGE, which Rust/Python/Go/etc. binding to use, the binding-pattern shape, what's in-tree vs. community-maintained.

## In-tree bindings

Maintained in this repo, version-locked to whisper.cpp.

- **Go** (`bindings/go/`): two layers — low-level CGO (`bindings/go/whisper.go`) and Go-idiomatic high-level package (`bindings/go/pkg/whisper/`). Active. Pattern: `model.NewContext()` returns a context handle, then `context.Process(samples, …)` and `context.NextSegment()`.
- **Java** (`bindings/java/`): JNI binding.
- **JavaScript** (`bindings/javascript/`): WASM via emscripten, npm-published as `whisper.cpp`. Browser-runnable; the API is "currently very rudimentary" per the binding README.
- **Ruby** (`bindings/ruby/`): full C extension with `Whisper::Context`, `Whisper::Params`, full VAD wrappers (`Whisper::VAD::Context`, etc.), segment/token models. Most comprehensive in-tree binding. The latest HEAD commit (`c81b2dab`) touched this binding ("transcribe without GVL, accept more MemoryViews, Windows support, fix memory size report, improve document").

Pattern (Ruby example, illustrative for binding shape):
```ruby
whisper = Whisper::Context.new("base")
params = Whisper::Params.new(language: "en", initial_prompt: "...", ...)
whisper.transcribe("audio.wav", params) { |segment| puts segment.text }
```

## Out-of-tree bindings

Community-maintained crates/packages. Referenced from the main `README.md` but not version-locked.

- **Rust** — `tazz4843/whisper-rs` (referenced at `README.md:703`). The recommended Rust binding. Wraps the C API; tracks upstream actively. Builds whisper.cpp from source, optionally with CUDA/Metal/etc. via cargo features. Exposes `WhisperContext`, `WhisperState`, `FullParams`. Good fit for embedding whisper.cpp in a Rust binary.
  - Feature flags: `cuda`, `coreml`, `metal`, `hipblas`, `vulkan`, `openblas`, etc. Maps to the equivalent GGML cmake toggles.
  - For A10 CUDA: `whisper-rs = { version = "*", features = ["cuda"] }` plus `CMAKE_CUDA_ARCHITECTURES=86` in the build env.
- **Python** — three competing bindings:
  - `whispercpp.py` (stlukey, Cython)
  - `pywhispercpp` (abdeladim-s, pybind11)
  - `whispercpp` (AIWintermuteAI fork of aarnphm, pybind11)
- **.NET** — `whisper.net` (sandrohanea), `NickDarvey/whisper`
- **Swift/Objective-C** — `whisper.spm` (in-tree spm package), `SwiftWhisper` (exPHAT)
- **React Native** — `whisper.rn` (mybigday) for iOS/Android
- **R** — `audio.whisper` (bnosac)
- **Unity** — `whisper.unity` (Macoron)

## Choosing a binding pattern for our pipeline

If embedding whisper.cpp in a Rust orchestrator (the `uu-tiktok` case), `whisper-rs` is the only credible choice. Pattern:

1. Set GGML CUDA build flags in the parent crate's `build.rs` environment, or as cargo features in `Cargo.toml`.
2. Load context once per GPU: `WhisperContext::new_with_params(path, ctx_params)`.
3. Allocate N states per context: `ctx.create_state()`. See `concurrency.md`.
4. Per file: `state.full(params, &samples)` — the binding equivalent of `whisper_full_with_state`.
5. Drain segments: `state.full_n_segments()` + `state.full_get_segment_text(i)` + per-token getters.

The C API surface is exposed nearly 1:1, so anything documented in `api-and-pipeline.md` is reachable from Rust.

## Stability

The C API has been stable in shape for a long time — the deprecation pattern at `whisper.h:216-239` (e.g., `whisper_init_from_file` → `whisper_init_from_file_with_params`) shows the project does versioned migrations rather than breaking changes. New features (VAD, DTW) appear as additive struct fields with safe defaults. So binding crates rarely break across upstream updates; you can pin to a recent version and update on a cadence.
