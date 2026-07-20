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

# Install base dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    curl \
    git \
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

# Download ARM toolchain based on target platform
# See https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads
ARG ARM_GCC_VERSION=15.2.rel1
RUN PLATFORM="${TARGETPLATFORM:-linux/$(dpkg --print-architecture)}" && \
    case "${PLATFORM}" in \
    "linux/amd64") \
        ARCH_SUFFIX="x86_64" ;; \
    "linux/arm64") \
        ARCH_SUFFIX="aarch64" ;; \
    *) \
        echo "Unsupported platform: ${PLATFORM}" && exit 1 ;; \
    esac && \
    wget https://developer.arm.com/-/media/Files/downloads/gnu/${ARM_GCC_VERSION}/binrel/arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi.tar.xz && \
    tar -xf arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi.tar.xz && \
    mv arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi /opt/ && \
    rm arm-gnu-toolchain-${ARM_GCC_VERSION}-${ARCH_SUFFIX}-arm-none-eabi.tar.xz && \
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
RUN rustup target install thumbv8m.main-none-eabihf
RUN rustup target install \
    x86_64-unknown-linux-gnu \
    aarch64-unknown-linux-gnu \
    x86_64-pc-windows-gnu \
    aarch64-pc-windows-gnullvm \
    x86_64-apple-darwin \
    aarch64-apple-darwin

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
