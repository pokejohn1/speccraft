"""Speccraft PDF regression check (M3).

Builds the original test masters, switches on a fixed selection in each, exports a PDF with
spec.exportPdf, and compares the PDF's text with a known-good copy in expected/. Line breaks and
page headers and footers are ignored, so a different font on another computer does not count
as a difference; numbering, order and which paragraphs print do.

Needs: python-docx (pip install python-docx) and pdftotext (poppler).
Usage:
    python speccraft/tests/check_pdfs.py [path/to/wordcraft-cli] [--update]
The CLI defaults to target/release/wordcraft-cli (build it with cargo build --release -p wordcraft-cli).
"""
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import make_masters  # noqa: E402
import run_selection  # noqa: E402

# name, master file, ids switched off after switching everything on
CASES = [
    ("door-hardware", "087100 Door Hardware.docx",
     ["p0008", "p0013", "p0021", "p0033", "p0051", "p0053", "p0073"]),
    ("joint-sealants", "079200 Joint Sealants.docx", ["p0025", "p0028", "p0041", "p0044", "p0069"]),
    # A stray unstyled paragraph switched off must not take the rest of its Part along.
    ("joint-sealants-stray", "079200 Joint Sealants.docx", ["p0060"]),
    # Fill-ins (M7): a blank typed, a choice made, the rest left open and printed as written.
    ("joint-sealants-fields", "079200 Joint Sealants.docx", ["p0028", "p0041", "p0044"],
     ["spec.fields.detect", 'spec.fields.set={"id": "f0001", "value": "40"}', 'spec.fields.set={"id": "f0004", "option": 1}']),
]
FURNITURE = re.compile(r"^(<Insert Project Name>.*|Speccraft test master: .*)$")


def normalise(pdf):
    text = subprocess.run(["pdftotext", "-raw", str(pdf), "-"], check=True, capture_output=True, text=True).stdout
    lines = [ln.strip() for ln in text.replace("\f", "\n").splitlines()]
    lines = [ln for ln in lines if ln and not FURNITURE.match(ln)]
    return re.sub(r"\s+", " ", " ".join(lines)).strip()


def main():
    args = [a for a in sys.argv[1:] if a != "--update"]
    update = "--update" in sys.argv
    root = HERE.parents[1]
    cli = Path(args[0]) if args else root / "target" / "release" / ("wordcraft-cli.exe" if sys.platform == "win32" else "wordcraft-cli")
    expected_dir = HERE / "expected"
    expected_dir.mkdir(exist_ok=True)
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        masters = tmp / "masters"
        sys.argv = ["make_masters", str(masters)]
        make_masters.main()
        for case in CASES:
            name, master, off = case[:3]
            cmds = [x for c in (case[3] if len(case) > 3 else []) for x in ("--cmd", c)]
            out = tmp / "out"
            sys.argv = ["run_selection", str(cli), str(masters / master), str(out), name, *off, *cmds]
            run_selection.main()
            got = normalise(out / f"{name}.pdf")
            want_file = expected_dir / f"{name}.txt"
            if update or not want_file.exists():
                want_file.write_text(got + "\n", encoding="utf-8")
                print(f"{name}: expected text written")
                continue
            want = want_file.read_text(encoding="utf-8").strip()
            if got == want:
                print(f"{name}: OK")
            else:
                failures += 1
                i = next((k for k, (a, b) in enumerate(zip(got, want)) if a != b), min(len(got), len(want)))
                print(f"{name}: DIFFERENT at character {i}\n  expected: ...{want[max(0, i - 60):i + 60]}\n  got:      ...{got[max(0, i - 60):i + 60]}")
                shutil.copy(out / f"{name}.pdf", HERE / f"{name}.failed.pdf")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
