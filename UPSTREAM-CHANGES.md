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
| `crates/engine/src/cmd/spec/sections.rs` | Section paragraphs (M2): `spec.import`, `spec.activate`, `spec.deactivate`, `spec.printable`, kinds and levels from styles, the on/off state (custom property `Speccraft.On`), the parent rule, and their tests. A child module of `spec.rs`, so it needs no registry line of its own |
| `crates/engine/src/cmd/spec/output.rs` | PDF output (M3): `spec.exportPdf` writes a PDF of exactly the paragraphs that print, from a cleaned copy of the document through WordCraft's own PDF export and safe file write; and its tests. Also a child module of `spec.rs` |
| `UPSTREAM-CHANGES.md` | This list |

## Original files edited

| File | Change | Why |
|---|---|---|
| `crates/engine/src/cmd.rs` (was `cmd/mod.rs` before WordCraft 0.18) | Added `pub mod spec;` and `v.extend(spec::specs());` (2 lines) | Registers the spec commands |
| `crates/engine/src/cmd/controls.rs` | `fn insert` became `pub(crate) fn insert` (1 word) | Lets the spec commands reuse the content control insert code |
| `.gitignore` | Added a block at the end ignoring `*.docx`, `*.docm`, `*.dotx`, `*.dotm`, `*.doc`, `*.pdf`, `*.rtf`, `*.odt` | The repo is public: author masters, project sections and exported PDFs must never be committed. Upstream tracks no files of these types. To add a test file of one of these types on purpose, use `git add -f` |

## Merge history

| Date | WordCraft version | Result |
|---|---|---|
| 2026-10-21 | 0.9.0 (`e05b7fe`) | Forked |
| 2026-11-19 | v0.18.1 | Merged with no conflicts (git followed `cmd/mod.rs` → `cmd.rs`). All 22 spec tests pass; engine tests 1379 of 1380 pass, the one failure is upstream's (below) |

## Merging a newer WordCraft

Merge a release tag (for example `v0.19.0`), not `main`.

1. Fetch the new version and merge it.
2. If git reports a conflict, it will be in one of the files above; keep both sides' changes.
3. Run `cargo xtask ci` (the project's own full check) and make sure the spec tests still pass.

## Known issues

`cargo xtask ci` stops at the first failing test, so on this machine it never reaches the later
steps. Run `cargo test --workspace --no-fail-fast` to see every failure, then `cargo xtask assets`,
`cargo xtask layers` and `cargo xtask wasm` separately. Only the failures below are expected.

### Caused by Speccraft (left failing on purpose for now)

- `every_tab_and_command_is_translated` (`crates/ui-egui/src/i18n/tests.rs`) fails: WordCraft
  wants every command label and ribbon location translated into all 22 interface languages, and the
  Speccraft labels are English only ("Specs", "Items", "Tag as Spec Item", "List Spec
  Items", "Set Spec Item Text", "Import Section Paragraphs", "Switch Paragraph On", "Switch
  Paragraph Off", "List Printable Paragraphs", "Export Printable PDF"). Effect: in a non-English interface these commands
  show in English. To fix later: add translations, ideally in one new Speccraft file with a small
  hook in `crates/ui-egui/src/i18n/mod.rs` (log that edit in the table above) rather than editing
  the 22 `.tsv` files.

### Upstream, not caused by Speccraft

- `swiss_german_and_portugal_portuguese_keep_their_languages_rules` (`crates/engine/src/cmd/packs.rs`):
  fails on plain WordCraft v0.18.1 too ("German hyphenates Zusammenarbeit"), at least where no
  German hyphenation data is installed. Seen on 2026-11-19.

Font-dependent, fail on Windows:

- `inline_box_that_resizes_to_fit_takes_its_texts_height` (`crates/layout/src/tests_textbox.rs`):
  it counts pages, and line heights depend on the installed fonts.
- `emoji_are_drawn_in_colour`, `emoji_keep_their_colours_on_a_dark_page` and
  `emoji_with_text_effects_stay_drawn` (`crates/render/src/tests_color_glyphs.rs`): they expect a
  particular colour emoji font.

Neither the `layout` nor the `render` crate uses any Speccraft code.
