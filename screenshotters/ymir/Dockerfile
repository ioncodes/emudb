# syntax=docker/dockerfile:1.7

FROM ubuntu:24.04 AS build

ENV DEBIAN_FRONTEND=noninteractive
ENV VCPKG_ROOT=/opt/vcpkg
ENV VCPKG_FORCE_SYSTEM_BINARIES=1

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential git ninja-build pkg-config curl zip unzip tar \
        ca-certificates gpg wget python3 autoconf automake autoconf-archive libtool \
        bison flex \
        libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxfixes-dev libxi-dev \
        libxss-dev libxkbcommon-dev libwayland-dev wayland-protocols libdrm-dev libgbm-dev \
        libgl1-mesa-dev libegl1-mesa-dev libgles2-mesa-dev \
        libasound2-dev libpulse-dev libdbus-1-dev libudev-dev libibus-1.0-dev \
        libvulkan-dev glslc \
    && rm -rf /var/lib/apt/lists/*

# Ubuntu 24.04's packaged cmake (3.28) predates the `string(JSON ... STRING_ENCODE)`
# mode vcpkg's SPDX/SBOM generation relies on, which breaks every port install.
# Pull a current cmake from Kitware's apt repo instead of the distro package.
RUN wget -qO- https://apt.kitware.com/keys/kitware-archive-latest.asc \
        | gpg --dearmor -o /usr/share/keyrings/kitware-archive-keyring.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/kitware-archive-keyring.gpg] https://apt.kitware.com/ubuntu/ noble main" \
        > /etc/apt/sources.list.d/kitware.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

RUN git clone https://github.com/microsoft/vcpkg.git "$VCPKG_ROOT" \
    && "$VCPKG_ROOT/bootstrap-vcpkg.sh" -disableMetrics

COPY --from=emulator . /src/ymir

WORKDIR /src/shotter
COPY vcpkg.json vcpkg-configuration.json ./
COPY CMakeLists.txt ./
COPY src ./src

RUN --mount=type=cache,target=/root/.cache/vcpkg \
    --mount=type=cache,target=/opt/vcpkg/buildtrees \
    --mount=type=cache,target=/opt/vcpkg/downloads \
    cmake -S . -B build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_TOOLCHAIN_FILE="$VCPKG_ROOT/scripts/buildsystems/vcpkg.cmake" \
        -DYMIR_CORE_DIR=/src/ymir \
        -DCMAKE_CXX_FLAGS="-flax-vector-conversions" \
    && cmake --build build --target ymir-screenshotter-worker -j"$(nproc)"

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        python3 \
        libvulkan1 mesa-vulkan-drivers libgl1 libegl1 libgles2 \
        libx11-6 libxext6 libxrandr2 libxcursor1 libxfixes3 libxi6 libxkbcommon0 \
        libwayland-client0 libasound2t64 libpulse0 libdbus-1-3 libudev1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/shotter/build/ymir-screenshotter-worker /usr/local/bin/ymir-screenshotter-worker

RUN chmod +x /usr/local/bin/ymir-screenshotter-worker

ENTRYPOINT ["/usr/local/bin/ymir-screenshotter-worker"]
