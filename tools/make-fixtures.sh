#!/usr/bin/env bash
# Produces fixtures for testing the decoder against another encoder's output.
#
# Why: our encoder emits only fixed Huffman codes, while Word and every other
# program emit dynamic ones. Without these fixtures the most important branch
# of the decoder would go untested.
#
# The streams come from the system gzip. The DEFLATE body is cut out of the .gz
# file (the header is exactly 10 bytes when -n is used, the trailer 8), and the
# original size and CRC-32 are taken from that trailer - which incidentally
# validates our CRC-32 against a reference implementation.
#
# Run: docker compose run --rm dev bash tools/make-fixtures.sh

set -euo pipefail

FIXTURES=/work/crates/wp-deflate/tests/fixtures
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$FIXTURES"
rm -f "$FIXTURES"/*.deflate "$FIXTURES"/manifest.txt

cd "$WORK"

# 1. WordprocessingML markup - the dominant kind of data inside a .docx.
for i in $(seq 1 800); do
    printf '<w:p><w:pPr><w:pStyle w:val="Normal"/><w:spacing w:after="%d"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="24"/></w:rPr><w:t xml:space="preserve">Paragraph number %d of the sample document.</w:t></w:r></w:p>\n' "$((i % 240))" "$i"
done > markup.bin

# 2. Multilingual text: multi-byte UTF-8 has very different byte statistics
#    from ASCII and drives the encoder into other parts of the code alphabet.
for i in $(seq 1 400); do
    printf '<w:p><w:r><w:t>%d The quick brown fox. Съешь ещё этих булок. Ταχίστη αλώπηξ. نص حكيم له سر قاطع. דג סקרן שט בים. वह क्षमा का प्रतीक है। เป็นมนุษย์สุดประเสริฐ 永和九年歲在癸丑 다람쥐 헌 쳇바퀴 🌍📄</w:t></w:r></w:p>\n' "$i"
done > multilingual.bin

# 3. Incompressible data - gzip will store it in uncompressed blocks.
head -c 40000 /dev/urandom > noise.bin

# 4. A mix of compressible and incompressible data, so one stream contains
#    blocks of several different types.
cat markup.bin noise.bin multilingual.bin > mixed.bin

# 5. Long runs - maximum match lengths at short distances.
#    No pipe from yes: it gets SIGPIPE, and under pipefail that kills the script.
printf 'ABCDEFGHIJKLMNOP\n%.0s' $(seq 6000) > repeats.bin

# 6. Degenerate cases.
printf '' > empty.bin
printf 'x' > single.bin

for name in markup multilingual noise mixed repeats empty single; do
    for level in 1 6 9; do
        gzip -"$level" -n -c "$name.bin" > "$name-$level.gz"

        total=$(stat -c%s "$name-$level.gz")

        # Confirm the header really is the short form: FLG (byte 3) must be
        # zero, otherwise optional fields follow it.
        flg=$(od -An -tu1 -j 3 -N 1 -v "$name-$level.gz" | tr -d ' ')
        if [ "$flg" != "0" ]; then
            echo "unexpected FLG=$flg in $name-$level.gz" >&2
            exit 1
        fi

        body=$((total - 18))
        tail -c +11 "$name-$level.gz" | head -c "$body" > "$FIXTURES/$name-$level.deflate"

        # gzip trailer: CRC-32 then original size, both little-endian.
        crc=$(od -An -tu4 -j $((total - 8)) -N 4 -v "$name-$level.gz" | tr -d ' ')
        size=$(od -An -tu4 -j $((total - 4)) -N 4 -v "$name-$level.gz" | tr -d ' ')

        echo "$name-$level $size $crc" >> "$FIXTURES/manifest.txt"
    done
done

echo "done, fixtures: $(wc -l < "$FIXTURES/manifest.txt")"
gzip --version | head -1
