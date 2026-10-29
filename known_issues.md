# Known issues

Things noticed along the way that are not fixed yet. Remove an entry when it
is fixed; add the commit or pull request that fixed it if it helps.

## Native app (`lumi-app` / `lumi-ui`)

### Account page: multi-currency Change and Balance overflow their columns

On an account page the Change and Balance columns have fixed widths (150px
and 170px, `JournalDelegate::new` in `lumi-ui/src/journal_report.rs`), and
`amounts_text` joins all of an entry's currencies into one line, e.g.
`352.20 HKD, 78.02 USD`. With two or more currencies the text is wider than
the column: being right-aligned it runs out of the cell on the left, over the
Change column, and the Balance header can be cut off at the window's edge.

Seen with `Assets:Cash` of a ledger that holds that account in several
currencies.

Options:

- Render one currency per line in those cells (and size rows accordingly),
  so the column width only has to fit the widest single amount.
- Size the Change and Balance columns from the widest value on the page, or
  let the user resize them (`DataTable` supports column resizing).
- Both, since a long single amount can still overflow a fixed column.

### Balance sheet / income statement rely on a `Scrollable` workaround

`lumi-ui/src/trie_report.rs` sets `self_start()` on the report's scroll
element to work around a bug in GPUI Kit 0.8.1's two-way `Scrollable`, which
stretched the content to the area's height so nothing could scroll
vertically. The upstream fix is proposed in
`Lencerf/gpui-kit`, branch `fix/scrollable-both-axes`. Once `lumi-ui` is on
a GPUI Kit release that includes it, the `self_start()` call and its comment
can be removed.

### Entry editor hosts its own modal layer instead of `window.open_dialog`

In GPUI Kit 0.8.1, `gpui_base::Dialog` renders inside `deferred(...).with_priority(10 + layer)`,
whereas `gpui_component`'s `CompletionMenu` (and other code-editor popovers)
uses `deferred(...)` with default priority `0` instead of `POPUP_PRIORITY` (`100`).
As a result, any `Editor` placed inside `window.open_dialog` has its completion
dropdown painted underneath the opaque dialog surface. `lumi-ui/src/entry_editor.rs`
mounts `EntryEditorLayer` in `LumiApp`'s base pass so `Editor` popovers at
priority `0` paint above the modal. Once GPUI Kit gives `Editor` popovers
`POPUP_PRIORITY`, `EntryEditor` can return to `window.open_dialog`.
