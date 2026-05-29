# gecko-wii screenshotter

Drives [gecko](https://github.com/ioncodes/gecko) against Wii discs and uploads
screenshots under the `gecko-wii` archive slug on `emu.layle.dev`. Its GameCube sibling
lives at `../gecko-gc/`; the two share an identical binary, Dockerfile, and `run.py` —
only the slug, archive slug, ROM subdir, and game/title lists differ.

## Required secrets (BIOS files)

The orchestrator mounts `<secret_root>/gecko-wii/` at `/config` inside the container.
Place the three gecko BIOS blobs there:

- `/secrets/gecko-wii/IPL.decoded.bin`
- `/secrets/gecko-wii/dsp_rom.bin`
- `/secrets/gecko-wii/dsp_coef.bin`

Wii boots via `Wii::apploader_hle` rather than IPL, but the replacement worker reads all
three files unconditionally at startup, so `IPL.decoded.bin` must be present (symlink the
GC copy in).

Upstream gecko bakes these via `include_bytes!("../../../../private/*.bin")`. The
Dockerfile replaces `crates/screenshotter/src/bin/screenshotter-worker.rs` at build time
with a variant that reads them from `GECKO_BIOS_DIR` (default `/config`) at runtime, so
the build needs no secrets.

## ROMs

Reads from `<rom_root>/Nintendo - Wii/`. Supported direct: `iso`, `rvz`, `zip` (gecko's
`image::load_dvd` auto-extracts the first `.iso`/`.rvz` from a zip).

## GPU

`gpu = "nvidia"`. Gecko's wgpu adapter does not need a display surface; the Dockerfile
bakes a Vulkan ICD JSON pointing at `libGLX_nvidia.so.0` so the loader picks up the
NVIDIA driver injected by `--gpus all`.
