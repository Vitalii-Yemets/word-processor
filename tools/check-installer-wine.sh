#!/usr/bin/env bash
# Runs the Windows installer under Wine and checks what it leaves behind, and
# what it leaves behind once it is uninstalled.
#
# Why: the installer's Windows half is the registry, the Start menu, the list
# of installed programs and the shell's associations, and nothing in the
# build image is Windows. Wine is: a registry, a shell with its link object
# and its ShellExecute, a command interpreter — the same documented
# interfaces the installer is written against, implemented by somebody else.
# That is not Windows, and a check that passes here has not been run on
# Windows; but it has been run.
#
# Wine is large and nothing else needs it, so it is not in the build image:
# this installs it into a container of its own, thrown away afterwards. Not
# the build image's Debian either: its Wine is 8, which has no
# bcryptprimitives.dll, and every Rust program asks that for its random
# numbers before main — none starts on it. Debian 13's Wine is 10.
#
# Run on the host, after ./x.sh win:
#
#   docker run --rm -v "$PWD:/work" -w /work debian:trixie \
#     bash tools/check-installer-wine.sh

set -euo pipefail

built=/work/dist/word-processor-setup.exe
[ -f "$built" ] || { echo "no $built: run ./x.sh win first" >&2; exit 2; }
# Run from the container's own disk: Wine maps a program's file into memory,
# and a folder shared from a Windows host does not map — the program starts
# on pages of nothing and fails before its first line.
setup=$(mktemp -d)/word-processor-setup.exe
cp "$built" "$setup"

failures=0
pass() { echo "ok: $*"; }
fail() { echo "FAIL: $*"; failures=$((failures + 1)); }
check() { # check <what> <command...>
  local what=$1
  shift
  if "$@" >/dev/null 2>&1; then pass "$what"; else fail "$what"; fi
}

# The manifest is inside the program, not beside it: without it Windows
# asks for an administrator for anything called setup. Read off the file.
check "the installer says it needs no administrator" grep -qa 'level="asInvoker"' "$setup"

if ! command -v wine >/dev/null || ! command -v Xvfb >/dev/null \
  || [ ! -d /usr/share/fonts/truetype/dejavu ]; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
    wine64 wine xvfb procps fonts-dejavu-core >/dev/null
fi
export WINEDEBUG=-all WINEPREFIX=/tmp/wine-check
rm -rf "$WINEPREFIX"
wineboot -i >/dev/null 2>&1
# The program reads the fonts in the Windows folder for them, and Wine keeps
# its own elsewhere: without some there, the program says it has none and
# stops, and a document opened is not seen to be opened.
cp /usr/share/fonts/truetype/dejavu/*.ttf "$WINEPREFIX/drive_c/windows/Fonts/"
# A screen for the program to be started on, when a document is opened.
Xvfb :57 -screen 0 1400x900x24 >/dev/null 2>&1 &
screen=$!
export DISPLAY=:57

windows() { wine cmd /c "$1" 2>/dev/null | tr -d '\r'; }
query() { wine reg query "$@" 2>/dev/null | tr -d '\r'; }
local_data=$(windows 'echo %LOCALAPPDATA%' | tail -1 | sed 's/ *$//')
roaming=$(windows 'echo %APPDATA%' | tail -1 | sed 's/ *$//')
folder=$(winepath -u "$local_data\\Programs\\Word Processor")
link=$(winepath -u "$roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Word Processor.lnk")
classes='HKCU\Software\Classes'
uninstall_key='HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\WordProcessor'

echo "--- installing"
check "the installer ran" timeout 300 wine "$setup" --quiet
wineserver -w
for file in word-processor.exe wp.exe uninstall.exe installed.txt; do
  check "$file is in the folder" test -f "$folder/$file"
done
check "the uninstaller left behind needs no administrator either" \
  grep -qa 'level="asInvoker"' "$folder/uninstall.exe"

check "a Word document opens with the program" \
  bash -c "wine reg query '$classes\\WordProcessor.docx\\shell\\open\\command' /ve 2>/dev/null \
    | grep -q 'word-processor.exe\" \"%1\"'"
check "a template's verb is New" \
  bash -c "wine reg query '$classes\\WordProcessor.dotx\\shell' /ve 2>/dev/null | grep -q 'new'"
check "New makes a document from it" \
  bash -c "wine reg query '$classes\\WordProcessor.dotx\\shell\\new\\command' /ve 2>/dev/null \
    | grep -q 'word-processor.exe\" \"%1\"'"
check "and Open opens the template itself" \
  bash -c "wine reg query '$classes\\WordProcessor.dotx\\shell\\open\\command' /ve 2>/dev/null \
    | grep -q -- '--open \"%1\"'"
check "a kind nothing had is this program's" \
  bash -c "wine reg query '$classes\\.docx' /ve 2>/dev/null | grep -q 'WordProcessor.docx'"
check "a kind another program has is not taken" \
  bash -c "! wine reg query '$classes\\.txt' /ve 2>/dev/null | grep -q 'WordProcessor'"
check "but this program is offered for it" \
  bash -c "wine reg query '$classes\\.txt\\OpenWithProgids' 2>/dev/null | grep -q 'WordProcessor.txt'"
check "Explorer's New menu offers a Word document" \
  bash -c "wine reg query '$classes\\.docx\\WordProcessor.docx\\ShellNew' /v NullFile 2>/dev/null | grep -q NullFile"
check "the program is on the Default Apps page" \
  bash -c "wine reg query 'HKCU\\Software\\RegisteredApplications' /v WordProcessor 2>/dev/null | grep -q Capabilities"
check "the program is in the list of installed programs" \
  bash -c "wine reg query '$uninstall_key' /v DisplayName 2>/dev/null | grep -q 'Word Processor'"
check "with the uninstaller to take it off" \
  bash -c "wine reg query '$uninstall_key' /v UninstallString 2>/dev/null | grep -q 'uninstall.exe\" --uninstall'"
check "and nothing to change or repair" \
  bash -c "wine reg query '$uninstall_key' /v NoModify 2>/dev/null | grep -q 0x1"
check "the Start menu has it" test -f "$link"
check "and the shortcut points at the program" \
  bash -c "tr -d '\\000' < '$link' | grep -q 'word-processor.exe'"

echo "--- opening a document the way a double-click does"
# Windows' HKEY_CLASSES_ROOT is the machine's classes with the person's over
# them. Wine's is the machine's alone, so its shell does not see a program
# installed for one person at all. What the installer wrote under the
# person's classes is copied into the machine's, as it stands — which is
# the merging Windows does by itself — and then the shell is asked to open
# a document, which is what a double-click asks it.
merged=(.docx .dotx WordProcessor.docx WordProcessor.dotx)
for key in "${merged[@]}"; do
  wine reg copy "$classes\\$key" "HKLM\\Software\\Classes\\$key" /s /f >/dev/null 2>&1
done
documents=$(mktemp -d)
# Real ones, written by the command line the installer put beside the
# program: the program stays open on a document it could open, and stops at
# once on one it could not.
wine "$folder/wp.exe" new "$(winepath -w "$documents/Letter.docx")" </dev/null >/dev/null 2>&1
wine "$folder/wp.exe" new "$(winepath -w "$documents/Invoice.dotx")" </dev/null >/dev/null 2>&1
running() { ps -eo args | grep -v grep | grep 'word-processor.exe' || true; }
# The program's command line, once it is running with the file and is still
# running a moment later; nothing if it never started or stopped.
opened_with() {
  local file=$1
  for _ in $(seq 1 80); do
    if running | grep -q "$file"; then
      sleep 3
      running | grep "$file" || true
      return
    fi
    sleep 0.25
  done
}
# A double-click asks the shell to open the file without naming a verb,
# which means the kind's own default one: ShellExecute with no verb, as
# rundll32's ShellExec_RunDLL calls it. Wine's `start` names "open", which is
# the right-click Open.
double_click() { (timeout 60 wine rundll32 shell32.dll,ShellExec_RunDLL "$(winepath -w "$1")" >/dev/null 2>&1 &); }
open_verb() { (timeout 60 wine start /unix "$1" >/dev/null 2>&1 &); }

double_click "$documents/Letter.docx"
line=$(opened_with Letter.docx)
check "a double-clicked document opens in the installed program" test -n "$line"
wineserver -k 2>/dev/null || true
double_click "$documents/Invoice.dotx"
line=$(opened_with Invoice.dotx)
check "a double-clicked template is opened by the installed program" test -n "$line"
check "to make a new document from it, which is New" bash -c "! grep -q -- '--open' <<<'$line'"
wineserver -k 2>/dev/null || true
open_verb "$documents/Invoice.dotx"
line=$(opened_with Invoice.dotx)
check "while Open on a template opens the template itself" bash -c "grep -q -- '--open' <<<'$line'"
wineserver -k 2>/dev/null || true
# What Explorer's New ▸ Word Document makes: a file of no bytes.
: > "$documents/Empty.docx"
double_click "$documents/Empty.docx"
line=$(opened_with Empty.docx)
check "an empty Word document from the New menu opens as a blank one" test -n "$line"
wineserver -k 2>/dev/null || true
for key in "${merged[@]}"; do
  wine reg delete "HKLM\\Software\\Classes\\$key" /f >/dev/null 2>&1
done

echo "--- uninstalling"
check "the uninstaller ran" timeout 300 wine "$folder/uninstall.exe" --uninstall --quiet
wineserver -w
# The uninstaller cannot remove itself while it runs; the command
# interpreter removes it, and the folder, once it has stopped.
for _ in $(seq 1 40); do [ -d "$folder" ] || break; sleep 0.25; done
check "the folder is gone" test ! -e "$folder"
check "the kinds are gone" \
  bash -c "! wine reg query '$classes\\WordProcessor.docx' 2>/dev/null | grep -q WordProcessor"
check "the kind this program took has no program again" \
  bash -c "! wine reg query '$classes\\.docx' /ve 2>/dev/null | grep -q WordProcessor"
check "and is not offered" \
  bash -c "! wine reg query '$classes\\.docx\\OpenWithProgids' 2>/dev/null | grep -q WordProcessor"
check "the other kind keeps what it had" \
  bash -c "wine cmd /c assoc .txt 2>/dev/null | grep -qi 'txtfile'"
check "the Default Apps page does not list it" \
  bash -c "! wine reg query 'HKCU\\Software\\RegisteredApplications' 2>/dev/null | grep -q WordProcessor"
check "the list of installed programs does not" \
  bash -c "! wine reg query '$uninstall_key' 2>/dev/null | grep -q DisplayName"
check "the Start menu does not" test ! -e "$link"

kill "$screen" 2>/dev/null || true
rm -rf "$documents" "$WINEPREFIX"
if [ "$failures" -gt 0 ]; then
  echo "$failures checks failed"
  exit 1
fi
echo "all checks passed"
