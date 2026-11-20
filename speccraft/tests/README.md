# Speccraft test sections and PDF regression check

| File | What it does |
|---|---|
| `make_masters.py` | Writes two test master sections in the SCT/PRT/ART/PR1–PR5/CMT/EOS format: 08 7100 Door Hardware (a table, `//` and CMT notes, bracketed choices) and 07 9200 Joint Sealants (fill-in blanks, PR1–PR5, a hidden paragraph, an unstyled paragraph, a landscape page whose section break sits on a note). The wording is original, written for these tests, so the files may be shared. |
| `run_selection.py` | Imports a master with `spec.import`, switches everything on, switches the given ids off with `spec.deactivate`, and exports with `spec.exportPdf`. |
| `check_pdfs.py` | The regression check: runs the cases below and compares each PDF's text with `expected/`. Line breaks, headers and footers are ignored, so fonts on another computer don't matter; numbering, order and what prints do. |
| `items.py` | Lists the spec items (id, style, text) of an imported section. |
| `pdf_to_master.py` | Rebuilds a MasterSpec-style guide specification PDF (after `pdftotext -layout`) as a styled .docx master for local testing. The result contains the publisher's text: keep it local, never commit it. |

## Running the check

```sh
cargo build --release -p wordcraft-cli
pip install python-docx          # once; pdftotext comes with poppler
python speccraft/tests/check_pdfs.py
python speccraft/tests/check_pdfs.py --update   # after an intended change, then review the diff in expected/
```

A failing case leaves its PDF beside this file as `<case>.failed.pdf` (ignored by git).

## Cases

| Case | Switched off | Checks |
|---|---|---|
| `door-hardware` | an item in a list, a related section, a PR3, the WARRANTY, EXIT DEVICES and HARDWARE SCHEDULE articles | numbering closes up at every level; notes never print; the finishes table prints under its paragraph |
| `joint-sealants` | a PR2, WARRANTY, a PR4 (its PR5 goes too), URETHANE article (with the hidden paragraph), the paragraph above the schedule table | the table leaves with its paragraph; the landscape page survives although its section break was on a note |
| `joint-sealants-stray` | the stray unstyled paragraph | the Articles after it still print |
| `joint-sealants-fields` | WARRANTY, a PR4, URETHANE article; then fill-ins found, the temperature blank set to 40, the colour choice set to "Match adjacent surface" | chosen and typed values print; open fill-ins print as written |
