# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS build

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config cmake git ca-certificates \
        libvulkan-dev \
        libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxi-dev \
        libxkbcommon-dev libwayland-dev \
        libasound2-dev libudev-dev \
    && rm -rf /var/lib/apt/lists/*

COPY --from=emulator . /src/gecko

COPY screenshotter-worker.rs /src/gecko/crates/screenshotter/src/bin/screenshotter-worker.rs

WORKDIR /src/gecko
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/gecko/target \
    cargo build --release -p screenshotter --bin screenshotter-worker \
    && install -Dm755 target/release/screenshotter-worker /out/screenshotter-worker

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        python3 \
        libvulkan1 mesa-vulkan-drivers \
        libx11-6 libxext6 \
        libegl1 libgl1 libgles2 \
        libasound2t64 libudev1 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/vulkan/icd.d \
    && printf '{"file_format_version":"1.0.1","ICD":{"library_path":"libGLX_nvidia.so.0","api_version":"1.4.329"}}\n' \
        > /etc/vulkan/icd.d/nvidia_icd.json

COPY --from=build /out/screenshotter-worker /usr/local/bin/gecko-screenshotter-worker
COPY run.py /screenshotter/run.py
COPY gamelist.txt /screenshotter/gamelist.txt
COPY titlemap.txt /screenshotter/titlemap.txt
COPY screenshotter.toml /screenshotter/screenshotter.toml

RUN chmod +x /screenshotter/run.py /usr/local/bin/gecko-screenshotter-worker

# Per-game invocation: orchestrator runs `docker run <image> {disc} {out}` once
# per game (see per_game_args in screenshotter.toml). The worker takes positional
# <input> <output_dir>; the old manifest run.py is no longer the entrypoint.
ENTRYPOINT ["/usr/local/bin/gecko-screenshotter-worker"]
