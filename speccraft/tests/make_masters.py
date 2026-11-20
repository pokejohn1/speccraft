"""Write original test master sections in the SCT/PRT/ART/PR1-PR5/CMT/EOS format.

The wording is invented for Speccraft's tests; it is not taken from any published master.
Usage: python3 make_masters.py OUT_DIR
"""
import copy
import sys
from pathlib import Path

from docx import Document
from docx.enum.section import WD_ORIENT, WD_SECTION
from docx.enum.style import WD_STYLE_TYPE
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Inches, Pt, RGBColor

LEVELS = [  # style, number format, text, indent (in), hanging (in)
    ("PRT", "decimal", "PART %1 -", 0.0, 0.0),
    ("ART", "decimal", "%1.%2", 0.0, 0.5),
    ("PR1", "upperLetter", "%3.", 0.5, 0.4),
    ("PR2", "decimal", "%4.", 0.9, 0.4),
    ("PR3", "lowerLetter", "%5.", 1.3, 0.4),
    ("PR4", "decimal", "%6)", 1.7, 0.4),
    ("PR5", "lowerLetter", "%7)", 2.1, 0.4),
]


def el(tag, **attrs):
    e = OxmlElement(tag)
    for k, v in attrs.items():
        e.set(qn(k), str(v))
    return e


def add_numbering(doc):
    numbering = doc.part.numbering_part.element
    absn = el("w:abstractNum", **{"w:abstractNumId": 90})
    absn.append(el("w:multiLevelType", **{"w:val": "multilevel"}))
    for i, (style, fmt, text, ind, hang) in enumerate(LEVELS):
        lvl = el("w:lvl", **{"w:ilvl": i})
        lvl.append(el("w:start", **{"w:val": 1}))
        lvl.append(el("w:numFmt", **{"w:val": fmt}))
        lvl.append(el("w:pStyle", **{"w:val": style}))
        lvl.append(el("w:lvlText", **{"w:val": text}))
        lvl.append(el("w:lvlJc", **{"w:val": "left"}))
        ppr = el("w:pPr")
        ppr.append(el("w:ind", **{"w:left": int((ind + hang) * 1440), "w:hanging": int(hang * 1440)}))
        lvl.append(ppr)
        absn.append(lvl)
    first_num = numbering.find(qn("w:num"))
    if first_num is not None:
        first_num.addprevious(absn)
    else:
        numbering.append(absn)
    num = el("w:num", **{"w:numId": 90})
    num.append(el("w:abstractNumId", **{"w:val": 90}))
    numbering.append(num)
    return 90


def add_styles(doc, num_id):
    styles = doc.styles
    normal = styles["Normal"]
    normal.font.name = "Arial"
    normal.font.size = Pt(10)

    def para_style(name, ilvl=None, bold=False, caps=False, before=0, after=0, keep=False):
        st = styles.add_style(name, WD_STYLE_TYPE.PARAGRAPH)
        st.base_style = normal
        st.font.bold = bold
        st.font.all_caps = caps
        pf = st.paragraph_format
        pf.space_before = Pt(before)
        pf.space_after = Pt(after)
        pf.keep_with_next = keep
        if ilvl is not None:
            ppr = st.element.get_or_add_pPr()
            numpr = el("w:numPr")
            numpr.append(el("w:ilvl", **{"w:val": ilvl}))
            numpr.append(el("w:numId", **{"w:val": num_id}))
            ppr.insert(0, numpr)
        return st

    sct = para_style("SCT", bold=True, caps=True, after=12)
    sct.paragraph_format.alignment = 1
    para_style("PRT", 0, bold=True, caps=True, before=12, keep=True)
    para_style("ART", 1, bold=True, caps=True, before=12, keep=True)
    for i, name in enumerate(["PR1", "PR2", "PR3", "PR4", "PR5"]):
        para_style(name, i + 2, before=6 if i == 0 else 0)
    cmt = para_style("CMT", before=6, after=6)
    cmt.font.color.rgb = RGBColor(0x1F, 0x4E, 0x9A)
    cmt.font.italic = True
    eos = para_style("EOS", bold=True, caps=True, before=12)
    eos.paragraph_format.alignment = 1


def header_footer(section, section_no, version):
    section.header.paragraphs[0].text = "<Insert Project Name>\t\t<Insert Project Number>"
    f = section.footer.paragraphs[0]
    f.text = f"{version}\t{section_no} - "
    run = f.add_run()
    for tag, extra in (("w:fldChar", {"w:fldCharType": "begin"}), (None, None), ("w:fldChar", {"w:fldCharType": "end"})):
        if tag is None:
            it = el("w:instrText", **{"xml:space": "preserve"})
            it.text = " PAGE "
            run._r.append(it)
        else:
            run._r.append(el(tag, **extra))


def write(out, items, section_no, version):
    doc = Document()
    num_id = add_numbering(doc)
    add_styles(doc, num_id)
    sec = doc.sections[0]
    sec.left_margin = sec.right_margin = Inches(1)
    header_footer(sec, section_no, version)
    body_first = True
    for item in items:
        kind = item[0]
        if kind == "TABLE":
            rows = item[1]
            t = doc.add_table(rows=len(rows), cols=len(rows[0]))
            t.style = "Table Grid"
            for r, row in enumerate(rows):
                for c, txt in enumerate(row):
                    cell = t.cell(r, c)
                    cell.text = txt
                    if r == 0:
                        cell.paragraphs[0].runs[0].bold = True
            continue
        if kind == "LANDSCAPE":
            new = doc.add_section(WD_SECTION.NEW_PAGE)
            new.orientation = WD_ORIENT.LANDSCAPE
            new.page_width, new.page_height = new.page_height, new.page_width
            continue
        if kind == "PORTRAIT":
            new = doc.add_section(WD_SECTION.NEW_PAGE)
            new.orientation = WD_ORIENT.PORTRAIT
            new.page_width, new.page_height = new.page_height, new.page_width
            continue
        hidden = kind == "HIDDEN"
        style, text = (item[1], item[2]) if hidden else (item[0], item[1])
        p = doc.add_paragraph(style=style if style else None) if not body_first else doc.paragraphs[0] if doc.paragraphs else doc.add_paragraph()
        if body_first:
            body_first = False
            if style:
                p.style = doc.styles[style]
        run = p.add_run(text)
        if hidden:
            run.font.hidden = True
    doc.core_properties.title = items[0][1]
    doc.save(out)


DOOR_HARDWARE = [
    ("SCT", "SECTION 08 7100 - DOOR HARDWARE"),
    ("PRT", "GENERAL"),
    ("ART", "SUMMARY"),
    ("CMT", "Edit this article to suit the Project. Delete products not required."),
    ("PR1", "Section includes:"),
    ("PR2", "Hinges and continuous hinges."),
    ("PR2", "Mechanical locks and latches."),
    ("PR2", "Exit devices."),
    ("PR2", "Door closers."),
    ("PR2", "Stops, holders and protective plates."),
    ("PR2", "Weatherstripping and thresholds."),
    ("PR1", "Related requirements:"),
    ("PR2", "Section 08 1113 \"Hollow Metal Doors and Frames\" for door preparation."),
    ("PR2", "Section 08 1416 \"Flush Wood Doors\" for factory-prepared wood doors."),
    ("PR2", "Section 28 1300 \"Access Control\" for card readers and their wiring."),
    ("ART", "SUBMITTALS"),
    ("PR1", "Product data: for each type of hardware item, showing finish and options selected."),
    ("PR1", "Hardware schedule: organized by hardware set, listing each opening served."),
    ("PR2", "Show the location of each item on each door."),
    ("PR2", "Coordinate the schedule with door and frame shop drawings."),
    ("PR3", "Submit the schedule before door and frame shop drawings are approved."),
    ("PR3", "Include keying information on a separate sheet."),
    ("PR1", "  // Retain the next paragraph only if the Owner keeps a permanent master key system."),
    ("PR1", "Keying schedule: prepared after a meeting with the Owner, showing each key group."),
    ("PR1", "Samples: for each exposed finish, 4 inches square."),
    ("ART", "QUALITY ASSURANCE"),
    ("PR1", "Installer qualifications: a firm with at least five years of experience installing similar hardware."),
    ("PR1", "Hardware consultant: a person certified as an architectural hardware consultant, available during the Work."),
    ("PR1", "Fire-rated openings: hardware listed for use on rated doors of the rating shown."),
    ("ART", "DELIVERY, STORAGE AND HANDLING"),
    ("PR1", "Deliver each item packed and labeled with its hardware set number."),
    ("PR1", "Store hardware in a locked, dry room on site."),
    ("ART", "WARRANTY"),
    ("PR1", "Manufacturer's warranty for the following periods from Substantial Completion:"),
    ("PR2", "Exit devices: three years."),
    ("PR2", "Door closers: ten years."),
    ("PR2", "Electrified hardware: two years."),
    ("PRT", "PRODUCTS"),
    ("ART", "HINGES"),
    ("CMT", "Select hinge weight to suit door size and frequency of use."),
    ("PR1", "Butt hinges: five-knuckle, ball-bearing, template type."),
    ("PR2", "Exterior doors: stainless steel, with nonremovable pins."),
    ("PR2", "Interior doors: steel, plated finish."),
    ("PR2", "Size: 4-1/2 by 4-1/2 inches unless otherwise indicated."),
    ("PR1", "Continuous hinges: aluminum, geared, full height of door."),
    ("ART", "LOCKS AND LATCHES"),
    ("PR1", "Mortise locks: heavy duty, with lever trim."),
    ("PR2", "Lever design: [Return lever] [Straight lever with end cap]."),
    ("PR2", "Backset: 2-3/4 inches."),
    ("PR1", "Cylindrical locks: heavy duty, for interior doors where mortise locks are not scheduled."),
    ("ART", "EXIT DEVICES"),
    ("PR1", "Rim exit devices: touch-pad type, listed for panic hardware."),
    ("PR1", "Concealed vertical rod devices: for pairs of doors without a mullion."),
    ("PR2", "Provide less-bottom-rod devices where floors are finished with carpet."),
    ("ART", "CLOSERS"),
    ("PR1", "Surface closers: cast iron body, full rack-and-pinion, adjustable spring power."),
    ("PR2", "Provide parallel arm mounting on the push side of out-swinging doors."),
    ("PR2", "Provide back-check and delayed action on accessible doors."),
    ("ART", "HARDWARE FINISHES"),
    ("PR1", "Provide the following finishes unless otherwise indicated:"),
    ("TABLE", [
        ["Item", "Finish", "BHMA code"],
        ["Hinges, exterior", "Satin stainless steel", "630"],
        ["Hinges, interior", "Satin chrome, plated", "652"],
        ["Locks and levers", "Satin chrome", "626"],
        ["Closers", "Powder coat, aluminum colour", "689"],
    ]),
    ("PR1", "Match finishes of items on the same door."),
    ("PRT", "EXECUTION"),
    ("ART", "EXAMINATION"),
    ("PR1", "Examine doors and frames for correct preparation before installing hardware."),
    ("ART", "INSTALLATION"),
    ("PR1", "Mount hardware at the heights recommended by the door and hardware manufacturers."),
    ("PR1", "Install each item in accordance with its manufacturer's instructions."),
    ("PR2", "Drill and tap for machine screws; do not use self-tapping screws on metal doors."),
    ("PR2", "Set thresholds in a full bed of sealant."),
    ("ART", "ADJUSTING"),
    ("PR1", "Adjust closers so that doors close fully from 70 degrees in at least three seconds."),
    ("PR1", "Return to the site six months after Substantial Completion and readjust each closer."),
    ("ART", "HARDWARE SCHEDULE"),
    ("CMT", "List each hardware set. The sets below show the format only."),
    ("PR1", "Hardware Set 01: exterior pair, exit devices, card access."),
    ("PR1", "Hardware Set 02: interior single door, office lock."),
    ("EOS", "END OF SECTION 08 7100"),
]

JOINT_SEALANTS = [
    ("SCT", "SECTION 07 9200 - JOINT SEALANTS"),
    ("PRT", "GENERAL"),
    ("ART", "SUMMARY"),
    ("PR1", "Section includes sealants for the following joints:"),
    ("PR2", "Exterior joints in vertical surfaces and horizontal non-traffic surfaces."),
    ("PR2", "Exterior joints in horizontal traffic surfaces."),
    ("PR2", "Interior joints in vertical surfaces and horizontal non-traffic surfaces."),
    ("PR2", "Interior joints in wet areas."),
    ("ART", "ACTION SUBMITTALS"),
    ("PR1", "Product data: for each joint sealant product."),
    ("PR1", "Samples for colour selection: cured sample strips showing the full range of colours."),
    ("PR1", "Joint sealant schedule: include the following for each joint type:"),
    ("PR2", "Joint sealant application, joint location and designation."),
    ("PR2", "Joint sealant manufacturer and product name."),
    ("PR2", "Joint sealant formulation."),
    ("PR2", "Joint sealant colour."),
    ("ART", "INFORMATIONAL SUBMITTALS"),
    ("PR1", "Qualification data: for the installer."),
    ("PR1", "Field adhesion test reports: for each sealant and substrate combination tested."),
    ("ART", "QUALITY ASSURANCE"),
    ("PR1", "Installer: a firm employing applicators trained by the sealant manufacturer."),
    ("PR1", "Mockups: install sealant joints in mockups of assemblies specified in other Sections."),
    ("ART", "FIELD CONDITIONS"),
    ("PR1", "Do not proceed with installation under the following conditions:"),
    ("PR2", "When ambient and substrate temperatures are outside the limits permitted by the manufacturer, or below ___ deg F."),
    ("PR2", "When joint substrates are wet."),
    ("PR2", "Where joint widths are less than those allowed by the manufacturer."),
    ("ART", "WARRANTY"),
    ("PR1", "Installer's warranty: replace sealants that fail to seal within ___ years from Substantial Completion."),
    ("PR1", "Manufacturer's warranty: replace sealants that fail to perform within ___ years."),
    ("PRT", "PRODUCTS"),
    ("ART", "MATERIALS, GENERAL"),
    ("PR1", "Compatibility: provide sealants, primers and backings compatible with one another and with joint substrates."),
    ("PR1", "Colours of exposed sealants: [As selected by Architect from manufacturer's full range] [Match adjacent surface] [<Insert colour>]."),
    ("PR1", "VOC content: sealants used inside the weatherproofing system shall not exceed ___ g/L."),
    ("CMT", "Retain the next article for metal panel and curtain wall joints."),
    ("ART", "SILICONE JOINT SEALANTS"),
    ("PR1", "Silicone, nonstaining, S, NS, 100/50, NT: single-component, nonsag, plus 100 percent and minus 50 percent movement capability."),
    ("PR2", "Basis-of-design product: <Insert manufacturer and product>."),
    ("PR3", "Substitutions: under provisions of Division 01."),
    ("PR4", "Submit a comparison of movement capability and adhesion."),
    ("PR5", "Include test results from an independent laboratory."),
    ("PR1", "Mildew-resistant silicone: for interior wet areas, formulated with a fungicide."),
    ("ART", "URETHANE JOINT SEALANTS"),
    ("PR1", "Urethane, M, P, 25, T, NT: multicomponent, pourable, traffic grade, for horizontal joints."),
    ("PR1", "Urethane, S, NS, 25, NT: single-component, nonsag, for vertical joints in concrete and masonry."),
    ("HIDDEN", "PR1", "Hidden revision note: urethane products to be confirmed by the Owner."),
    ("ART", "JOINT-SEALANT BACKING"),
    ("PR1", "Cylindrical sealant backings: closed-cell polyethylene, sized 25 percent larger than the joint width."),
    ("PR1", "Bond-breaker tape: polyethylene tape where backing rods are not used."),
    ("ART", "MISCELLANEOUS MATERIALS"),
    ("PR1", "Primer: as recommended by the sealant manufacturer for each substrate."),
    ("PR1", "Cleaners for nonporous surfaces: chemical cleaners that do not stain or damage the surface."),
    ("PRT", "EXECUTION"),
    ("ART", "PREPARATION"),
    ("PR1", "Clean joints immediately before installing sealants."),
    ("PR2", "Remove laitance and form-release agents from concrete."),
    ("PR2", "Remove loose particles and dust by brushing or blowing with oil-free air."),
    ("PR1", "Prime joint substrates where recommended by the manufacturer."),
    (None, "Unstyled paragraph left in the master by mistake."),
    ("ART", "INSTALLATION"),
    ("PR1", "Install sealant backings of the type indicated to support sealants during tooling."),
    ("PR1", "Install sealants to fill joints completely, wetting the joint surfaces."),
    ("PR1", "Tool sealants immediately after application to form a smooth, concave joint."),
    ("ART", "FIELD QUALITY CONTROL"),
    ("PR1", "Field adhesion testing: test ___ joints for every 1000 feet of joint length."),
    ("ART", "JOINT-SEALANT SCHEDULE"),
    ("CMT", "The schedule below is in landscape. Edit joint types to suit the Project."),
    ("LANDSCAPE",),
    ("PR1", "Joint sealant types:"),
    ("TABLE", [
        ["Type", "Application", "Locations", "Sealant", "Colour"],
        ["JS-1", "Exterior vertical", "Control joints in masonry; perimeter of frames", "Silicone, S, NS, 100/50", "Match adjacent"],
        ["JS-2", "Exterior horizontal traffic", "Joints in paving and walkways", "Urethane, M, P, 25, T", "Grey"],
        ["JS-3", "Interior vertical", "Perimeter of frames; partitions at walls", "Acrylic latex", "Paint to match"],
        ["JS-4", "Interior wet areas", "Plumbing fixtures; tile at countertops", "Mildew-resistant silicone", "White"],
    ]),
    ("PR1", "Provide additional joint types as required by the Contract Documents."),
    ("PORTRAIT",),
    ("EOS", "END OF SECTION 07 9200"),
]


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    write(out / "087100 Door Hardware.docx", DOOR_HARDWARE, "08 7100", "Speccraft test master: 11.2026")
    write(out / "079200 Joint Sealants.docx", JOINT_SEALANTS, "07 9200", "Speccraft test master: 11.2026")
    print("written to", out)


if __name__ == "__main__":
    main()
