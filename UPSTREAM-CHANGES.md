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
| `crates/engine/src/cmd/spec/rules.rs` | Coordination rules (M6): `spec.rules.list/add/remove/check`, `spec.why`; require / recommend / warn / exclude on paragraph and fill-in conditions; rules kept in `Speccraft.Rules`, author overrides in `Speccraft.Overridden`; and its tests. `sections::outline` applies the rules (an entry's `manual` and `linked` flags) |
| `crates/engine/src/cmd/spec/library.rs` | Master library (M9): `spec.library.scan/info/search/preview/tag`; an index file `library.speccraft-library` in the library folder (number, title, division, version, paragraphs, and the author's tags, note and approval, kept across rescans; only changed masters are read again); MasterFormat division names; masters are never written. `spec.project.set` takes `library`, and `spec.project.addSection` refuses a section number the project already has |
| `crates/engine/src/cmd/spec/quality.rs` | Quality checks (M10): `spec.check` on the section on screen (open fill-ins, the section's rule findings, nothing chosen, empty headings, a missing or switched-off Part, drafting notes, editing marks and bracketed text left in printing paragraphs, the same paragraph twice under one heading, END OF SECTION missing or naming another number), each finding naming its paragraph. `spec.project.validate` runs them for every section, links references and placeholders to their paragraph, gives every finding a `key`, and leaves out warnings and notices accepted with `spec.project.acknowledge` (kept in the project file's `acknowledged`); `spec.project.publish` stops at errors unless `force` |
| `crates/engine/src/cmd/spec/update.rs` | Master updates (M11): `spec.update.compare/apply/history`. Lines the copy's recorded master wording (`Speccraft.Original`, in the order kept in the new `Speccraft.Order`) up with the master as it is now: changed, added and removed paragraphs, each with whether the author edited it too (conflict), prints, is named by a rule, or has fill-ins that taking it would reset. Applying takes `master` or `mine` per change (defaults: the master's, except conflicts and removing a printing paragraph); new paragraphs come in switched off; the copy's record becomes the new master; each update is added to `Speccraft.History`. Older copies without recorded wording compare through their fingerprints. `edits.rs`: `set_paragraph_text`, `insert_item` and `baseline` opened to the crate; restoring a deleted master paragraph uses the master's order; reading the wording back from the master stops once the master has changed. `project.rs`: copies record `Speccraft.Order`; Check Project reads the copy's own master record |
| `crates/engine/src/cmd/spec/manual.rs` | Project manual (M14): `spec.project.manual` puts every included section's issued copy one after another (each with its own page setup, headers, footers and styles, so its numbering starts again; page numbers from 1 in each section, or running on), after a table of contents (project, then sections by division with page counts); Check Project first; PDF and/or Word into Published. `output.rs` (M13): `spec.exportDocx` writes the printed copy as a plain Word document (Speccraft's content controls taken off with their text kept, `Speccraft.*` properties dropped); the issued copy is shared with `spec.exportPdf` (`issued_copy`, `strip_markings`, `page_count`); `spec.project.publish` takes `format` (`pdf`, `docx`, `both`) |
| `crates/engine/src/cmd/spec/overview.rs` | Project overview (M12): `spec.project.overview` runs Check Project once and reads each section: what prints, fill-ins open, findings not accepted, changes from the master (modified, added, deleted), a newer master, the last master update, and publishing (published, changed since, not published, left out); which sections need attention; the project's own findings |
| `crates/engine/src/cmd/spec/forms.rs` | Full, short and outline forms (M16): `spec.short.set/list`; per paragraph "left out of the short form" (with what is under it) and short wording (`Speccraft.Short`, with the full text's fingerprint so Check Section flags short wording whose full text changed, code `shortStale`); the outline is the short form to a depth of paragraph levels under each Article. `output.rs`: `printable_copy_as` / `issued_copy_as`; `spec.exportPdf`, `spec.exportDocx`, `spec.project.publish` and `spec.project.manual` take `form` and `depth` (files get " (Short Form)" / " (Outline)"). `fields.rs` (W1): `spec.fields.need` (`Speccraft.FieldNeeds`), required open fill-ins are errors, optional ones aren't reported |
| `crates/engine/src/cmd/spec/edits.rs` | Project editing layer (M8): `spec.origin`, `spec.restore`, `spec.duplicate`, `spec.addParagraph`, `spec.removeParagraph`; the master's wording kept in `Speccraft.Original` (recorded by the first `spec.import`, or read back from the master for older copies); and its tests |
| `crates/engine/src/cmd/spec/fields.rs` | Fill-ins and choices (M7): `spec.fields.detect`, `spec.fields.list`, `spec.fields.set`; fields are inline content controls tagged `field:<id>` (combo box, text or date) with the master's wording kept in the document property `Speccraft.Fields`; publish-time resolution used by `printable_copy`; and its tests |
| `crates/engine/src/cmd/spec/project.rs` | Project file (M4): `spec.project.new/info/set/addSection/removeSection/include/order/requires/validate/publish`; the `.speccraft` JSON format; copying a master into the project (records `Speccraft.Role`, `Speccraft.Master` and `Speccraft.Baseline` in the copy); placeholder filling used by `spec.exportPdf`; and its tests. A child module of `spec.rs` |
| `crates/ui-egui/src/speccraft/marks.rs`, `crates/ui-egui/src/speccraft/outline.rs`, `crates/ui-egui/src/speccraft/rules_view.rs` | M5: spec marks in the page margin (state colours, a wash over what won't print, click to switch) and the Spec Outline pane (tree with filter and search, details of the paragraph at the caret, Printed view of the pages as they publish) |
| `crates/ui-egui/src/speccraft/library.rs` | M9: the Master Library pane (folder, search, division and approved filters, results by division with "in project" marks, preview outline, tags, note, approval, Add to Project) and its `ui.spec.library*` commands; the Specs tab's Master Library button |
| `crates/ui-egui/src/speccraft/checks.rs` | M10: the Spec Outline pane's Checks view and the Sections pane's project report (by section; click to open the section at the paragraph; Accept / Undo; Show accepted; Publish Anyway after Publish stops at errors); `ui.spec.goTo`, `ui.spec.acknowledge`; the Specs tab's Check Section button |
| `crates/ui-egui/src/speccraft.rs` (M13–M14) | Publish format choice (PDF, Word, both) and Manual in the Sections pane; Project Manual and Export Word on the Specs tab; `ui.spec.manual`, `ui.spec.publishFormat`; Publish Anyway goes ahead with whichever was stopped |
| `crates/ui-egui/src/speccraft/updates.rs` | M11: the Spec Outline pane's Updates view (changes word by word, what each means for the project, Take the master's / Keep mine, Apply Update, earlier updates) and `ui.spec.update*`; the Specs tab's Master Updates button; a "newer master" finding opens the view |
| `crates/ui-egui/src/speccraft/tabs.rs` | M15: section tabs above the page (one per open document while a project is open; section number and title, • unsaved, ↑ newer master; close saves first, the last tab stays); at most 8 (the least recently used is saved and closed); opening a section opens or shows its tab; Check, Publish and the manual save open sections first; Go to Section follows a "Section 07 9200" reference at the caret; `ui.spec.tab`, `ui.spec.closeTab`, `ui.spec.followReference` |
| `crates/ui-egui/src/speccraft/overview.rs` | M12: the Project Overview pane (one row per section, filter: needs attention, newer master, not published, fill-ins open, changed from master; each part links to the section or its Fill-ins, Checks, Updates or outline view); `ui.spec.overview`; the Specs tab's Project Overview button. `panes.rs`, `panes/state.rs`, `panes/host.rs`: `Pane::SpecOverview` like the other Speccraft panes (`ALL` 14 → 15) |
| `crates/engine/src/cmd/spec/paragraphs.rs` | M18: Enter in a section paragraph adds a managed paragraph (`u0001`…; at the start of a paragraph before it, else after it or as its first child), `spec.autoSelectNew` (`Speccraft.AutoSelectNew`), `spec.indent` (a paragraph or branch a level down or up by its style), `spec.fixHierarchy`, `spec.noNumber` (`Speccraft.NoNumber`; numbering 0 with the level's indent). `sections.rs`: author notes between asterisks; W6 (choices under a paragraph switched off are remembered in `Speccraft.Refused` and come back with it); M17 `spec.exclude` and `seq` in `spec.outline`. `edits.rs`: `spec.removeParagraph` takes `ids` or the selection's added paragraphs; empty added paragraphs stay block items |
| `crates/engine/src/cmd/spec/units.rs` | M19: `spec.units` (`Speccraft.Units`: both, US only, metric only) applied to the printed copy; `is_measure`. `fields.rs`: multiple-choice fields (`spec.fields.multiple`, `Speccraft.FieldMulti`; printed as "a, b, and c"), `spec.fields.set` `options` and `fill` (a blank inside an option), and the `multiple`, `chosen`, `measure` fields |
| `crates/ui-egui/src/speccraft/marks.rs`, `choices.rs` (M17–M19) | The page's sequence-number column (state colours, rule bar, M* / U, a dot for choices under an off parent, a red cross for left out, dotted for notes) and its right-click menu; fill-ins coloured by kind (choices blue, measurements green, blanks red); clicking a choice opens its list (radio buttons, check boxes for a multiple choice, a box for an option's blank), double-clicking puts the caret in it. `outline.rs`: sequence numbers in the outline; the Fill-ins view uses the same list. `speccraft.rs`: `ui.spec.why`, `ui.spec.deleteAdded`; Specs › Edits: Delete Added, Indent, Outdent, No Number, Fix Hierarchy, Units, Auto-Select New |
| `crates/engine/src/cmd/spec/terms.rs` | M20: global terms (`spec.terms.apply/list`: `<Insert Name>` and fill-ins made from one become inline controls tagged `term:` holding the project value; back to the placeholder without one), section references (`spec.refs.apply/list`: `sref:` controls whose quoted title follows the project's sections), reference standards (`spec.standards.list`, `find_standards`); the master wording of each in `Speccraft.Inline`, so values don't make a paragraph M*. `edits.rs`: the comparison with the master puts terms and references back (`terms::originals_list`). `output.rs`: `strip_markings` takes these controls off too |
| `crates/engine/src/cmd/spec/print.rs` | M20–M22: `spec.project.sync` (terms, references, units, Auto-Select New and the project header/footer into the section; `Speccraft.Furniture` fingerprint), `spec.project.settings` (print options, header, footer, editing settings, a section keeping its own header/footer), `spec.project.standards` and `spec.project.setStandard`; header/footer lines with `{Section Name}`, `{Section ID}`, `{Page}`, `{Pages}` (SECTIONPAGES) and project values; print options applied to issued copies (drafts with notes and a watermark, keep with next, widow control, PDF bookmarks by outline level, all choice options with the chosen underlined, END OF SECTION text, hyperlinks), published file names. `project.rs`: the project file gains `standards`, `editing`, `header`, `footer`, `print`; publish takes `division` and `print`. `output.rs`: `issued_copy_with` syncs the copy with its project and applies the print options; `printable_copy_with` keeps notes for drafts; exports take `print`. `manual.rs`: `Layout` (odd pages with marked blank pages, with or without header/footer), `division`, `contentsOnly`, defaults from the print options |
| `crates/ui-egui/src/speccraft/project_settings.rs` | M20–M22: Sections pane › Header and Footer, Print Options, Reference Standards; the section on screen is synced with the project when opened, when its tab is shown and when project values or settings change. `marks.rs`: terms purple, references green (Ctrl+click opens the section), standards with a dotted green line. Specs tab: Print Options; Units and Auto-Select New change the project's setting when a project is open |
| `crates/engine/src/cmd/spec/attachments.rs` | M23: `spec.project.attach` (copies a PDF into the project's `Attachments` folder as `087100-ATT1.pdf`, checks it reads, records it in the project file's `attachments`), `spec.project.detach` (the file stays), `spec.project.attachments`; `pdf_with` writes a PDF with other PDFs' pages put in. `output.rs`: issued copies carry their section's attachments; `spec.exportPdf` ends with them. `manual.rs`: attachments after their section or at the end (print option `attachments`: after, end, none), listed in the contents; `build` returns where each section starts. UI: section menu › Attach PDF…, "+n PDF" on the section row, Sections pane › Attached PDFs (remove), Print Options › Attached PDFs; `file_dialogs.rs` gains a `pdf` filter |
| `crates/engine/src/cmd/spec/review.rs` | M24: review rounds: `spec.review.track` (WordCraft's Track Changes plus a baseline of the printing paragraphs in `Speccraft.Review`), `spec.review.newRound` (new baseline, optionally accepting the wording changes), `spec.review.changes`; print option `markup` (none, all, tracking): PDFs with tracked changes shown and a change bar by paragraphs switched on this round. UI: blue bars by sequence numbers switched this round; Specs › Track Changes, New Review Round |
| `crates/engine/src/cmd/spec/comments.rs` | M25: `spec.comments.set` (priority and status kept in `Speccraft.CommentMeta` by the comment's durable id, which it assigns when missing; resolved/closed mark the comment done), `spec.comments.list` (filters, counts, the paragraph each is on, replies), `spec.project.comments` (the project's comments report as Word, PDF or CSV). UI: Spec Outline › Comments (`comments_view.rs`): counts by priority as filters, priority and status menus, go to paragraph, report buttons; Specs › Review Comments |
| `crates/engine/src/cmd/spec/notes.rs` | M26: `spec.paragraph.info` (the master's notes before a paragraph, the author's project note, tags, why), `spec.paragraph.note` (`Speccraft.ProjectNotes`; drafts print them after their paragraph), `spec.paragraph.tags` and `spec.tags.list` (`Speccraft.Tags`). `attachments.rs`: `spec.project.attachmentTitle`. UI (`manager.rs`): Spec Outline views Paragraph, Terms (the section's global terms; a value changed there is the project's) and Standards, and the review round's changes at the top of Review; Sections pane: publish scope (all, a division, contents only), Options (open sections in tabs or in place; margin columns: M*/U, rule bars, review bars, sequence numbers), section menu › Keep Its Own Header and Footer, attachment titles and Open |
| Leftovers after M26 | `manual.rs` `number_past_attachments`: with running page numbers, sections after attached PDFs number past their pages (PDF manual only). `marks.rs`: section references not wrapped (in a choice's options) are found in the text, underlined green and Ctrl+click opens them |
| `crates/ui-egui/src/speccraft/project_rules.rs` | W3: Sections pane › Rules Between Sections (the project's rules in words, their Check Project findings, remove; an add form); `ui.spec.addProjectRule`, `ui.spec.removeProjectRule` |
| `crates/ui-egui/src/speccraft/outline.rs`, `marks.rs` (M16, W1, W2) | Printed view and Publish form choice (full, short, outline with levels); each paragraph's details: In the short form, short wording, a note when it is out of date; Fill-ins view Required / Optional / Unmarked; page marks in one column |
| `crates/ui-egui/src/documents.rs` | `sync_document`'s switching split out as `show_here(id)`: shows another open document in the drawn window even when windows are native (section tabs switch this way and never open a window) |
| `crates/ui-egui/src/windows.rs` | `prune_windows` gives no window to documents waiting in Speccraft section tabs (`speccraft.tab_order`): with native windows it opened one for each open document without a window, so every switch between sections opened another window. `Windows::count` for tests |
| `crates/ui-egui/src/app/frame.rs` | One call, `crate::speccraft::tabs::strip(self, ui)`, after the panes and before the page |
| `crates/layout/src/lib.rs`, `ctx.rs`, `flow.rs` | W4: `LayoutOptions::unnumbered` (paragraphs by `unnumbered_key`, their address in the document laid out): such a paragraph's list number isn't counted and shows blank, its level's indents and tab kept. `None` by default, so WordCraft lays out as before |
| `crates/engine/src/session/layout.rs` | W4: the on-screen layout passes `cmd::spec::sections::unnumbered(&doc)` (switched-off spec paragraphs; `None` for documents without Speccraft data), so the editing view numbers as the printed copy |
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
| `crates/ui-egui/src/panes.rs` | `Pane::Sections`, `Pane::SpecOutline` and `Pane::SpecLibrary` and their arms in each match (title; default place: Sections left 280 pt, Spec Outline right 320 pt, Master Library left 330 pt; keys `sections`, `specOutline`, `specLibrary`; `ALL` 11 → 14; no close command; body) | The Sections, Spec Outline and Master Library panes |
| `crates/ui-egui/src/panes/state.rs` | `(Pane::Sections, app.speccraft.pane_open)`, `(Pane::SpecOutline, app.speccraft.outline_open)` and `(Pane::SpecLibrary, app.speccraft.library.open)` in `open` | When the panes show |
| `crates/ui-egui/src/panes/host.rs` | Three arms in `close` (`Pane::Sections`, `Pane::SpecOutline`, `Pane::SpecLibrary`) | Closing the panes from their title bars |
| `crates/ui-egui/src/canvas.rs` | 2 lines after the reading highlight in `show`: `crate::speccraft::marks::paint(…)` | Draws the spec marks over the pages |
| `crates/ui-egui/src/file_dialogs.rs` | One arm in `open_filters`: `"speccraft"` (Speccraft projects, All files) | Open Project's file filter |
| `crates/engine/src/io.rs` | Added `create_dir_all` (checked against the automation write root, like `write_file`), with a web stub (13 lines, after `list_dir`) | `spec.project.publish` creates its Published folder; commands may only reach the disk through `io` (`file_access::tests::commands_do_not_touch_the_file_system_themselves`) |
| `Cargo.toml` (workspace) | craft-pdf with the `write-pdf` feature as well as `write` | M23: krilla's embedding of existing PDF pages (adds `hayro-write`, Apache-2.0 OR MIT) |
| `crates/pdf/src/options.rs`, `export.rs`, `lib.rs` | `PdfOptions::inserts` (`PdfInsert { after, pdf }`): other PDFs' pages put in after a given page of a plain export, link and bookmark destinations moved along; `pdf_page_count` | M23: attachments in section PDFs and the project manual. Refused for imposition or several copies; unreadable PDFs are an error, not a crash. Test `other_pdfs_pages_are_put_in_after_the_given_page` |
| `crates/ui-egui/src/lib.rs` (`UiState`), `app/prefs.rs`, `window_ui.rs` | `UiState::speccraft` (a JSON value, default null), filled by `prefs()` and read back by `apply_prefs()`; listed as app-wide in `window_ui.rs` | Speccraft's Sections pane › Options kept between runs in `ui.json`. Test `the_options_survive_a_restart` |
| `crates/engine/src/cmd/text/enter_tab.rs` | 5 lines in `new_paragraph`: Enter first asks `crate::cmd::spec::paragraphs::enter` | M18: Enter in a section paragraph adds a managed paragraph instead of an unmanaged one. Documents without Speccraft data, selections and notes are left to WordCraft's own Enter, so `docs/typing-parity.md` is unchanged for ordinary documents |
| `crates/engine/src/cmd/controls.rs` | `fn insert` became `pub(crate) fn insert` (1 word) | Lets the spec commands reuse the content control insert code |
| `crates/ui-egui/src/windows.rs`, `chrome.rs`, `dialogs/chrome.rs`, `dialogs/dispatch.rs`, `backstage.rs` | Window title, title bar, "About Speccraft" and the Start screen use `speccraft::APP_NAME` and draw `speccraft::draw_logo`; About credits WordCraft | W7: the program shows as Speccraft. Test `tests.rs` title expectation now "Speccraft" |
| `apps/wordcraft/src/main.rs`, `apps/wordcraft/build.rs` | Window title "Speccraft"; app icon `assets/app-icon/speccraft-*.png`, exe icon `speccraft.ico`, ProductName "Speccraft". The app id and `run_native("WordCraft")` are unchanged so saved preferences carry over | W7. Icons made by `packaging/speccraft-icon.py` (rows in `ATTRIBUTION.md`) |
| `crates/ui-egui/src/i18n/audit.rs` | `"Speccraft"` added beside `"WordCraft"` to the names that need no translation | W7: the product name is not translated |
| `crates/ui-egui/src/file_dialogs.rs` | `AfterPick::ForDialogMany`: a multi-file pick handed to a dialog (Speccraft keeps the paths) | W8: Add Section picks several masters at once |
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
