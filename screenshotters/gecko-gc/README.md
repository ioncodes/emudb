# gecko-gc screenshotter

Drives [gecko](https://github.com/ioncodes/gecko) against GameCube discs and uploads
screenshots under the `gecko-gc` archive slug on `emu.layle.dev`. Its Wii sibling lives
at `../gecko-wii/`; the two share an identical binary, Dockerfile, and `run.py` — only
the slug, archive slug, ROM subdir, and game/title lists differ.

## Required secrets (BIOS files)

The orchestrator mounts `<secret_root>/gecko-gc/` at `/config` inside the container. Place
the three gecko BIOS blobs there:

- `/secrets/gecko-gc/IPL.decoded.bin`
- `/secrets/gecko-gc/dsp_rom.bin`
- `/secrets/gecko-gc/dsp_coef.bin`

(`dsp_rom.bin` / `dsp_coef.bin` are shared with the Wii config — symlink or duplicate as
needed under `/secrets/gecko-wii/`. `IPL.decoded.bin` is GC-only but harmless to mirror.)

Upstream gecko bakes these via `include_bytes!("../../../../private/*.bin")`. The
Dockerfile replaces `crates/screenshotter/src/bin/screenshotter-worker.rs` at build time
with a variant that reads them from `GECKO_BIOS_DIR` (default `/config`) at runtime, so
the build needs no secrets.

## ROMs

Reads from `<rom_root>/Nintendo - GameCube/`. Supported direct: `iso`, `rvz`, `zip`
(gecko's `image::load_dvd` auto-extracts the first `.iso`/`.rvz` from a zip).

## GPU

`gpu = "nvidia"`. Gecko's wgpu adapter does not need a display surface; the Dockerfile
bakes a Vulkan ICD JSON pointing at `libGLX_nvidia.so.0` so the loader picks up the
NVIDIA driver injected by `--gpus all`.
