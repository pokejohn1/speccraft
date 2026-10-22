# Speccraft: changes to the original WordCraft files

Speccraft is a fork of WordCraft (https://github.com/storytold/wordcraft), which is still in alpha.
This list is the first thing to check when pulling in a newer version of WordCraft: it names every
**original** file that Speccraft has edited, so a merge conflict can only happen in these files.
Everything not listed here is either untouched or a new file that only Speccraft has.

Keep it current: whenever an original file is edited, add a row in the same change.

## New files (Speccraft only, no conflicts possible)

| File | What it is |
|---|---|
| `crates/engine/src/cmd/spec.rs` | Spec commands `spec.tag`, `spec.list`, `spec.set`, and their tests |
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
