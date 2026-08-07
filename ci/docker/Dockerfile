FROM ubuntu:24.04

# Metadata
LABEL org.opencontainers.image.title="One ROM Build Environment"
LABEL org.opencontainers.image.description="Multi-platform build environment for One ROM firmware and host tooling"
LABEL org.opencontainers.image.authors="Piers Finlayson <piers@piers.rocks>"
LABEL org.opencontainers.image.url="https://onerom.org"
LABEL org.opencontainers.image.source="https://github.com/piersfinlayson/one-rom"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.vendor="piers.rocks"

# Multiplatform support
ARG BUILDPLATFORM
ARG TARGETPLATFORM
ARG TARGETARCH

# Install base dependencies.
#
# libclang-dev is bindgen's, not the firmware's - nothing here is compiled with
# clang.  onerom-fw-emulator's build script parses the firmware headers with
# libclang to generate its FFI bindings, and panics if it cannot load it.
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    curl \
    git \
    libclang-dev \
    libcurl4-openssl-dev \
    libjson-c-dev \
    libusb-1.0-0-dev \
    libudev-dev \
    libzip-dev \
    mingw-w64 \
    nano \
    pkg-config \
    sudo \
    vim \
    vice \
    wget \
    && rm -rf /var/lib/apt/lists/*

# Download ARM toolchain based on target platform.
#
# No default: the version is pinned in ci/arm-toolchain-version and passed in by
# ci/docker/build.sh and ci/docker/build-release.sh.  Baking a default here is
# how this container came to be on a different compiler from everything else, so
# an unset value is an error rather than a silent fallback.
#
# Arm moved toolchain hosting to gitlab.arm.com from 15.3.Rel1 onwards;
# developer.arm.com carries 15.2.rel1 and earlier only.
ARG ARM_GCC_VERSION
RUN test -n "${ARM_GCC_VERSION}" || { \
        echo "ARM_GCC_VERSION build-arg is required - build via ci/docker/build.sh," && \
        echo "which passes the version pinned in ci/arm-toolchain-version." && \
        exit 1; \
    } && \
    PLATFORM="${TARGETPLATFORM:-linux/$(dpkg --print-architecture)}" && \
    case "${PLATFORM}" in \
    "linux/amd64") \
        ARCH_SUFFIX="x86_64" ;; \
    "linux/arm64") \
        ARCH_SUFFIX="aarch64" ;; \
    *) \
        echo "Unsupported platform: ${PLATFORM}" && exit 1 ;; \
    esac && \
    TARBALL="arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi.tar.xz" && \
    BASE_URL="https://gitlab.arm.com/api/v4/projects/tooling%2Fgnu-toolchains-for-arm/packages/generic/gnu-toolchain/${ARM_GCC_VERSION}" && \
    wget "${BASE_URL}/${TARBALL}" && \
    wget "${BASE_URL}/${TARBALL}.sha256asc" && \
    sha256sum -c "${TARBALL}.sha256asc" && \
    tar -xf "${TARBALL}" && \
    mv arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi /opt/ && \
    rm "${TARBALL}" "${TARBALL}.sha256asc" && \
    ln -s /opt/arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi /opt/arm-toolchain

# Create build user with passwordless sudo
RUN useradd -m -s /bin/bash build && \
    echo "build ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Switch to build user
USER build
WORKDIR /home/build

# Set TOOLCHAIN environment variable
ENV TOOLCHAIN="/opt/arm-toolchain/bin"

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/home/build/.cargo/bin:${PATH}"
RUN cargo install wasm-pack cross
# Nightly toolchain (with clippy/rustfmt and the RP2350 target) for building
# One ROM Lab, the hardware tester, which pins nightly via its
# rust-toolchain.toml.
RUN rustup toolchain install nightly \
    --component clippy,rustfmt \
    --target thumbv8m.main-none-eabihf
RUN rustup target install \
    x86_64-unknown-linux-gnu \
    aarch64-unknown-linux-gnu \
    x86_64-pc-windows-gnu \
    aarch64-pc-windows-gnullvm \
    x86_64-apple-darwin \
    aarch64-apple-darwin \
    wasm32-unknown-emscripten

# Install the Emscripten SDK (for building One ROM Lens to wasm).  emcc/emar are
# placed on PATH; EM_CONFIG points emcc at the activated SDK's config.
#
# Pinned, for the same reason as the Arm toolchain and with the same no-default
# rule: the version comes from ci/emscripten-version via ci/docker/build.sh.
# Tracking "latest" here means an emscripten release can break the container
# with no change on our side.
ARG EMSCRIPTEN_VERSION
ENV EMSDK=/home/build/emsdk
RUN test -n "${EMSCRIPTEN_VERSION}" || { \
        echo "EMSCRIPTEN_VERSION build-arg is required - build via ci/docker/build.sh," && \
        echo "which passes the version pinned in ci/emscripten-version." && \
        exit 1; \
    } && \
    git clone https://github.com/emscripten-core/emsdk.git "${EMSDK}" && \
    "${EMSDK}/emsdk" install "${EMSCRIPTEN_VERSION}" && \
    "${EMSDK}/emsdk" activate "${EMSCRIPTEN_VERSION}"
ENV EM_CONFIG="${EMSDK}/.emscripten"
ENV PATH="${EMSDK}:${EMSDK}/upstream/emscripten:${PATH}"

# Install probe-rs
RUN curl --proto '=https' --tlsv1.2 -LsSf https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.sh | sh

# Build and install picotool
RUN git clone https://github.com/raspberrypi/pico-sdk /home/build/pico-sdk && \
    git clone https://github.com/raspberrypi/picotool.git /home/build/picotool && \
    cd /home/build/picotool && \
    git submodule update --init && \
    mkdir build && cd build && \
    PICO_SDK_PATH=/home/build/pico-sdk cmake .. && \
    make -j$(nproc) && \
    sudo make install

# Copy scripts overs
COPY --chown=build:build container-scripts/clone.sh /home/build/clone.sh
COPY --chown=build:build container-scripts/welcome.sh /home/build/welcome.sh
COPY --chown=build:build container-scripts/copy-fw.sh /home/build/copy-fw.sh

# Run welcome script on interactive shell startup
RUN echo "source /home/build/welcome.sh" >> /home/build/.bashrc

# Create output directory
RUN mkdir -p /home/build/output

WORKDIR /home/build
VOLUME ["/home/build/output"]

# Build-time metadata (populated by CI/build scripts)
ARG BUILD_DATE
ARG VERSION
ARG VCS_REF
LABEL org.opencontainers.image.created=${BUILD_DATE}
LABEL org.opencontainers.image.version=${VERSION}
LABEL org.opencontainers.image.revision=${VCS_REF}

# Set build metadata as environment variables
ENV BUILD_DATE=${BUILD_DATE}
ENV VERSION=${VERSION}
ENV VCS_REF=${VCS_REF}

# Prevent container from exiting immediately
CMD ["sleep", "infinity"]
