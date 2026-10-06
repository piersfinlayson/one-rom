# One ROM signing server. Build it from an export of the repository:
#
#   context=$(mktemp -d)
#   git archive HEAD | tar -x -C "$context"
#   docker build -f "$context/rust/signing-server/Dockerfile" -t onerom-signing-server "$context"

FROM rust:1-bookworm AS build

# Parallel build jobs increase memory use. Raise JOBS on a machine with more
# memory.
ARG JOBS=2

WORKDIR /src
COPY . .
RUN cargo build --manifest-path rust/Cargo.toml --release --locked --jobs "${JOBS}" \
        -p onerom-signing-server

FROM debian:bookworm-slim

# The server runs git to write the record. git runs ssh to push it.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git openssh-client \
 && rm -rf /var/lib/apt/lists/*

# ssh refuses to run as a user that doesn't exist.
RUN useradd --uid 10001 --home-dir /srv --no-create-home --shell /usr/sbin/nologin signer

COPY --from=build /src/rust/target/release/onerom-signing-server /usr/local/bin/

USER signer
WORKDIR /srv
ENTRYPOINT ["/usr/local/bin/onerom-signing-server"]
