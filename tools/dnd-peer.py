#!/usr/bin/env python3
"""The other end of a drag between programs, for the shells' tests.

A GTK window that gives something as a drag, or takes what is dropped on it
and says what it was. GTK speaks XDND on X and the data-device protocol on
Wayland; this program speaks neither itself, which is the point — what the
shells are held to is somebody else's reading of those protocols.

    dnd-peer.py source text WORDS    the window drags WORDS, as text
    dnd-peer.py source file PATH     the window drags the file at PATH
    dnd-peer.py target               the window takes a drop, and says it

Everything said goes to standard output a line at a time: "ready" once the
window is up; a source says "action copy" or "action move" or
"action none" when the drag ends, and "deleted" if the target moved it; a
target says "type NAME", then "text ..." or "uri ...". Placed at the
screen's right, at x=500, where a window manager does not decide otherwise.
It never outlives a minute.
"""

import os
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, Gio, GLib, Gtk  # noqa: E402


def say(*words):
    print(*words, flush=True)


def source(area, kind, payload):
    actions = Gdk.DragAction.COPY | Gdk.DragAction.MOVE
    area.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], actions)
    if kind == "text":
        area.drag_source_add_text_targets()
    else:
        area.drag_source_add_uri_targets()

    def give(_widget, _context, data, _info, _time):
        if kind == "text":
            data.set_text(payload, -1)
        else:
            data.set_uris([Gio.File.new_for_path(payload).get_uri()])

    def deleted(_widget, _context):
        say("deleted")

    def ended(_widget, context):
        action = context.get_selected_action()
        if action & Gdk.DragAction.MOVE:
            say("action move")
        elif action & Gdk.DragAction.COPY:
            say("action copy")
        else:
            say("action none")
        GLib.timeout_add(300, Gtk.main_quit)

    area.connect("drag-data-get", give)
    area.connect("drag-data-delete", deleted)
    area.connect("drag-end", ended)


def target(area):
    targets = Gtk.TargetList.new([])
    targets.add_uri_targets(1)
    targets.add_text_targets(0)
    area.drag_dest_set(
        Gtk.DestDefaults.ALL, [], Gdk.DragAction.COPY | Gdk.DragAction.MOVE
    )
    area.drag_dest_set_target_list(targets)

    def taken(_widget, _context, _x, _y, data, info, _time):
        say("type", data.get_target().name())
        if info == 1:
            for uri in data.get_uris() or []:
                say("uri", uri)
        else:
            say("text", data.get_text() or "")
        GLib.timeout_add(500, Gtk.main_quit)

    area.connect("drag-data-received", taken)


def main():
    mode = sys.argv[1]
    window = Gtk.Window(title="dnd peer " + mode)
    window.set_default_size(300, 300)
    window.move(int(os.environ.get("DND_PEER_X", "500")), 0)
    area = Gtk.EventBox()
    area.add(Gtk.Label(label=mode))
    window.add(area)
    if mode == "source":
        source(area, sys.argv[2], sys.argv[3])
    else:
        target(area)
    window.connect("destroy", Gtk.main_quit)
    window.show_all()

    def up():
        say("ready")
        return False

    GLib.timeout_add(300, up)
    GLib.timeout_add_seconds(60, Gtk.main_quit)
    Gtk.main()


if __name__ == "__main__":
    main()
