"""Import a master, switch on everything, then switch off the given ids (the engine takes their children along), export a PDF.

Usage: run_selection.py CLI MASTER.docx OUT_DIR NAME OFF_ID... [--cmd 'id={json}' ...]
Commands after --cmd run after the selection and before the export (fill-ins, for example).
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from items import items  # noqa: E402

def is_note(it):
    return it["style"] == "CMT" or it["text"].lstrip().startswith("//")


def main():
    args = sys.argv[1:]
    extra = []
    if "--cmd" in args:
        k = args.index("--cmd")
        args, rest = args[:k], args[k:]
        for i in range(0, len(rest) - 1, 2):
            extra += ["--cmd", rest[i + 1]]
    cli, master, out, name, *off = args
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    work = out / f"{name}.docx"
    shutil.copy(master, work)
    subprocess.run([cli, "run", "--file", str(work), "--cmd", "spec.import", "--save", str(work)], check=True)
    its = items(work)
    on = [it["id"] for it in its if not is_note(it)]
    pdf = out / f"{name}.pdf"
    subprocess.run([cli, "run", "--file", str(work),
                    "--cmd", "spec.activate=" + json.dumps({"ids": on}),
                    *(["--cmd", "spec.deactivate=" + json.dumps({"ids": off})] if off else []),
                    *extra,
                    "--cmd", "spec.exportPdf=" + json.dumps({"path": str(pdf)}),
                    "--save", str(work)], check=True)
    (out / f"{name}.on.json").write_text(json.dumps(on))
    print(name, "items", len(its), "switched off", off, "->", pdf)


if __name__ == "__main__":
    main()
