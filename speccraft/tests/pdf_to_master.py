"""Rebuild a MasterSpec-style guide spec PDF (pdftotext -layout output) as a styled .docx master.

For local testing only: the text belongs to the guide spec's publisher.
Usage: python3 pdf_to_master.py LAYOUT_TXT OUT_DOCX SECTION_NO TITLE
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from make_masters import write  # noqa: E402

PRT = re.compile(r"^PART \d+\s*-\s*(.+)$")
ART = re.compile(r"^\d+\.\d+\s+(.+)$")
MARK = re.compile(r"^(\s*)([A-Z]|\d+|[a-z])\.\s+(.+)$")
PAGE_LINE = re.compile(r"^\[(Project Name|Architect Name)")


def convert(lines, section_no, title):
    items = [("SCT", f"SECTION {section_no} - {title}")]
    cur = None  # [style, text]
    in_note = False
    started = False

    def flush():
        nonlocal cur
        if cur and cur[1].strip():
            items.append((cur[0], re.sub(r"\s+", " ", cur[1]).strip()))
        cur = None

    for raw in lines:
        line = raw.rstrip("\n").replace("\f", "")
        s = line.strip()
        if PAGE_LINE.match(s):
            continue
        if s.startswith("*****"):
            flush()
            in_note = not in_note
            continue
        if not started:
            if s.startswith("PART 1"):
                started = True
            else:
                continue
        if s == "END OF SECTION":
            flush()
            break
        if in_note:
            if not s or s == ".":
                flush()
                continue
            if cur is None:
                cur = ["CMT", s]
            else:
                cur[1] += " " + s
            continue
        if not s:
            continue
        m = PRT.match(s)
        if m:
            flush(); items.append(("PRT", m.group(1))); continue
        m = ART.match(s)
        if m and not line.startswith(" "):
            flush(); items.append(("ART", m.group(1))); continue
        m = MARK.match(line)
        if m:
            indent, mark, text = len(m.group(1)), m.group(2), m.group(3)
            if mark.isupper():
                style = "PR1"
            elif mark.isdigit():
                style = "PR2" if indent < 15 else "PR4"
            else:
                style = "PR3" if indent < 22 else "PR5"
            flush(); cur = [style, text]; continue
        if cur is not None:
            cur[1] += " " + s
    items.append(("EOS", f"END OF SECTION {section_no}"))
    return items


def main():
    txt, out, no, title = sys.argv[1:5]
    items = convert(Path(txt).read_text().splitlines(), no, title)
    write(out, items, no, "Guide spec rebuilt for local testing")
    counts = {}
    for st, *_ in items:
        counts[st] = counts.get(st, 0) + 1
    print(out, len(items), "paragraphs", counts)


if __name__ == "__main__":
    main()
