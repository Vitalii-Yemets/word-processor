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
#
# The fonts are here for the same reason: they are what the shaping tests are
# held against. DejaVu answers for Latin, Arabic and Hebrew and has nothing for
# the scripts that are written in syllables — so the rules for Devanagari and
# for Thai could be written and tested, and nothing could be drawn with them to
# look at. Lohit and TLWG are the smallest pair that answers for those two.
# None of them is linked into the product or shipped with it; the program reads
# whatever fonts the machine it runs on has.
RUN apt-get update && apt-get install -y --no-install-recommends \
        mingw-w64 \
        file \
        zip \
        unzip \
        fonts-dejavu-core \
        fonts-lohit-deva \
        fonts-thai-tlwg \
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
