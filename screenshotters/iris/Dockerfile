# syntax=docker/dockerfile:1.7

FROM ubuntu:24.04 AS build

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential git cmake ninja-build pkg-config python3 ca-certificates \
        wget lsb-release software-properties-common gnupg \
        libvulkan-dev \
        libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxfixes-dev libxi-dev \
        libxss-dev libxkbcommon-dev libwayland-dev libwayland-bin wayland-protocols libdecor-0-dev libdrm-dev libgbm-dev \
        libgl1-mesa-dev libegl1-mesa-dev libgles2-mesa-dev \
        libasound2-dev libpulse-dev libudev-dev libdbus-1-dev libibus-1.0-dev \
        libssl-dev \
    && wget -qO /tmp/llvm.sh https://apt.llvm.org/llvm.sh \
    && chmod +x /tmp/llvm.sh && /tmp/llvm.sh 19 \
    && rm -rf /var/lib/apt/lists/* /tmp/llvm.sh

ENV CC=clang-19 CXX=clang++-19

COPY --from=emulator . /src/iris

WORKDIR /src/iris
RUN cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_C_COMPILER=clang-19 -DCMAKE_CXX_COMPILER=clang++-19
RUN cmake --build build -j"$(nproc)"

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        python3 cage seatd \
        libvulkan1 mesa-vulkan-drivers \
        libx11-6 libxext6 libxrandr2 libxcursor1 libxfixes3 libxi6 libxss1 libxkbcommon0 \
        libwayland-client0 libwayland-server0 libwayland-egl1 libwayland-cursor0 libdecor-0-0 \
        libgl1 libegl1 libgles2 libdrm2 libgbm1 \
        libasound2t64 libpulse0 libdbus-1-3 libibus-1.0-5 libudev1 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/vulkan/icd.d \
    && printf '{"file_format_version":"1.0.1","ICD":{"library_path":"libGLX_nvidia.so.0","api_version":"1.4.329"}}\n' \
        > /etc/vulkan/icd.d/nvidia_icd.json

COPY --from=build /src/iris/build/iris /usr/local/bin/iris
COPY run.py /screenshotter/run.py
COPY gamelist.txt /screenshotter/gamelist.txt
COPY titlemap.txt /screenshotter/titlemap.txt
COPY screenshotter.toml /screenshotter/screenshotter.toml

RUN chmod +x /screenshotter/run.py /usr/local/bin/iris

ENTRYPOINT ["python3", "/screenshotter/run.py"]
