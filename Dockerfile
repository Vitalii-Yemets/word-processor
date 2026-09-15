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
# look at. Lohit, TLWG and IPA are the smallest set that answers for the three
# scripts whose rules are written out in this program: Devanagari, Thai and
# Japanese. Nanum answers for Korean, whose syllables the line breaking rules
# now separate: without it the one thing the generated tables added there could
# not be looked at, and the sample document's Korean line drew nothing at all.
# The URW set is the other kind of font altogether: PostScript outlines in a
# CFF table rather than quadratic ones in glyf, which is what every .otf file
# holds and what nothing on this image had until they were added. Inter is a
# variable font - one file that is a whole family, with axes and deltas rather
# than one weight - and nothing else here is. Noto Color Emoji draws its glyphs
# as pictures rather than outlines, which is the other half of what an emoji
# can be and cannot be tested without one.
# None of them is linked into the product or shipped with it; the program reads
# whatever fonts the machine it runs on has.
#
# The dictionaries are here for the same reason and on the same terms: a
# spelling checker that reads the affix rules cannot be believed against a
# dictionary written for the test. English shows the ordinary case, German the
# one where words are written run together. The thesaurus is the same again
# for the words that mean the same, and the English-German dictionary for what
# a word is in another language: a dictzip that cannot be read without reading
# a real one. None is shipped with the product; the program reads whatever the
# machine has.
#
# LibreOffice, without its windows, is here to write the files the readers of
# the older formats are tested against: a binary .doc from an implementation
# that is not this one is the only kind worth reading, because a file written
# by the reader's own author proves the author's understanding and nothing
# else. It is a test tool like zip and unzip, and nothing of it reaches the
# product.
#
# Xvfb is an X server without a screen: it draws into a file. It is here so
# that the Linux shell can be run against a real X server, and what it drew
# read back off the server's own frame buffer, in a container that has no
# display. A test tool too; the shell speaks the X protocol itself and links
# to nothing.
RUN apt-get update && apt-get install -y --no-install-recommends \
        mingw-w64 \
        file \
        zip \
        unzip \
        fonts-dejavu-core \
        fonts-lohit-deva \
        fonts-thai-tlwg \
        fonts-ipafont-gothic \
        fonts-nanum \
        fonts-urw-base35 \
        fonts-inter-variable \
        fonts-noto-color-emoji \
        hunspell-en-us \
        hunspell-de-de \
        mythes-en-us \
        dict-freedict-eng-deu \
        libreoffice-writer-nogui \
        xvfb \
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
