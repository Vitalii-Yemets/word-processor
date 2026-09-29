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

# The newer kinds of colour glyph: a COLR version 1 font, whose glyphs are
# trees of gradients, transforms and blend modes; an sbix font, Apple's
# pictures per glyph; and an SVG font, a drawing per glyph. No font of any of
# the three is packaged for this Debian, so fontTools - which is, and is an
# implementation of the format that is not this program - writes them, from
# tools/make-colour-fonts.py, the way LibreOffice writes the .doc files the
# old-format readers are held to. Every glyph is in the private use area, so
# none of them stands in for a real emoji anywhere else. Test fonts: nothing
# of them reaches the product.
COPY tools/make-colour-fonts.py /tmp/make-colour-fonts.py
RUN apt-get update && apt-get install -y --no-install-recommends python3-fonttools \
 && python3 /tmp/make-colour-fonts.py /usr/share/fonts/truetype/wp-colour \
 && rm -rf /var/lib/apt/lists/* /tmp/make-colour-fonts.py

# Russian and French dictionaries, for the grammar rules of those languages:
# every correction a rule offers is held to the language's own word list, so
# that a rule cannot put a misspelling where a mistake was. Test data like the
# English and German ones above, in a layer of its own so that adding them did
# not mean installing everything above again.
RUN apt-get update && apt-get install -y --no-install-recommends hunspell-ru hunspell-fr-classical \
 && rm -rf /var/lib/apt/lists/*

# More bilingual dictionaries, beside the English-German one above: German
# into English, so that the pair goes both ways, and English with French both
# ways and into Russian, so that the Translator is held to more than one pair
# and to dictionaries FreeDict writes in more than one way. Test data again,
# never shipped.
RUN apt-get update && apt-get install -y --no-install-recommends \
        dict-freedict-deu-eng \
        dict-freedict-eng-fra \
        dict-freedict-fra-eng \
        dict-freedict-eng-rus \
 && rm -rf /var/lib/apt/lists/*

# An input method, and something to type into it with. uim-xim is an X
# Input Method server, and byeoru the Korean input method it runs — one that
# needs no dictionary, so what a key sequence composes is the same every
# time — which is what the X shell's composing is held to: a program that
# spoke only to a server of its own writing would be held to its own
# reading of the protocol and nothing else. xdotool presses the keys through
# the server's own test extension, so they arrive as a person's would. Test
# tools like Xvfb and wtype: the shell speaks the protocol itself and links
# to none of them. In a layer of its own, after the others.
RUN apt-get update && apt-get install -y --no-install-recommends \
        uim-xim \
        uim-byeoru \
        xdotool \
 && rm -rf /var/lib/apt/lists/*

# The other end of a drag between programs: GTK, reached from Python, which
# is a drag source and a drop target on X and on Wayland alike, written by
# somebody else — tools/dnd-peer.py is a window of it that gives or takes
# what is dragged, and says what it was given. The X shell's and the
# Wayland shell's dragging and dropping are held to it, xdotool and the
# test's own pointer moving the pointer. Test tools: nothing of GTK reaches
# the product. In a layer of its own, after the others.
RUN apt-get update && apt-get install -y --no-install-recommends \
        python3-gi \
        gir1.2-gtk-3.0 \
 && rm -rf /var/lib/apt/lists/*

# What a screen reader on Linux reads a program through: the accessibility
# bus and its registry (at-spi2-core), and the library a screen reader
# reads it with (Atspi, which Orca is written on), reached from Python —
# tools/atspi-reader.py walks what the program says of itself, reads its
# text and presses its buttons, the way a screen reader would. The program
# speaks D-Bus and AT-SPI itself; test tools, nothing of them reaches the
# product. In a layer of its own, after the others.
RUN apt-get update && apt-get install -y --no-install-recommends \
        at-spi2-core \
        gir1.2-atspi-2.0 \
 && rm -rf /var/lib/apt/lists/*

# The desktop's portal, which is how a program on Wayland photographs the
# screen — the protocol will not let a client look at anything but its own
# windows. xdg-desktop-portal is the front every program talks to over
# D-Bus, and xdg-desktop-portal-wlr the half that does the work on sway: it
# runs grim for the whole screen, and grim over slurp for a rectangle a
# person drags out, which is Word's Screen Clipping. The front offers the
# screenshot only beside somewhere to ask permission, which on this
# version is xdg-desktop-portal-gtk's; and the wlr half starts the screen
# recording too, which wants PipeWire running. The program speaks D-Bus
# and the portal's interface itself; test tools, nothing of them reaches
# the product. In a layer of its own, after the others.
RUN apt-get update && apt-get install -y --no-install-recommends \
        xdg-desktop-portal \
        xdg-desktop-portal-wlr \
        xdg-desktop-portal-gtk \
        pipewire \
        slurp \
 && rm -rf /var/lib/apt/lists/*

# Another program's clipboard on X, for what is too big to go in one piece:
# xclip hands a selection over in pieces (INCR) once it is bigger than a
# request should carry, and takes one handed over that way, so the X
# shell's pieces are held to it both ways round. A test tool; the shell
# speaks the conventions itself. In a layer of its own, after the others.
RUN apt-get update && apt-get install -y --no-install-recommends xclip \
 && rm -rf /var/lib/apt/lists/*

# The names of the world's languages and countries in other languages, as
# Debian's translators give them: what the German catalogue's names of the
# languages a document's text is marked as are held to, since a list of
# names written from memory is a list nobody has checked. Test data; the
# program reads its catalogue, not these. In a layer of its own.
RUN apt-get update && apt-get install -y --no-install-recommends iso-codes \
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
