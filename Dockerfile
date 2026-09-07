# Build environment. Everything is built here; nothing is installed on the host.
#
# The version is pinned deliberately. A floating tag brought in a new compiler
# mid-session, and with it new lints that failed a build nothing in the project
# had changed. Upgrading should be a decision, not a surprise.
FROM rust:1.98.0-bookworm

# mingw-w64 is used purely as the linker for cross-compiling a Windows .exe.
# It is a build tool, not a dependency of the application: nothing third-party
# ends up inside the produced binary.
# zip and unzip are reference implementations used by the test suite to check
# interoperability in both directions. They are never linked into the product.
RUN apt-get update && apt-get install -y --no-install-recommends \
        mingw-w64 \
        file \
        zip \
        unzip \
        fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-gnu \
 && rustup component add clippy rustfmt

# The project has no dependencies at all, so builds never need the network.
#
# LANG matters for the tests: under the default POSIX locale Info-ZIP's unzip
# escapes non-ASCII entry names instead of writing them out, which would make a
# correct archive look wrong. C.UTF-8 needs no locale files.
ENV CARGO_NET_OFFLINE=true \
    CARGO_TERM_COLOR=always \
    LANG=C.UTF-8

WORKDIR /work
CMD ["bash"]
