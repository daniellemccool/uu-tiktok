# whisper.cpp — Build system and compile-time flags

Read this when answering questions about: how to compile whisper.cpp, which CMake options to set for CUDA/Metal/Vulkan, what GGML toggles are surfaced, lib-only vs. examples build, packaging.

## Whisper-level toggles (`CMakeLists.txt`, 258 lines)

- `BUILD_SHARED_LIBS` — shared lib output
- `WHISPER_BUILD_TESTS` / `WHISPER_BUILD_EXAMPLES` / `WHISPER_BUILD_SERVER` — defaults ON in standalone, OFF when included as subdir
- `WHISPER_USE_SYSTEM_GGML` — link to system-installed ggml instead of bundled
- `WHISPER_CURL` — model download via libcurl
- `WHISPER_SDL2` — for stream/command examples
- `WHISPER_FFMPEG` (Linux only) — FFmpeg input support (Opus/AAC/MP3/etc. without pre-conversion)
- `WHISPER_COREML` / `WHISPER_COREML_ALLOW_FALLBACK` — macOS encoder via Core ML / ANE
- `WHISPER_OPENVINO` — Intel OpenVINO encoder
- `WHISPER_FATAL_WARNINGS`, `WHISPER_ALL_WARNINGS*`
- `WHISPER_SANITIZE_THREAD/ADDRESS/UNDEFINED`

All compute backends are **GGML-level** toggles, surfaced through whisper-level flag aliasing. Old `WHISPER_CUDA`/`WHISPER_METAL`/etc. names auto-translate with deprecation warnings (`CMakeLists.txt:113-123`).

## NVIDIA CUDA build

```
cmake -B build -DGGML_CUDA=1 -DCMAKE_CUDA_ARCHITECTURES=86
cmake --build build -j --config Release
```

`CMAKE_CUDA_ARCHITECTURES=86` is correct for A10/A100 (Ampere `sm_86`). For Hopper (H100) use `90`, for Ada (RTX 4xxx) use `89`, for older Ampere consumer (RTX 3xxx) `86` works.

### Sub-options (`ggml/src/ggml-cuda/CMakeLists.txt`)

- `GGML_CUDA_FA = ON` (default) — compile FlashAttention CUDA kernels. Whisper sets `flash_attn = true` by default in `whisper_context_default_params`, so leave this on.
- `GGML_CUDA_FA_ALL_QUANTS = OFF` — compile FA for all quantization formats (slower compile)
- `GGML_CUDA_FORCE_MMQ` / `GGML_CUDA_FORCE_CUBLAS` — pick MMQ kernels vs. cuBLAS for quantized matmul (default lets the runtime choose)
- `GGML_CUDA_NO_VMM` — disable CUDA virtual memory management
- `GGML_CUDA_NO_PEER_COPY` — disable P2P GPU copies (multi-GPU)
- `GGML_CUDA_GRAPHS` — CUDA graph capture. Bench requires a 2-loop warmup because of this (`bench.cpp:92-94`). The first 30s window of any inference is slower than steady-state.
- `GGML_CUDA_NCCL` — NCCL multi-GPU communication

## Other GGML compute backends

Surfaced through the parent project: `GGML_CUDA`, `GGML_HIP` (AMD ROCm), `GGML_VULKAN`, `GGML_METAL`, `GGML_OPENCL`, `GGML_SYCL` / `GGML_SYCL_F16` (Intel oneAPI), `GGML_CANN` (Huawei NPU), `GGML_MUSA` (Moore Threads), `GGML_BLAS` (OpenBLAS for CPU encoder), `GGML_KOMPUTE`, `GGML_RPC` (distributed), `GGML_NATIVE` (`-march=native`), `GGML_OPENMP`, `GGML_CCACHE`.

## Lib-only build (for embedding in another binary)

```
cmake -B build \
  -DGGML_CUDA=1 -DCMAKE_CUDA_ARCHITECTURES=86 \
  -DBUILD_SHARED_LIBS=ON \
  -DWHISPER_BUILD_TESTS=OFF \
  -DWHISPER_BUILD_EXAMPLES=OFF \
  -DWHISPER_BUILD_SERVER=OFF
cmake --build build -j --config Release
```

Produces `libwhisper.so` (or `.a`) + `libggml*.so` deps. Public header at `include/whisper.h`. CMake package (`whisper-config.cmake`) and pkg-config (`whisper.pc`) files are installed for `find_package(whisper)` consumers.

## C++ standard

`target_compile_features(whisper PUBLIC cxx_std_11)` — minimum C++11, with comment "don't bump". Consumers can use C++11+.

## Linkage

`target_link_libraries(whisper PUBLIC ggml Threads::Threads)` — the public ggml dep is exposed transitively. CoreML and OpenVINO are linked as PRIVATE when enabled.
