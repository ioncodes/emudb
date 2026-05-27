# ymir-screenshotter

Emulator-specific screenshotter for [Ymir](https://github.com/StrikerX3/Ymir) (Sega Saturn),
driven by [emu-shot-orchestrator](../../). Produces **multiple frames per game**:
`/output/<UNIQUE_ID>/<n>.png`.

This is a rebuild of the original standalone ymir-screenshotter onto the new manifest-driven
contract. The original's self-contained orchestration (its own webhook, gamelist parsing,
header-derived game IDs, zip extraction, dedupe) now lives in the orchestrator. What remains
here is the valuable part: the native capture core that drives Ymir headlessly. **Ymir itself
does not take screenshots** — this repo's worker does.

## What was kept vs. changed

**Kept verbatim (the capture foundation):**
`worker_driver.{hpp,cpp}` (drives the Ymir core via its software renderer + frame callback),
`png_writer.*`, `file_io.*`, `game_id.*`, `input_sequence.*`, `ipl_picker.*`,
`third_party/stb_image_write.h`.

**Changed / new:**
- `worker.cpp` — now accepts `--ipl-dir` and auto-selects the region-appropriate IPL per disc
  (so the Python runner needs no Ymir region database). `--ipl <file>` still works.
- `run.py` — manifest-driven entrypoint that runs the worker once per game, in parallel.
- `CMakeLists.txt` — builds the Ymir core from the orchestrator-supplied `emulator` build
  context via `add_subdirectory(/src/ymir)` instead of `FetchContent` at a pinned tag, and
  builds only the worker target.

**Dropped:** `main.cpp`, `cleanup.*`, `zip_extractor.*` (orchestrator extracts archives),
and the old `webhook/`, `deploy/`, `scripts/` (orchestrator owns the webhook + tunnel).

## Output folder rule

The worker writes to `--out`, which the orchestrator sets to `/output/<UNIQUE_ID>`. The
`UNIQUE_ID` comes from `titlemap.txt` — **never** from the disc header. The worker's internal
`normalize_game_id` is used only for log lines. Raw frames are emitted here; the orchestrator
dedupes and removes single-colored frames afterwards.

## Contract

The orchestrator runs (no `--gpus`, since `requires_gpu=false`):

```
docker run --rm --network none \
  -v <job>/input:/input:ro -v <job>/output:/output \
  -v <job>/manifest.json:/job/manifest.json:ro -v /secrets/ymir:/config:ro \
  emu-shot/ymir:<tag> --job /job/manifest.json --input /input --output /output
```

Per game, `run.py` runs:

```
ymir-screenshotter-worker --ipl-dir /config/ipl --disc <input> --out /output/<id>
```

Provide Saturn IPL ROMs (512 KiB each) under `secrets/ymir/ipl/` on the host.

## Build-integration notes (iteration-prone)

- The Docker build uses **vcpkg** (manifest mode, `vcpkg.json`) for SDL3/fmt/etc., mirroring the
  original. The first build is heavy.
- `add_subdirectory(/src/ymir)` assumes Ymir's CMake exposes the `ymir::ymir-core` target and
  behaves as a subproject under the `Ymir_*` cache options set in `CMakeLists.txt`. If Ymir's
  CMake assumes top-level, minor overrides may be needed.
- Capture runs via Ymir's **software renderer** (no window/GPU), so the container needs no
  display; mesa/SDL runtime libs are included only for safety.
- IPL region selection relies on the disc header field `compatAreaCode` (as in the original).

`gamelist.txt`/`titlemap.txt` currently hold a small consistent starter subset for the first
end-to-end test; expand to the full set once it's green.
