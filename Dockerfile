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
# Japanese — and the other nine written on Devanagari's plan, a Lohit font
# for each and LKLUG for Sinhala, since the rules for each differ and each
# has to be looked at. Nanum answers for Korean, whose syllables the line breaking rules
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
# a real one. The hyphenation patterns are the same again for where a word may
# be broken: English has one level and German two, and the German reading of
# the file cannot be believed against a file written for the test. None is
# shipped with the product; the program reads whatever the machine has.
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
#
# OpenSSL and xmlsec are here for the signatures. A signature is arithmetic
# that is either right or worthless, and a program that only ever checked its
# own would go on passing with the padding written backwards: openssl makes a
# key and a certificate, signs what this program must accept, and checks what
# this program signs; xmlsec1 does the same for a whole XML signature, which
# is the canonicalisation and the digests as well as the arithmetic, and
# xmllint canonicalises on its own for the one step in between. All three are
# test tools like zip and LibreOffice: nothing of them reaches the product,
# which does every one of those things itself.
#
# Sway is a Wayland compositor that will run without a screen as well, and
# grim and wtype are the two clients that photograph what it shows and type
# into it. Together they are to the Wayland shell what Xvfb is to the X one:
# a real compositor on the other end of the socket, so that what this
# program sends can be seen to have arrived. Test tools again; the shell
# speaks the Wayland protocol itself and links to none of them.
RUN apt-get update && apt-get install -y --no-install-recommends \
        mingw-w64 \
        file \
        zip \
        unzip \
        fonts-dejavu-core \
        fonts-lohit-deva \
        fonts-lohit-beng-bengali \
        fonts-lohit-guru \
        fonts-lohit-gujr \
        fonts-lohit-orya \
        fonts-lohit-taml \
        fonts-lohit-telu \
        fonts-lohit-knda \
        fonts-lohit-mlym \
        fonts-lklug-sinhala \
        fonts-thai-tlwg \
        fonts-ipafont-gothic \
        fonts-nanum \
        fonts-urw-base35 \
        fonts-inter-variable \
        fonts-noto-color-emoji \
        hunspell-en-us \
        hunspell-de-de \
        hyphen-en-us \
        hyphen-de \
        mythes-en-us \
        dict-freedict-eng-deu \
        libreoffice-writer-nogui \
        xvfb \
        sway \
        grim \
        wtype \
        openssl \
        libxml2-utils \
        xmlsec1 \
    && rm -rf /var/lib/apt/lists/*

# Sway will not run as root — it refuses to start where it cannot drop
# privileges, and everything in this container is root — so the tests start
# it as somebody else. The user exists for that and for nothing else: the
# build, the program and every other test run as they did.
# Two locales besides the plain one, so that the tests can ask the C
# library what a German machine says about numbers, lengths and dates and
# get a German answer. Test data, like the fonts and the dictionaries: what
# a person's own machine says is whatever their own machine says.
RUN apt-get update && apt-get install -y --no-install-recommends locales \
 && sed -i 's/^# *\(de_DE.UTF-8\|en_US.UTF-8\)/\1/' /etc/locale.gen \
 && locale-gen \
 && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/sh compositor

RUN rustup target add x86_64-pc-windows-gnu \
 && rustup component add clippy rustfmt

# A CID-keyed PostScript font: outlines in a CFF table whose glyphs are split
# between several private dictionaries, each with subroutines of its own, and
# a table saying which glyph belongs to which. It is what every Chinese,
# Japanese and Korean .otf is, it is the one kind of CFF the URW set cannot
# stand for, and it is where cutting a font down for a PDF matters most — a
# collection of nineteen megabytes behind a page that uses forty ideographs.
# The package holds the serif and the bold as well; only the regular sans is
# kept, which is all the tests read. A test font like the rest: nothing of it
# reaches the product. In a layer of its own, after the others, so that
# adding it did not mean installing everything above it again.
RUN apt-get update && apt-get install -y --no-install-recommends fonts-noto-cjk \
 && find /usr/share/fonts/opentype/noto -name '*.tt[cf]' ! -name 'NotoSansCJK-Regular.ttc' -delete \
 && rm -rf /var/lib/apt/lists/*

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
