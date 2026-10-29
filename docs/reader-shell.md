# Reader shell

The app action bar contains **Open PDF…** and the current document title.
The embedded gpui-pdf header is the **only** page/zoom/search control set.
Page Up/Page Down and Cmd/Ctrl-F still work when the composer has focus.

The three panes use gpui-component's horizontal resizable group. Initial widths
are 240 px (sidebar) and 380 px (chat), bounded to 200–360 and 320–560 px;
the viewer receives the remaining width (minimum 280 px). Drag either divider.
The clipped viewer wrapper remains in place so zoomed pages cannot paint over
adjacent panes. Resizing does not rebuild PdfView or change raster quality/zoom;
only an explicitly selected upstream fit mode refits on viewport changes.

Scanned-page and selection actions float inside the clipped viewer instead of
changing its height. Selection actions can wrap in a narrow pane. Errors use
Root notifications, with a close button. Password entry remains an in-memory
masked overlay, now using the shared theme. The empty viewer offers a primary
Open PDF action. Import/preflight work runs off-thread and keeps the old reader
usable until the latest requested file is ready.

## Titles and display paths

A sensible PDF `/Title` takes precedence over the original source filename stem,
then the catalog title, then `Untitled PDF`. Empty, generic placeholder, control-
character and excessively long metadata titles are rejected. UTF-16BE and UTF-8
metadata are supported, with a Latin-1 fallback for older PDFs.

Gpui-pdf 0.6.1 has no display-name setter or toolbar visibility switch: its label
is the opened path's filename. Before constructing the viewer, a background
preflight reads metadata and creates `<data-dir>/display/<document-id>/<title>.pdf`.
The filename replaces separators, controls and cross-platform reserved punctuation,
removes leading/trailing spaces/dots, and caps its UTF-8 length. This is normally
a hard link to the managed copy; filesystems without hard-link support use a
copy. SQLite identity, hashing, extraction and writes still use the original
managed UUID path. Display links may be safely removed while the app is closed.
This adds one lightweight metadata parse before the existing viewer/snapshot
parses, not synchronous UI-thread I/O. The window title uses the full document
title: `<title> — Learning Companion`.

Encrypted metadata may be unavailable before the password is entered. In that
case the viewer header uses the original source stem; after unlock, snapshot
metadata updates the window/catalog title. The immutable upstream viewer path
cannot be renamed in-place without replacing the unlocked viewer.

## Reading position

The vendored viewer samples its visible page and in-page scroll fraction every
150 ms (fractions are rounded down to 5% steps), emitting only changes outside
layout/render. Wheel scrolling, scrollbar drags, viewer arrows, editable page
counter, outline and search jumps all update the app counter, current sidebar
section and the shared viewport used by Follow and “What did I just read?”.
Scrolling never submits an agent question automatically.

Progress saves after a one-second idle debounce and flushes on normal quit.
Reopening restores the saved page (not the exact in-page fraction); restoring or
attaching text extraction does not itself schedule a progress write.

## Viewer layout limitations

The upstream header is not responsive at very narrow viewer widths. Its own
search bar also controls its internal page-area height. These are upstream
layout behaviors in the vendored viewer.
