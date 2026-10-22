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
| `crates/engine/src/cmd/mod.rs` | Added `pub mod spec;` and `v.extend(spec::specs());` (2 lines) | Registers the spec commands |
| `crates/engine/src/cmd/controls.rs` | `fn insert` became `pub(crate) fn insert` (1 word) | Lets the spec commands reuse the content control insert code |

## Merging a newer WordCraft

1. Fetch the new version and merge it.
2. If git reports a conflict, it will be in one of the two files above; keep both sides' changes.
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

### Upstream, not caused by Speccraft (font-dependent, fail on Windows)

- `inline_box_that_resizes_to_fit_takes_its_texts_height` (`crates/layout/src/tests_textbox.rs`):
  it counts pages, and line heights depend on the installed fonts.
- `emoji_are_drawn_in_colour`, `emoji_keep_their_colours_on_a_dark_page` and
  `emoji_with_text_effects_stay_drawn` (`crates/render/src/tests_color_glyphs.rs`): they expect a
  particular colour emoji font.

Neither the `layout` nor the `render` crate uses any Speccraft code.
