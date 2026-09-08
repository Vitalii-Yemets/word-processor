#!/usr/bin/env bash
# Fetches the button drawings from Fluent UI System Icons.
#
# Microsoft publishes the icon set Office and Windows are drawn with under the
# MIT licence. Using it rather than drawing our own gets the right visual
# language for nothing, and it is the same language the program is trying to
# match. Word's *own* icons are not this: they are Microsoft's product art and
# are not licensed for reuse.
#
# The files land in crates/wp-app/assets/icons and are committed, so a build
# needs no network. Re-run this only to add an icon or take one away.
set -euo pipefail
cd "$(dirname "$0")/.."

repository="https://raw.githubusercontent.com/microsoft/fluentui-system-icons/main"
out="crates/wp-app/assets/icons"
mkdir -p "$out"

# The directory an icon lives in is its display name, which is not derivable
# from the file name — "text_bullet_list_ltr" lives under "Text Bullet List LTR"
# and "textbox" under "TextBox". The index the repository generates is the only
# authority on that, so it is what gets consulted.
index="$(mktemp)"
trap 'rm -f "$index"' EXIT
curl -sfL -m 120 -o "$index" "$repository/icons_regular.md"

grep -oE 'assets/[^/]+/SVG/ic_fluent_[a-z0-9_]+_[0-9]+_regular\.svg' "$index" \
  | sed 's|assets/||; s|/SVG/|\t|; s|ic_fluent_||; s|_[0-9]*_regular\.svg||' \
  | sort -u -t"$(printf '\t')" -k2,2 > "$index.map"

fetch_one() {
  local directory="$1" name="$2" size="$3"
  local file="ic_fluent_${name}_${size}_regular.svg"
  if ! curl -sfL -m 60 -o "crates/wp-app/assets/icons/$file" \
      "$repository/assets/${directory// /%20}/SVG/$file"; then
    echo "MISSING $file" >&2
    return 1
  fi
}
export -f fetch_one
export repository

grep -v '^#' tools/icons.list | awk 'NF { print $2 }' | sort -u | while read -r name; do
  directory="$(awk -F"\t" -v want="$name" '$2 == want { print $1; exit }' "$index.map")"
  if [ -z "$directory" ]; then
    echo "NO SUCH ICON $name" >&2
    continue
  fi
  printf '%s\t%s\n' "$directory" "$name"
done | xargs -P 8 -d '\n' -I{} bash -c 'IFS="	" read -r d n <<< "{}"; fetch_one "$d" "$n" 20 && fetch_one "$d" "$n" 24'

curl -sfL -m 60 -o "$out/LICENSE.txt" "$repository/LICENSE"
echo "fetched $(ls "$out"/*.svg | wc -l) drawings"
