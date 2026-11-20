"""List the spec items of an imported section: id, style, text. Usage: items.py FILE.docx"""
import json
import sys
import zipfile
from xml.etree import ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"


def items(path):
    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("word/document.xml"))
    out = []
    for sdt in root.iter(W + "sdt"):
        tag = sdt.find(f"{W}sdtPr/{W}tag")
        if tag is None or not tag.get(W + "val", "").startswith("spec:"):
            continue
        p = sdt.find(f"{W}sdtContent/{W}p")
        if p is None:
            continue
        st = p.find(f"{W}pPr/{W}pStyle")
        text = "".join(t.text or "" for t in p.iter(W + "t"))
        out.append({"id": tag.get(W + "val")[5:], "style": st.get(W + "val") if st is not None else None, "text": text})
    return out


if __name__ == "__main__":
    print(json.dumps(items(sys.argv[1]), indent=0))
