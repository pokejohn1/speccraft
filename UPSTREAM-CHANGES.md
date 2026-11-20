# Speccraft: changes to the original WordCraft files

Speccraft is a fork of WordCraft (https://github.com/storytold/wordcraft), which is still in alpha.
This list is the first thing to check when pulling in a newer version of WordCraft: it names every
**original** file that Speccraft has edited, so a merge conflict can only happen in these files.
Everything not listed here is either untouched or a new file that only Speccraft has.

Keep it current: whenever an original file is edited, add a row in the same change.

## New files (Speccraft only, no conflicts possible)

| File | What it is |
|---|---|
| `crates/engine/src/cmd/spec.rs` | Spec commands `spec.tag`, `spec.list`, `spec.set`, and their tests (M1) |
| `crates/engine/src/cmd/spec/sections.rs` | Section paragraphs (M2): `spec.import`, `spec.activate`, `spec.deactivate`, `spec.printable`; M5: `spec.toggle` (Mod+Alt+J; refused state kept in `Speccraft.Refused`) and `spec.outline`; kinds and levels from styles, the on/off state (custom property `Speccraft.On`), the parent rule, and their tests. A child module of `spec.rs`, so it needs no registry line of its own |
| `crates/engine/src/cmd/spec/output.rs` | PDF output (M3): `spec.exportPdf` writes a PDF of exactly the paragraphs that print, from a cleaned copy of the document through WordCraft's own PDF export and safe file write; and its tests. Also a child module of `spec.rs` |
| `crates/engine/src/cmd/spec/fields.rs` | Fill-ins and choices (M7): `spec.fields.detect`, `spec.fields.list`, `spec.fields.set`; fields are inline content controls tagged `field:<id>` (combo box, text or date) with the master's wording kept in the document property `Speccraft.Fields`; publish-time resolution used by `printable_copy`; and its tests |
| `crates/engine/src/cmd/spec/project.rs` | Project file (M4): `spec.project.new/info/set/addSection/removeSection/include/order/requires/validate/publish`; the `.speccraft` JSON format; copying a master into the project (records `Speccraft.Role`, `Speccraft.Master` and `Speccraft.Baseline` in the copy); placeholder filling used by `spec.exportPdf`; and its tests. A child module of `spec.rs` |
| `crates/ui-egui/src/speccraft/marks.rs`, `crates/ui-egui/src/speccraft/outline.rs` | M5: spec marks in the page margin (state colours, a wash over what won't print, click to switch) and the Spec Outline pane (tree with filter and search, details of the paragraph at the caret, Printed view of the pages as they publish) |
| `crates/ui-egui/src/speccraft.rs` | The Specs ribbon tab and the Sections pane (M4; project information editor added in M5): `ui.spec.*` commands (open/new/close project, add section from a master, open a section in the same window after saving the one on screen, include, move, remove, check, publish), and their tests |
| `speccraft/tests/` | Test master sections (original wording), the M3 PDF regression check `check_pdfs.py` and its expected text, and a tool to rebuild a guide spec PDF as a .docx for local tests. Python; see its README |
| `UPSTREAM-CHANGES.md` | This list |

## Original files edited

| File | Change | Why |
|---|---|---|
| `crates/engine/src/cmd.rs` (was `cmd/mod.rs` before WordCraft 0.18) | Added `pub mod spec;` and `v.extend(spec::specs());` (2 lines) | Registers the spec commands |
| `crates/engine/src/ribbon.rs` | `TABS` gained `"Specs"` after `"Review"` (13 → 14) | The Specs ribbon tab. After Review, not after Home, so upstream tests that expect Insert after Home keep passing |
| `crates/ui-egui/src/ribbon/tabs.rs` | `TABS` length 13 → 14 | Follows the engine's list |
| `crates/ui-egui/src/ribbon.rs` | One match arm: `"Specs" => crate::speccraft::ribbon(app, ui)` | Draws the Specs tab |
| `crates/ui-egui/src/lib.rs` | `pub mod speccraft;` and the `WordApp` field `speccraft: speccraft::SpecUi` (4 lines) | State of the Specs tab and Sections pane |
| `crates/ui-egui/src/app.rs` | `speccraft: Default::default(),` in `WordApp::new` | Initialises that field |
| `crates/ui-egui/src/window_ui.rs` | `speccraft: _,` under "App-wide" in `swap_window` | WordCraft makes every `WordApp` field say whether it is app-wide or per window; the open project is app-wide |
| `crates/ui-egui/src/app/commands.rs` | 4 lines at the top of `ui_command`: `ui.spec.*` goes to `crate::speccraft::command` | Registers the Specs UI commands |
| `crates/ui-egui/src/panes.rs` | `Pane::Sections` and `Pane::SpecOutline` and their arms in each match (title; default place: Sections left 280 pt, Spec Outline right 320 pt; keys `sections`, `specOutline`; `ALL` 11 → 13; no close command; body) | The Sections and Spec Outline panes |
| `crates/ui-egui/src/panes/state.rs` | `(Pane::Sections, app.speccraft.pane_open)` and `(Pane::SpecOutline, app.speccraft.outline_open)` in `open` | When the panes show |
| `crates/ui-egui/src/panes/host.rs` | Two arms in `close` (`Pane::Sections`, `Pane::SpecOutline`) | Closing the panes from their title bars |
| `crates/ui-egui/src/canvas.rs` | 2 lines after the reading highlight in `show`: `crate::speccraft::marks::paint(…)` | Draws the spec marks over the pages |
| `crates/ui-egui/src/file_dialogs.rs` | One arm in `open_filters`: `"speccraft"` (Speccraft projects, All files) | Open Project's file filter |
| `crates/engine/src/io.rs` | Added `create_dir_all` (checked against the automation write root, like `write_file`), with a web stub (13 lines, after `list_dir`) | `spec.project.publish` creates its Published folder; commands may only reach the disk through `io` (`file_access::tests::commands_do_not_touch_the_file_system_themselves`) |
| `crates/engine/src/cmd/controls.rs` | `fn insert` became `pub(crate) fn insert` (1 word) | Lets the spec commands reuse the content control insert code |
| `.gitignore` | Added a block at the end ignoring `*.docx`, `*.docm`, `*.dotx`, `*.dotm`, `*.doc`, `*.pdf`, `*.rtf`, `*.odt` | The repo is public: author masters, project sections and exported PDFs must never be committed. Upstream tracks no files of these types. To add a test file of one of these types on purpose, use `git add -f`. Also ignores Python caches under `speccraft/` |

## Merge history

| Date | WordCraft version | Result |
|---|---|---|
| 2026-10-21 | 0.9.0 (`e05b7fe`) | Forked |
| 2026-11-19 | v0.18.1 | Merged with no conflicts (git followed `cmd/mod.rs` → `cmd.rs`). All 22 spec tests pass; engine tests 1379 of 1380 pass, the one failure is upstream's (below) |
| 2026-11-20 | v0.18.1 (checked again) | Full test run on Windows: 21 spec tests and `hostile_params_never_panic` pass; 5,754 tests pass, 12 fail, all upstream or environment (see Known issues). The gypsum board check gives the same results as before the upgrade (360 items, PDF unchanged by import, state survives save and reopen, `spec.exportPdf` gives a 1-page PDF) |

## Merging a newer WordCraft

Merge a release tag (for example `v0.19.0`), not `main`.

1. Fetch the new version and merge it.
2. If git reports a conflict, it will be in one of the files above; keep both sides' changes.
3. Run `cargo xtask ci` (the project's own full check) and make sure the spec tests still pass.

## Known issues

`cargo xtask ci` stops at the first failing test, so on this machine it never reaches the later
steps. Run `cargo test --workspace --no-fail-fast` to see every failure, then `cargo xtask assets`,
`cargo xtask layers` and `cargo xtask wasm` separately. Only the failures below are expected.
Checked on WordCraft v0.18.1, Windows, 2026-11-20.

### Caused by Speccraft

- The Speccraft command labels and ribbon locations are English only ("Specs", "Items", "Tag as
  Spec Item", "List Spec Items", "Set Spec Item Text", "Import Section Paragraphs", "Switch
  Paragraph On", "Switch Paragraph Off", "List Printable Paragraphs", "Export Printable PDF"), so
  in a non-English interface these commands show in English. No test flags this any more:
  WordCraft 0.18 removed `every_tab_and_command_is_translated`. To fix later: add translations,
  ideally in one new Speccraft file with a small hook in `crates/ui-egui/src/i18n/mod.rs` (log that
  edit in the table above) rather than editing the 22 `.tsv` files.

### Upstream, not caused by Speccraft

These 11 also fail on plain WordCraft v0.18.1 (tag `v0.18.1`, no Speccraft code), so they come
from WordCraft or from what this computer has installed.

Language data not installed (hyphenation and spelling dictionaries):

- `swiss_german_and_portugal_portuguese_keep_their_languages_rules` (`crates/engine/src/cmd/packs.rs`)
- `review_language_validates_tags_and_marks_text_unchecked` (`crates/engine/src/tests.rs`)
- `auto_hyphenation_breaks_long_words`, `fragmented_hidden_text_lays_out_like_the_visible_text`
  and `hyphenation_and_proofing_follow_each_runs_language` (`crates/layout/src/tests.rs`)

Fonts and symbols (user interface, `crates/ui-egui`):

- `interface_symbols_have_glyphs` (`theme.rs`), `rtl_template_titles_are_right_aligned`
  (`backstage`), and four tests that stop on a missing icon glyph (U+F03A):
  `a_dialog_closing_into_the_browser_leaves_it_the_keyboard` (`file_browser`),
  `the_title_bar_moves_the_window_in_every_modal_state` (`dialogs`),
  `return_on_the_print_page_prints` (`tests`) and `the_player_and_the_file_browser_close_from_the_box`
  (`window_close`)

Timing, fails only when the whole suite runs at once:

- `a_silent_connection_is_closed` (`apps/wordcraft/src/control_server.rs`): passes when run on its
  own (`cargo test -p wordcraft --bin wordcraft a_silent_connection_is_closed`).

No longer failing since v0.18.1: the layout test `inline_box_that_resizes_to_fit_takes_its_texts_height`
and the three colour-emoji tests in `crates/render`.
