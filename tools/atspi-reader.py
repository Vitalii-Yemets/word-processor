#!/usr/bin/env python3
"""A screen reader's view of a program, for the shells' tests.

It reads through Atspi, the library Orca is written on, over the desktop's
accessibility bus — so what it reads is what the program said of itself
in AT-SPI, taken by somebody else's reading of the protocol.

    atspi-reader.py NAME PRESS [OPENS [BOX VALUE]]

finds the application called NAME on the desktop, and then says, a line at
a time on standard output:

    node DEPTH ROLE | NAME | STATES | VALUE     for everything in its tree
    text WORDS / caret N / selection A B        of its document
    word WORDS A B      the word at the eighth character
    line A B            the line at the fifteenth, as the layout broke it
    attributes A B K=V,...      how the third character's stretch is set
    extents X Y W H     where the first character is, in the window
    action NAME         what pressing the button called PRESS is called
    pressed / checked yes|no    after pressing it
    written TEXT        after writing VALUE into the box called BOX
    event caret N       when the caret moves, after selecting 0 to 5
    event window activate NAME / event focused NAME / event text insert TEXT
                        what is heard on pressing the button called OPENS
    dialog DEPTH ROLE | NAME | STATES | VALUE   the dialog that opened
    done

or "missing NAME" if the application never appears. A VALUE is what a
box, a list box or a status strip holds, or where a scroll bar is. It never
outlives half a minute.
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


def value(node):
    interfaces = node.get_interfaces()
    role = node.get_role()
    if "Text" in interfaces and role != Atspi.Role.DOCUMENT_TEXT:
        return Atspi.Text.get_text(node, 0, -1)
    if "Value" in interfaces:
        current = Atspi.Value.get_current_value(node)
        return str(int(current)) if current == int(current) else str(current)
    return ""


def line(prefix, node, depth):
    say(prefix, depth, node.get_role_name(), "|", node.get_name(), "|", states(node), "|", value(node))


def walk(node, depth, found):
    line("node", node, depth)
    found.append(node)
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            walk(child, depth + 1, found)


def listen(events, seconds, stop_on_first=False):
    """Registers for the events, and gives them a while to come."""
    loop = GLib.MainLoop()
    listeners = []

    def heard(event):
        kind = event.type
        source = event.source.get_name() if event.source is not None else ""
        if kind.startswith("object:text-caret-moved"):
            say("event caret", event.detail1)
        elif kind.startswith("window:activate"):
            say("event window activate", source)
        elif kind.startswith("object:state-changed:focused") and event.detail1 == 1:
            say("event focused", source)
        elif kind.startswith("object:text-changed:insert"):
            say("event text insert", event.any_data)
        if stop_on_first:
            loop.quit()

    for name in events:
        listener = Atspi.EventListener.new(heard)
        listener.register(name)
        listeners.append((listener, name))
    GLib.timeout_add(int(seconds * 1000), loop.quit)
    return loop, listeners


def forget(listeners):
    for listener, name in listeners:
        listener.deregister(name)


def main():
    name, press = sys.argv[1], sys.argv[2]
    opens = sys.argv[3] if len(sys.argv) > 3 else None
    box, written = (sys.argv[4], sys.argv[5]) if len(sys.argv) > 5 else (None, None)
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
        row = Atspi.Text.get_string_at_offset(document, 15, Atspi.TextGranularity.LINE)
        say("line", row.start_offset, row.end_offset)
        attributes, start, end = Atspi.Text.get_attribute_run(document, 2, False)
        pairs = ",".join(f"{k}={attributes[k]}" for k in sorted(attributes))
        say("attributes", start, end, pairs)
        box_at = Atspi.Text.get_character_extents(document, 0, Atspi.CoordType.WINDOW)
        say("extents", box_at.x, box_at.y, box_at.width, box_at.height)

    button = next((n for n in found if n.get_name() == press), None)
    if button is not None:
        say("action", Atspi.Action.get_action_name(button, 0))
        Atspi.Action.do_action(button, 0)
        say("pressed")
        time.sleep(0.5)
        held = button.get_state_set().contains(Atspi.StateType.CHECKED)
        say("checked", "yes" if held else "no")

    entry = next((n for n in found if box is not None and n.get_name() == box), None)
    if entry is not None:
        Atspi.EditableText.set_text_contents(entry, written)
        time.sleep(0.5)
        say("written", Atspi.Text.get_text(entry, 0, -1))

    if document is not None:
        loop, listeners = listen(["object:text-caret-moved"], 5, stop_on_first=True)
        Atspi.Text.set_selection(document, 0, 0, 5)
        loop.run()
        forget(listeners)

    opener = next((n for n in found if opens is not None and n.get_name() == opens), None)
    if opener is not None:
        loop, listeners = listen(
            ["window:activate", "object:state-changed:focused", "object:text-changed:insert"], 3
        )
        Atspi.Action.do_action(opener, 0)
        loop.run()
        forget(listeners)
        frame = app.get_child_at_index(0)
        for index in range(frame.get_child_count()):
            child = frame.get_child_at_index(index)
            if child is not None and child.get_role() == Atspi.Role.DIALOG:
                line("dialog", child, 0)
                for inner in range(child.get_child_count()):
                    line("dialog", child.get_child_at_index(inner), 1)
    say("done")


if __name__ == "__main__":
    main()
