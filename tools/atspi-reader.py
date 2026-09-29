#!/usr/bin/env python3
"""A screen reader's view of a program, for the shells' tests.

It reads through Atspi, the library Orca is written on, over the desktop's
accessibility bus — so what it reads is what the program said of itself
in AT-SPI, taken by somebody else's reading of the protocol.

    atspi-reader.py NAME PRESS

finds the application called NAME on the desktop, and then says, a line at
a time on standard output:

    node DEPTH ROLE | NAME | STATES     for everything in its tree
    text WORDS / caret N / selection A B        of its document
    word WORDS A B      the word at the eighth character
    extents X Y W H     where the first character is, in the window
    action NAME         what pressing the button called PRESS is called
    pressed / checked yes|no    after pressing it
    event caret N       when the caret moves, after selecting 0 to 5
    done

or "missing NAME" if the application never appears. It never outlives
half a minute.
"""

import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402


def say(*words):
    print(*words, flush=True)


def find(name, wait=15):
    until = time.time() + wait
    while time.time() < until:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            if app is not None and app.get_name() == name:
                return app
        time.sleep(0.2)
    return None


def states(node):
    wanted = ["CHECKED", "SELECTED", "FOCUSED", "ENABLED", "EDITABLE", "MULTI_LINE"]
    held = node.get_state_set()
    return ",".join(s.lower() for s in wanted if held.contains(getattr(Atspi.StateType, s)))


def walk(node, depth, found):
    say("node", depth, node.get_role_name(), "|", node.get_name(), "|", states(node))
    found.append(node)
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            walk(child, depth + 1, found)


def main():
    name, press = sys.argv[1], sys.argv[2]
    app = find(name)
    if app is None:
        say("missing", name)
        return
    found = []
    walk(app, 0, found)

    document = next((n for n in found if n.get_role() == Atspi.Role.DOCUMENT_TEXT), None)
    if document is not None:
        say("text", Atspi.Text.get_text(document, 0, -1))
        say("caret", Atspi.Text.get_caret_offset(document))
        chosen = Atspi.Text.get_selection(document, 0)
        say("selection", chosen.start_offset, chosen.end_offset)
        word = Atspi.Text.get_string_at_offset(document, 7, Atspi.TextGranularity.WORD)
        say("word", word.content.strip(), word.start_offset, word.end_offset)
        box = Atspi.Text.get_character_extents(document, 0, Atspi.CoordType.WINDOW)
        say("extents", box.x, box.y, box.width, box.height)

    button = next((n for n in found if n.get_name() == press), None)
    if button is not None:
        say("action", Atspi.Action.get_action_name(button, 0))
        Atspi.Action.do_action(button, 0)
        say("pressed")
        time.sleep(0.5)
        held = button.get_state_set().contains(Atspi.StateType.CHECKED)
        say("checked", "yes" if held else "no")

    if document is not None:
        loop = GLib.MainLoop()

        def moved(event):
            say("event caret", event.detail1)
            loop.quit()

        listener = Atspi.EventListener.new(moved)
        listener.register("object:text-caret-moved")
        Atspi.Text.set_selection(document, 0, 0, 5)
        GLib.timeout_add_seconds(5, loop.quit)
        loop.run()
    say("done")


if __name__ == "__main__":
    main()
