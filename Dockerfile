# Build environment. Everything is built here; nothing is installed on the host.
FROM rust:1-bookworm

# mingw-w64 is used purely as the linker for cross-compiling a Windows .exe.
# It is a build tool, not a dependency of the application: nothing third-party
# ends up inside the produced binary.
RUN apt-get update && apt-get install -y --no-install-recommends \
        mingw-w64 \
        file \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-gnu \
 && rustup component add clippy rustfmt

# The project has no dependencies at all, so builds never need the network.
ENV CARGO_NET_OFFLINE=true \
    CARGO_TERM_COLOR=always

WORKDIR /work
CMD ["bash"]
