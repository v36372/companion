# Named PDF destinations

The app's snapshot outline supplements gpui-pdf 0.6.1's explicit page targets
with catalog `/Names` → `/Dests` name-tree and legacy catalog `/Dests` dictionary
lookups. The name tree may use `/Kids`; leaves use alternating `/Names` strings
and destination values. Arrays and dictionaries wrapping an array in `/D` are
supported, including indirect objects. Page references map to the actual parsed
page sequence, with zero-based indices retained until display/tool boundaries.
Keys compare as PDF bytes (not decoded/trimmed titles). Traversal has a 32-level
depth limit, a 100,000-work-unit budget, an 8 MiB stored-key budget and visited
object sets. The upstream outline's existing 10,000-item/32-level limits remain.

Resolved entries navigate through the app's tracked explicit navigation path.
Unresolved entries are dimmed, marked `(unavailable)`, and have no pointer cursor,
hover treatment or click handler. The agent-facing outline Markdown uses the same
resolved page numbers as the rail.

## In-page links: upstream follow-up

Named in-page links have the same issue. In gpui-pdf 0.6.1 `src/outline.rs`,
`link_target` only reads `/Dest` as an array or `/A /S /GoTo /D` as an array.
Named strings and names are dropped rather than emitted as `PdfLink`s.

This cannot cheaply be repaired by passing our map to the current viewer API:
`src/lib.rs`'s private `prepare_pdf` eagerly calls `page_links`, stores results in
private `PreparedPdf.links`, and installs them into private `PdfView.links`.
`PdfView` has no link setter, document accessor, internal-link callback or
annotation-overlay injection API. `set_on_open_external` handles only external
URIs and cannot restore dropped internal annotations. No dependency is vendored
or patched here. A follow-up should share named-destination resolution between
upstream `outline` and `page_links` (or expose a link setter/prepare hook so our
resolver can supply the corrected links), retaining URI allowlisting and
rotation/rectangle handling. Until then, named in-page links remain unavailable.

## Reproduce outline coverage

Download PDFs outside the repository, then run:

```sh
lc-cargo run -p companion-pdf --example outline_counts -- /path/to/thinkpython2.pdf /path/to/attention.pdf
```

Measured with the task's public downloads:

| PDF | Upstream resolved/total | Supplemented resolved/total |
| --- | --- | --- |
| Think Python, 2nd edition | 0/240 | 240/240 |
| Attention Is All You Need (arXiv 1706.03762) | 0/22 | 22/22 |

The example prints upstream resolved/total counts followed by supplemented
resolved/total counts without performing text extraction or opening a GUI.
The generated fixtures in `crates/companion-pdf/tests/destinations.rs` test both
catalog forms, child name-tree nodes, indirect arrays/dictionaries, non-UTF-8
byte keys, direct destinations, GoTo actions, unresolved/remote targets, cycles,
name-tree precedence, and depth/work limits. No downloaded books are committed.
