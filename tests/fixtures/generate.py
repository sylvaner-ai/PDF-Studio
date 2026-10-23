#!/usr/bin/env python3
"""Génère le jeu de PDF de test publics de PDF Studio.

Usage : python3 tests/fixtures/generate.py
Dépendances : reportlab, pikepdf, pillow, fonttools
(pip install reportlab pikepdf pillow fonttools).

Les fichiers produits sont déterministes (dates et identifiants figés) pour que
leur régénération ne modifie pas le dépôt, sauf chiffre.pdf (cf. chiffre()). Les documents personnels de
l'utilisateur ne sont jamais générés ici : ils vivent dans tests/fixtures/private/,
exclu du dépôt.
"""

from __future__ import annotations

import io
import random
from pathlib import Path

import pikepdf
from PIL import Image, ImageDraw, ImageFilter, ImageFont
from reportlab.lib import colors
from reportlab.lib.pagesizes import A4, A5, landscape, letter
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.lib.units import cm
from reportlab.pdfgen import canvas
from reportlab.platypus import SimpleDocTemplate, Table, TableStyle, Paragraph, Spacer

OUT = Path(__file__).resolve().parent
SEED = 20261021

LOREM = (
    "Le cœur de l'œuvre réside dans l'efficacité : affiner, réfléchir, "
    "déléguer. À l'été, Noël et Ève visitèrent Évreux ; « ça » coûte 12,50 € "
    "TTC, soit 15 % de plus qu'en 2025. The quick brown fox jumps over the lazy dog."
)


def new_canvas(name: str, pagesize=A4, title: str | None = None) -> canvas.Canvas:
    c = canvas.Canvas(str(OUT / name), pagesize=pagesize, invariant=1)
    if title:
        c.setTitle(title)
    c.setAuthor("PDF Studio — jeu de test")
    return c


def text_page(c: canvas.Canvas, number: int, total: int) -> None:
    width, height = c._pagesize
    c.setFont("Helvetica-Bold", 18)
    c.drawString(2 * cm, height - 2.5 * cm, f"Page {number} sur {total}")
    c.setFont("Times-Roman", 11)
    y = height - 4 * cm
    for line in range(30):
        c.drawString(2 * cm, y, f"{line + 1:02d}. {LOREM[: 70 + (line * 7) % 40]}")
        y -= 0.7 * cm
    c.setFont("Helvetica", 9)
    c.drawRightString(width - 2 * cm, 1.5 * cm, f"{number}")


def texte_simple() -> None:
    """Une page de texte : plusieurs polices, accents, ligatures, couleurs."""
    c = new_canvas("texte_simple.pdf", title="Texte simple")
    width, height = A4
    y = height - 3 * cm
    for font, size in [
        ("Helvetica", 12),
        ("Helvetica-Bold", 14),
        ("Times-Roman", 12),
        ("Times-Italic", 12),
        ("Courier", 11),
    ]:
        c.setFont(font, size)
        c.drawString(2 * cm, y, f"{font} {size} pt — {LOREM[:60]}")
        y -= 1.2 * cm
    c.setFillColor(colors.HexColor("#1f4e9c"))
    c.setFont("Helvetica-Bold", 20)
    c.drawString(2 * cm, y - cm, "Titre en bleu profond")
    c.setFillColor(colors.HexColor("#b3261e"))
    c.setFont("Times-Bold", 12)
    c.drawString(2 * cm, y - 2.2 * cm, "Avertissement : ligatures fi fl ffi, chiffres 0123456789.")
    c.showPage()
    c.save()


def multipage(name: str, pages: int, title: str) -> None:
    c = new_canvas(name, title=title)
    for n in range(1, pages + 1):
        text_page(c, n, pages)
        c.showPage()
    c.save()


def signets() -> None:
    """Document de 6 pages avec un plan hiérarchique."""
    c = new_canvas("signets.pdf", title="Document avec signets")
    chapters = ["Introduction", "Méthode", "Résultats"]
    page = 0
    height = A4[1]
    for i, chapter in enumerate(chapters):
        for part in range(2):
            page += 1
            if part == 0:
                # Destination /Fit : la page entière.
                c.bookmarkPage(f"chapitre{i + 1}")
                c.addOutlineEntry(f"{i + 1}. {chapter}", f"chapitre{i + 1}", level=0)
            # Destination /XYZ : la première ligne du texte, à 4 cm du haut.
            key = f"section{page}"
            c.bookmarkHorizontalAbsolute(key, top=height - 4 * cm)
            c.addOutlineEntry(f"{i + 1}.{part + 1} Section {part + 1}", key, level=1)
            text_page(c, page, 6)
            c.showPage()
    c.showOutline()
    c.save()


def formats_mixtes() -> None:
    """Pages de formats et d'orientations différents, dont une page /Rotate 90."""
    c = new_canvas("formats_mixtes.pdf", title="Formats mixtes")
    for size, label in [
        (A4, "A4 portrait"),
        (landscape(A4), "A4 paysage"),
        (letter, "US Letter"),
        (A5, "A5"),
        (A4, "A4 avec rotation de 90°"),
    ]:
        c.setPageSize(size)
        c.setFont("Helvetica-Bold", 24)
        c.drawString(2 * cm, size[1] - 3 * cm, label)
        c.rect(1 * cm, 1 * cm, size[0] - 2 * cm, size[1] - 2 * cm)
        c.showPage()
    c.save()
    with pikepdf.open(OUT / "formats_mixtes.pdf", allow_overwriting_input=True) as pdf:
        pdf.pages[4].Rotate = 90
        save(pdf, "formats_mixtes.pdf")


def vectoriel() -> None:
    """Tracés, dégradé et transparence."""
    c = new_canvas("vectoriel.pdf", title="Graphismes vectoriels")
    width, height = A4
    c.linearGradient(2 * cm, height - 8 * cm, width - 2 * cm, height - 8 * cm,
                     (colors.HexColor("#1f4e9c"), colors.white), extend=False)
    c.setFillColor(colors.HexColor("#1f4e9c"))
    c.setFillAlpha(0.5)
    c.circle(width / 2, height / 2, 4 * cm, fill=1, stroke=0)
    c.setFillColor(colors.HexColor("#b3261e"))
    c.circle(width / 2 + 3 * cm, height / 2, 4 * cm, fill=1, stroke=0)
    c.setFillAlpha(1)
    c.setLineWidth(3)
    c.setStrokeColor(colors.black)
    path = c.beginPath()
    path.moveTo(2 * cm, 5 * cm)
    path.curveTo(6 * cm, 12 * cm, 12 * cm, 0, width - 2 * cm, 6 * cm)
    c.drawPath(path, stroke=1, fill=0)
    c.showPage()
    c.save()


def tableau() -> None:
    """Facture fictive avec un tableau à bordures."""
    doc = SimpleDocTemplate(str(OUT / "tableau.pdf"), pagesize=A4, title="Facture",
                            author="PDF Studio — jeu de test", invariant=1)
    styles = getSampleStyleSheet()
    rows = [["Désignation", "Quantité", "Prix unitaire", "Total"]]
    for i in range(1, 9):
        rows.append([f"Prestation n° {i}", str(i), f"{i * 12.5:.2f} €", f"{i * i * 12.5:.2f} €"])
    rows.append(["", "", "Total HT", "2 550,00 €"])
    table = Table(rows, colWidths=[7 * cm, 2.5 * cm, 3.5 * cm, 3.5 * cm])
    table.setStyle(TableStyle([
        ("GRID", (0, 0), (-1, -2), 0.5, colors.grey),
        ("BACKGROUND", (0, 0), (-1, 0), colors.HexColor("#e8ecf4")),
        ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
        ("ALIGN", (1, 1), (-1, -1), "RIGHT"),
        ("SPAN", (0, -1), (1, -1)),
    ]))
    doc.build([Paragraph("Facture n° 2026-042", styles["Title"]), Spacer(1, cm), table])


def scan_simule() -> None:
    """Page entièrement en image (aucun texte natif), légèrement inclinée et bruitée."""
    rng = random.Random(SEED)
    dpi = 150
    w, h = int(8.27 * dpi), int(11.69 * dpi)
    img = Image.new("L", (w, h), 245)
    draw = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype("DejaVuSerif.ttf", 22)
    except OSError:
        font = ImageFont.load_default()
    y = 120
    for line in range(28):
        draw.text((110, y), LOREM[: 60 + (line * 5) % 30], fill=25, font=font)
        y += 48
    img = img.rotate(0.8, resample=Image.BICUBIC, fillcolor=245)
    noise = Image.effect_noise((w, h), 18).point(lambda v: v // 8)
    img = Image.blend(img, noise.convert("L"), 0.08).filter(ImageFilter.GaussianBlur(0.6))
    for _ in range(400):
        x, yy = rng.randrange(w), rng.randrange(h)
        draw = ImageDraw.Draw(img)
        draw.point((x, yy), fill=rng.randrange(80, 200))
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=70)
    buf.seek(0)
    c = new_canvas("scan_simule.pdf", title="Scan simulé")
    from reportlab.lib.utils import ImageReader
    c.drawImage(ImageReader(buf), 0, 0, width=A4[0], height=A4[1])
    c.showPage()
    c.save()


ORGANISATION_PAGES = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]


def organisation() -> None:
    """Cinq pages (Alpha à Echo) réunissant ce que l'organiseur doit préserver.

    Rotation de 90° pour Delta et Echo, numérotation logique, plan, liens internes
    (action GoTo et destination directe), lien Web, note avec sa fenêtre, champs
    de formulaire sur Alpha et Delta, métadonnées XMP et préférences d'affichage.
    Les attributs hérités de l'arbre des pages sont construits par les tests
    (pdf_core/tests/ecriture.rs) : qpdf les recopierait dans chaque page.
    """
    c = new_canvas("organisation.pdf", title="Organisation des pages")
    for n, name in enumerate(ORGANISATION_PAGES, start=1):
        c.setFont("Helvetica-Bold", 28)
        c.drawString(2 * cm, A4[1] - 3 * cm, f"Feuille {name}")
        c.setFont("Helvetica", 12)
        c.drawString(2 * cm, A4[1] - 4.5 * cm, f"Page {n} sur {len(ORGANISATION_PAGES)}")
        c.showPage()
    c.save()

    with pikepdf.open(OUT / "organisation.pdf", allow_overwriting_input=True) as pdf:
        pages = [page.obj for page in pdf.pages]
        for page in pages[3:]:
            page.Rotate = 90

        pdf.Root.PageLabels = pikepdf.Dictionary(Nums=pikepdf.Array([
            0, pikepdf.Dictionary(S=pikepdf.Name.r),
            2, pikepdf.Dictionary(S=pikepdf.Name.D),
        ]))
        pdf.Root.ViewerPreferences = pikepdf.Dictionary(DisplayDocTitle=True)
        pdf.Root.Metadata = pdf.make_stream(
            b'<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>'
            b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
            b'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description '
            b'xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title><rdf:Alt>'
            b'<rdf:li xml:lang="x-default">Organisation des pages</rdf:li></rdf:Alt></dc:title>'
            b'</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>',
            Type=pikepdf.Name.Metadata, Subtype=pikepdf.Name.XML)

        def fit(page):
            return pikepdf.Array([page, pikepdf.Name.Fit])

        def link(rect, **entries):
            return pdf.make_indirect(pikepdf.Dictionary(
                Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Link,
                Rect=pikepdf.Array(rect), Border=pikepdf.Array([0, 0, 0]), **entries))

        def field(page, name, rect):
            return pdf.make_indirect(pikepdf.Dictionary(
                Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Widget, FT=pikepdf.Name.Tx,
                T=pikepdf.String(name), V=pikepdf.String(""), Rect=pikepdf.Array(rect),
                F=4, P=page, DA=pikepdf.String("/Helv 12 Tf 0 g")))

        # Alpha : liens vers Charlie (action GoTo) et vers Echo (destination), champ « nom ».
        to_charlie = link([50, 600, 250, 620], A=pikepdf.Dictionary(S=pikepdf.Name.GoTo, D=fit(pages[2])))
        to_echo = link([50, 560, 250, 580], Dest=fit(pages[4]))
        name_field = field(pages[0], "nom", [50, 500, 300, 520])
        pages[0].Annots = pikepdf.Array([to_charlie, to_echo, name_field])

        # Bravo : lien Web et note avec sa fenêtre.
        web = link([50, 600, 250, 620], A=pikepdf.Dictionary(
            S=pikepdf.Name.URI, URI=pikepdf.String("https://example.org/")))
        note = pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Text, Rect=pikepdf.Array([400, 700, 420, 720]),
            Contents=pikepdf.String("Note de relecture"), P=pages[1], Name=pikepdf.Name.Comment))
        popup = pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Popup, Rect=pikepdf.Array([420, 600, 560, 700]),
            Parent=note, P=pages[1]))
        note.Popup = popup
        pages[1].Annots = pikepdf.Array([web, note, popup])

        # Delta : champ « ville ».
        city_field = field(pages[3], "ville", [50, 500, 300, 520])
        pages[3].Annots = pikepdf.Array([city_field])
        helvetica = pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name.Helvetica,
            Encoding=pikepdf.Name.WinAnsiEncoding))
        pdf.Root.AcroForm = pikepdf.Dictionary(
            Fields=pikepdf.Array([name_field, city_field]), NeedAppearances=True,
            DR=pikepdf.Dictionary(Font=pikepdf.Dictionary(Helv=helvetica)),
            DA=pikepdf.String("/Helv 12 Tf 0 g"))

        # Plan : une entrée par page, Delta et Echo regroupées sous « Annexes ».
        outlines = pdf.make_indirect(pikepdf.Dictionary(Type=pikepdf.Name.Outlines))
        entries = [pdf.make_indirect(pikepdf.Dictionary(Title=pikepdf.String(name), Dest=fit(page)))
                   for name, page in zip(ORGANISATION_PAGES, pages)]
        annexes = pdf.make_indirect(pikepdf.Dictionary(Title=pikepdf.String("Annexes"), Count=2))
        top = entries[:3] + [annexes]

        def chain(parent, children):
            for i, child in enumerate(children):
                child.Parent = parent
                if i > 0:
                    child.Prev = children[i - 1]
                if i + 1 < len(children):
                    child.Next = children[i + 1]
            parent.First = children[0]
            parent.Last = children[-1]

        chain(outlines, top)
        chain(annexes, entries[3:])
        outlines.Count = len(top) + 2
        pdf.Root.Outlines = outlines
        save(pdf, "organisation.pdf")


def signature_factice() -> None:
    """Formulaire portant un champ de signature rempli (signature factice, non
    vérifiable) : sert à tester l'alerte avant modification d'un document signé."""
    with pikepdf.open(OUT / "texte_simple.pdf") as pdf:
        page = pdf.pages[0].obj
        signature = pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Sig, Filter=pikepdf.Name("/Adobe.PPKLite"),
            SubFilter=pikepdf.Name("/adbe.pkcs7.detached"),
            ByteRange=pikepdf.Array([0, 0, 0, 0]), Contents=pikepdf.String(b"\x00" * 16)))
        widget = pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Widget, FT=pikepdf.Name.Sig,
            T=pikepdf.String("signature"), V=signature, Rect=pikepdf.Array([0, 0, 0, 0]), F=132, P=page))
        page.Annots = pikepdf.Array([widget])
        pdf.Root.AcroForm = pikepdf.Dictionary(Fields=pikepdf.Array([widget]), SigFlags=3)
        save(pdf, "signature_factice.pdf")


EDITION_PARAGRAPHE = (
    "Le chiffre d'affaires du trimestre progresse de 12 % par rapport à "
    "l'an dernier, porté par les nouveaux contrats d'infogérance. La marge "
    "brute reste stable malgré la hausse des coûts de l'énergie, et la "
    "trésorerie couvre six mois d'activité."
)


def edition() -> None:
    """Contenu à modifier (jalon 4) : titre et paragraphe justifié dans une
    police TrueType intégrée en sous-ensemble (Vera), ligne en Helvetica non
    intégrée, image, tracés ; seconde page tournée de 90°."""
    from reportlab.lib.enums import TA_JUSTIFY
    from reportlab.lib.styles import ParagraphStyle
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.ttfonts import TTFont

    pdfmetrics.registerFont(TTFont("Vera", "Vera.ttf"))
    pdfmetrics.registerFont(TTFont("VeraBd", "VeraBd.ttf"))
    c = new_canvas("edition.pdf", title="Édition du contenu")
    width, height = A4
    c.setFont("VeraBd", 20)
    c.drawString(2 * cm, height - 3 * cm, "Rapport trimestriel")
    style = ParagraphStyle("corps", fontName="Vera", fontSize=11, leading=14,
                           alignment=TA_JUSTIFY)
    paragraph = Paragraph(EDITION_PARAGRAPHE, style)
    _, used = paragraph.wrap(12 * cm, 10 * cm)
    paragraph.drawOn(c, 2 * cm, height - 4.5 * cm - used)
    c.setFont("Helvetica", 12)
    c.setFillColor(colors.HexColor("#b3261e"))
    c.drawString(2 * cm, height - 9 * cm, "Texte en Helvetica, police standard.")
    c.setFillColor(colors.black)

    image = Image.new("RGB", (60, 40), "#1f4e9c")
    ImageDraw.Draw(image).ellipse((10, 5, 50, 35), fill="#f2c94c")
    buffer = io.BytesIO()
    image.save(buffer, "PNG")
    buffer.seek(0)
    from reportlab.lib.utils import ImageReader
    c.drawImage(ImageReader(buffer), 2 * cm, height - 14 * cm, 4 * cm, 8 / 3 * cm)

    c.setStrokeColor(colors.HexColor("#2e7d32"))
    c.setLineWidth(2)
    c.rect(10 * cm, height - 14 * cm, 5 * cm, 3 * cm, stroke=1, fill=0)
    c.line(10 * cm, height - 15 * cm, 15 * cm, height - 15 * cm)
    c.showPage()

    c.setFont("Helvetica", 14)
    c.drawString(2 * cm, height - 3 * cm, "Page tournée")
    c.showPage()
    c.save()

    with pikepdf.open(OUT / "edition.pdf", allow_overwriting_input=True) as pdf:
        pdf.pages[1].obj.Rotate = 90
        save(pdf, "edition.pdf")


POLICE_CFF_LIGNES = ("Montant prélevé : 42,50 €", "Échéance réglée")


def police_cff() -> None:
    """Police CFF intégrée (Type1C), réduite aux caractères utilisés, sans
    /Encoding dans le PDF : les codes suivent l'encodage propre au programme
    (« é » en 0xE9, qui vaut « Ø » en StandardEncoding), comme dans des
    factures produites par certains logiciels."""
    import reportlab
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.t2CharStringPen import T2CharStringPen
    from fontTools.ttLib import TTFont as FontFile

    source = FontFile(Path(reportlab.__file__).parent / "fonts" / "Vera.ttf")
    cmap = source.getBestCmap()
    glyphs = source.getGlyphSet()
    units = source["head"].unitsPerEm
    chars = sorted(set("".join(POLICE_CFF_LIGNES)))
    names = [".notdef"] + [cmap[ord(c)] for c in chars]
    charstrings = {}
    for name in names:
        pen = T2CharStringPen(glyphs[name].width, glyphs)
        glyphs[name].draw(pen)
        charstrings[name] = pen.getCharString()

    builder = FontBuilder(units, isTTF=False)
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({ord(c): cmap[ord(c)] for c in chars})
    builder.setupCFF("VeraCFF", {"FullName": "Vera CFF"}, charstrings, {})
    builder.setupHorizontalMetrics({name: (glyphs[name].width, 0) for name in names})
    cff = builder.font["CFF "].cff
    encoding = [".notdef"] * 256
    codes = {c: c.encode("cp1252")[0] for c in chars}
    for c, code in codes.items():
        encoding[code] = cmap[ord(c)]
    cff.topDictIndex[0].Encoding = encoding
    program = io.BytesIO()
    cff.compile(program, builder.font)

    pdf = pikepdf.new()
    by_code = {code: c for c, code in codes.items()}
    widths = [
        round(glyphs[cmap[ord(by_code[code])]].width * 1000 / units) if code in by_code else 0
        for code in range(32, 256)
    ]
    fontfile = pikepdf.Stream(pdf, program.getvalue())
    fontfile.Subtype = pikepdf.Name.Type1C
    descriptor = pdf.make_indirect(pikepdf.Dictionary(
        Type=pikepdf.Name.FontDescriptor, FontName=pikepdf.Name("/ABCDEF+VeraCFF"),
        Flags=32, FontBBox=[-183, -236, 1287, 928], ItalicAngle=0, Ascent=928,
        Descent=-236, CapHeight=729, StemV=80, FontFile3=fontfile,
    ))
    font = pdf.make_indirect(pikepdf.Dictionary(
        Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1,
        BaseFont=pikepdf.Name("/ABCDEF+VeraCFF"), FirstChar=32, LastChar=255,
        Widths=widths, FontDescriptor=descriptor,
    ))
    lines = [line.encode("cp1252") for line in POLICE_CFF_LIGNES]
    content = b"BT /F1 18 Tf 72 760 Td (" + lines[0] + b") Tj 0 -40 Td (" + lines[1] + b") Tj ET"
    pdf.add_blank_page(page_size=(595, 842))
    page = pdf.pages[0]
    page.obj.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font))
    page.obj.Contents = pdf.make_stream(content)
    save(pdf, "police_cff.pdf")


ENCADRE_LIGNES = (
    ("Date de facture", "02/04/2026"),
    ("N° de facture", "12345678901234"),
    ("Remise à zéro", "01/01/2026"),
    ("Fin d'engagement", "Sans engagement"),
    ("N° de compte client", "87654321"),
    ("N° de ligne", "01 23 45 67 89"),
    ("Référence", "XX-1234-5678"),
)


def encadre() -> None:
    """Encadré d'étiquettes et de valeurs, comme en tête de certaines factures :
    une ligne par objet BT, étiquettes en gras bleu nuit à gauche, valeurs en
    gras bleu clair à droite, souvent précédées d'une espace noire dans une
    autre police ; la première étiquette partage son BT avec le pied de page,
    et deux lignes enchaînent étiquette et valeur sans saut de ligne de base
    (assez près pour former une seule ligne de texte)."""
    pdf = pikepdf.new()
    fonts = {
        name: pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1,
            BaseFont=pikepdf.Name("/" + base), Encoding=pikepdf.Name.WinAnsiEncoding,
        ))
        for name, base in (("F1", "Helvetica-Bold"), ("F2", "Helvetica"))
    }

    def show(text: str) -> bytes:
        escaped = text.encode("cp1252").replace(b"\\", b"\\\\")
        return b"(" + escaped.replace(b"(", b"\\(").replace(b")", b"\\)") + b") Tj "

    label = b"0 0.11 0.369 rg /F1 9 Tf "
    value = b"0 0.58 0.729 rg /F1 9 Tf "
    space = b"0 g /F2 9 Tf "
    content = b""
    for row, (name, data) in enumerate(ENCADRE_LIGNES):
        y = 680.2 - 12.7 * row
        content += b"BT "
        if row == 0:
            content += b"0.4 g /F2 6 Tf 1 0 0 1 36 40 Tm " + show("Page 1/1 · facture fictive")
        content += label + b"1 0 0 1 31.2 %.1f Tm " % y
        if row == 3:
            # Étiquette, espaces de même style, puis la valeur.
            content += show(name + "   ")
        elif row == 4:
            # Étiquette, une espace dans une autre police, puis la valeur.
            content += show(name + " ") + space + show(" ")
        elif row == 0:
            content += show(name + "   ") + space + b"1 0 0 1 119.3 %.1f Tm " % (y + 5.5) + show(" ")
        else:
            content += show(name + "   ") + space + b"1 0 0 1 119.3 %.1f Tm " % (y + 0.5) + show(" ")
        content += value + b"1 0 0 1 127.4 %.1f Tm " % y + show(data) + b"ET\n"
    pdf.add_blank_page(page_size=(595, 842))
    page = pdf.pages[0]
    page.obj.Resources = pikepdf.Dictionary(Font=pikepdf.Dictionary(**fonts))
    page.obj.Contents = pdf.make_stream(content)
    save(pdf, "encadre.pdf")


def save(pdf: pikepdf.Pdf, name: str, **kwargs) -> None:
    pdf.save(OUT / name, static_id=True, deterministic_id=False, **kwargs)


def chiffre() -> None:
    """Copie de texte_simple.pdf protégée par le mot de passe « motdepasse ».

    Le chiffrement AES tire un sel aléatoire : le fichier n'est donc régénéré que
    s'il est absent, pour ne pas modifier le dépôt à chaque exécution.
    """
    if (OUT / "chiffre.pdf").exists():
        return
    with pikepdf.open(OUT / "texte_simple.pdf") as pdf:
        save(pdf, "chiffre.pdf",
             encryption=pikepdf.Encryption(user="motdepasse", owner="proprietaire", aes=True, R=6))


def pdf20() -> None:
    """Le même contenu, déclaré en PDF 2.0."""
    with pikepdf.open(OUT / "texte_simple.pdf") as pdf:
        save(pdf, "pdf20.pdf", force_version="2.0")


def corrompu() -> None:
    """Fichier tronqué de moitié : doit produire une erreur propre ou être réparé."""
    data = (OUT / "multipage_100.pdf").read_bytes()
    (OUT / "corrompu.pdf").write_bytes(data[: len(data) // 2])



# MARK: - Documents de référence de l'OCR (jalon 5)
#
# Documents natifs dont le texte, connu, sert de référence au banc d'essai de
# la reconnaissance de texte : le banc les rastérise, les dégrade comme un
# scanner (flou, bruit, compression, inclinaison) puis compare la lecture de
# chaque moteur au texte d'origine. Textes originaux, écrits pour ce jeu.

OCR = OUT / "ocr"

OCR_LETTRE = [
    "Madame, Monsieur,",
    "Suite à notre entretien du 14 mars dernier, je vous confirme l'accord "
    "conclu pour la rénovation des locaux situés au deuxième étage. Les travaux "
    "débuteront le lundi 7 avril et s'achèveront au plus tard le 30 mai, sous "
    "réserve des délais de livraison des matériaux.",
    "Le coût total s'élève à 18 450,00 € hors taxes, soit 22 140,00 € TTC. Un "
    "acompte de 30 % sera versé à la signature du devis ; le solde sera réglé "
    "à la réception des travaux, après la levée d'éventuelles réserves.",
    "Je reste à votre disposition pour toute précision complémentaire et vous "
    "prie d'agréer, Madame, Monsieur, l'expression de mes salutations "
    "distinguées.",
]

OCR_ARTICLE = (
    "La numérisation des archives d'une entreprise ne se limite pas à produire "
    "des images : encore faut-il pouvoir y retrouver une information. "
    "Reconnaître le texte d'une page scannée exige d'abord de la redresser, puis "
    "de distinguer le papier de l'encre, enfin de lire chaque ligne. Les "
    "caractères accentués, les ligatures et les chiffres mettent les moteurs à "
    "l'épreuve ; une virgule mal lue dans un montant suffit à fausser un total. "
    "C'est pourquoi chaque mot reçoit une note de confiance, qui signale à "
    "l'utilisateur les passages à vérifier. Un œil exercé repère vite ces "
    "hésitations : « 1 » pris pour « l », « 0 » pour « O », ou « é » privé de "
    "son accent. "
)

OCR_ANGLAIS = [
    "Quarterly Operations Review",
    "Revenue for the third quarter reached $4.2 million, up 12% compared with "
    "the same period last year. Gross margin improved to 41.5%, driven by "
    "higher utilisation of the support team and a lower cost of hardware.",
    "Two new framework agreements were signed in September, with delivery "
    "scheduled between October 15 and December 20. Headcount grew from 58 to "
    "63 employees; attrition remained below 5% on a rolling twelve-month basis.",
    "Key risks include supplier lead times, currency exposure on imported "
    "equipment, and the pending renewal of the main data-centre lease.",
]


def ocr_paragraphs(name: str, title: str, paragraphs, font: str, size: float,
                   heading: str | None = None, ttf: str | None = None) -> None:
    from reportlab.lib.enums import TA_JUSTIFY
    from reportlab.lib.styles import ParagraphStyle
    if ttf:
        from reportlab.pdfbase import pdfmetrics
        from reportlab.pdfbase.ttfonts import TTFont
        pdfmetrics.registerFont(TTFont(font, ttf))
    doc = SimpleDocTemplate(str(OCR / name), pagesize=A4, title=title,
                            author="PDF Studio — jeu de test", invariant=1,
                            leftMargin=2.5 * cm, rightMargin=2.5 * cm)
    body = ParagraphStyle("corps", fontName=font, fontSize=size, leading=size * 1.35,
                          alignment=TA_JUSTIFY, spaceAfter=size * 0.8)
    story = []
    if heading:
        story.append(Paragraph(heading, ParagraphStyle("titre", fontName=font,
                                                       fontSize=size * 1.6,
                                                       leading=size * 2, spaceAfter=size)))
    story += [Paragraph(text, body) for text in paragraphs]
    doc.build(story)


def ocr_lettre() -> None:
    """Lettre en Times 12 pt : scan propre de référence."""
    OCR.mkdir(exist_ok=True)
    c = new_canvas("ocr/lettre.pdf", title="Lettre")
    width, height = A4
    c.setFont("Times-Bold", 12)
    c.drawString(2.5 * cm, height - 2.5 * cm, "Société Durand & Fils")
    c.setFont("Times-Roman", 11)
    for i, line in enumerate(["12, rue des Lilas", "69003 Lyon", "Tél. 04 78 12 34 56"]):
        c.drawString(2.5 * cm, height - 3.1 * cm - i * 0.5 * cm, line)
    c.drawRightString(width - 2.5 * cm, height - 5.5 * cm, "Lyon, le 2 avril 2026")
    c.setFont("Times-Bold", 12)
    c.drawString(2.5 * cm, height - 7 * cm, "Objet : confirmation de commande n° 2026-0412")
    from reportlab.lib.enums import TA_JUSTIFY
    from reportlab.lib.styles import ParagraphStyle
    style = ParagraphStyle("corps", fontName="Times-Roman", fontSize=12, leading=16,
                           alignment=TA_JUSTIFY, spaceAfter=10)
    y = height - 8.2 * cm
    for text in OCR_LETTRE:
        paragraph = Paragraph(text, style)
        _, used = paragraph.wrap(width - 5 * cm, height)
        y -= used
        paragraph.drawOn(c, 2.5 * cm, y)
        y -= 10
    c.drawString(width - 8 * cm, y - 1.5 * cm, "Claire Martin")
    c.drawString(width - 8 * cm, y - 2.1 * cm, "Directrice administrative")
    c.showPage()
    c.save()


def ocr_facture() -> None:
    """Facture : en-tête d'étiquettes et de valeurs, tableau, totaux."""
    OCR.mkdir(exist_ok=True)
    doc = SimpleDocTemplate(str(OCR / "facture.pdf"), pagesize=A4, title="Facture",
                            author="PDF Studio — jeu de test", invariant=1)
    styles = getSampleStyleSheet()
    entete = Table([
        ["Facture n°", "F-2026-0387", "Client", "Atelier Lefèvre"],
        ["Date", "15/03/2026", "Adresse", "8, place de l'Église"],
        ["Échéance", "14/04/2026", "Ville", "33000 Bordeaux"],
    ], colWidths=[3 * cm, 4 * cm, 3 * cm, 6 * cm])
    entete.setStyle(TableStyle([
        ("FONTNAME", (0, 0), (0, -1), "Helvetica-Bold"),
        ("FONTNAME", (2, 0), (2, -1), "Helvetica-Bold"),
        ("FONTSIZE", (0, 0), (-1, -1), 10),
    ]))
    lignes = [["Réf.", "Désignation", "Qté", "P.U. HT", "Montant HT"]]
    articles = [
        ("A-104", "Câble réseau catégorie 6, 3 m", 24, 4.90),
        ("B-221", "Commutateur 24 ports administrable", 2, 389.00),
        ("C-017", "Onduleur 1 500 VA", 1, 612.50),
        ("D-330", "Installation et paramétrage (heure)", 6, 85.00),
        ("E-008", "Déplacement forfaitaire", 1, 45.00),
    ]
    total = 0.0
    for ref, label, qty, price in articles:
        amount = qty * price
        total += amount
        lignes.append([ref, label, str(qty), f"{price:,.2f} €".replace(",", " ").replace(".", ","),
                       f"{amount:,.2f} €".replace(",", " ").replace(".", ",")])
    tva = total * 0.2
    def eur(v):
        return f"{v:,.2f} €".replace(",", " ").replace(".", ",")
    lignes += [["", "", "", "Total HT", eur(total)],
               ["", "", "", "TVA 20 %", eur(tva)],
               ["", "", "", "Total TTC", eur(total + tva)]]
    table = Table(lignes, colWidths=[1.8 * cm, 7.2 * cm, 1.4 * cm, 2.8 * cm, 3 * cm])
    table.setStyle(TableStyle([
        ("GRID", (0, 0), (-1, -4), 0.5, colors.grey),
        ("BACKGROUND", (0, 0), (-1, 0), colors.HexColor("#e8ecf4")),
        ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
        ("FONTNAME", (3, -1), (-1, -1), "Helvetica-Bold"),
        ("ALIGN", (2, 1), (-1, -1), "RIGHT"),
        ("FONTSIZE", (0, 0), (-1, -1), 10),
    ]))
    pied = Paragraph("Paiement par virement à réception de facture. Pénalités de retard : "
                     "trois fois le taux d'intérêt légal. Indemnité forfaitaire pour frais de "
                     "recouvrement : 40 €.", styles["Normal"])
    doc.build([Paragraph("Facture", styles["Title"]), entete, Spacer(1, cm), table,
               Spacer(1, cm), pied])


def ocr_colonnes() -> None:
    """Article sur deux colonnes, Helvetica 10 pt."""
    OCR.mkdir(exist_ok=True)
    from reportlab.lib.enums import TA_JUSTIFY
    from reportlab.lib.styles import ParagraphStyle
    from reportlab.platypus import BaseDocTemplate, Frame, PageTemplate
    width, height = A4
    doc = BaseDocTemplate(str(OCR / "colonnes.pdf"), pagesize=A4, title="Deux colonnes",
                          author="PDF Studio — jeu de test", invariant=1)
    gap, margin = 0.8 * cm, 2 * cm
    column = (width - 2 * margin - gap) / 2
    frames = [Frame(margin, margin, column, height - 2 * margin, id="gauche"),
              Frame(margin + column + gap, margin, column, height - 2 * margin, id="droite")]
    doc.addPageTemplates([PageTemplate(id="deux", frames=frames)])
    body = ParagraphStyle("corps", fontName="Helvetica", fontSize=10, leading=13.5,
                          alignment=TA_JUSTIFY, spaceAfter=8)
    titre = ParagraphStyle("titre", fontName="Helvetica-Bold", fontSize=14, leading=18,
                           spaceAfter=10)
    story = [Paragraph("Archives : du papier au texte", titre)]
    for i in range(5):
        story.append(Paragraph(f"{i + 1}. " + OCR_ARTICLE, body))
    doc.build(story[:6])


def ocr_tableau_dense() -> None:
    """Tableau de dates, références et montants en petits caractères."""
    OCR.mkdir(exist_ok=True)
    rng = random.Random(SEED + 5)
    doc = SimpleDocTemplate(str(OCR / "tableau_dense.pdf"), pagesize=A4, title="Relevé",
                            author="PDF Studio — jeu de test", invariant=1)
    styles = getSampleStyleSheet()
    libelles = ["Prélèvement assurance", "Virement reçu", "Carte bancaire", "Frais de tenue",
                "Remise de chèque", "Échéance de prêt", "Retrait espèces", "Abonnement télécom"]
    rows = [["Date", "Opération", "Référence", "Débit", "Crédit"]]
    for i in range(28):
        day = 1 + i
        amount = rng.randrange(500, 250000) / 100
        label = libelles[rng.randrange(len(libelles))]
        ref = f"{rng.randrange(10**7, 10**8)}"
        debit, credit = (f"{amount:,.2f}".replace(",", " ").replace(".", ","), "")
        if label.startswith(("Virement", "Remise")):
            debit, credit = credit, debit
        rows.append([f"{day:02d}/02/2026", label, ref, debit, credit])
    table = Table(rows, colWidths=[2.2 * cm, 5.8 * cm, 2.8 * cm, 2.6 * cm, 2.6 * cm])
    table.setStyle(TableStyle([
        ("GRID", (0, 0), (-1, -1), 0.4, colors.grey),
        ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
        ("FONTSIZE", (0, 0), (-1, -1), 8.5),
        ("ALIGN", (3, 1), (-1, -1), "RIGHT"),
        ("ROWBACKGROUNDS", (0, 1), (-1, -1), [colors.white, colors.HexColor("#f2f2f2")]),
    ]))
    doc.build([Paragraph("Relevé de compte — février 2026", styles["Heading2"]), table])


def ocr_petits_caracteres() -> None:
    """Conditions générales en 8 pt, police TrueType intégrée (DejaVu Sans)."""
    texts = [
        f"Article {n}. " + text for n, text in enumerate([
            "Les présentes conditions s'appliquent à toute commande passée auprès du "
            "prestataire, quel qu'en soit le montant, sauf accord écrit contraire.",
            "Les prix s'entendent hors taxes, en euros, frais de port non compris ; ils "
            "peuvent être révisés au 1er janvier de chaque année, dans la limite de 4 %.",
            "Le délai de paiement est fixé à trente jours fin de mois à compter de la date "
            "d'émission de la facture. Aucun escompte n'est accordé pour paiement anticipé.",
            "La garantie couvre les défauts de fabrication pendant vingt-quatre mois ; elle "
            "exclut l'usure normale, les chocs et toute intervention d'un tiers non agréé.",
            "Tout litige relève de la compétence exclusive du tribunal de commerce du siège "
            "du prestataire, même en cas d'appel en garantie ou de pluralité de défendeurs.",
            "Conformément au règlement général sur la protection des données, le client "
            "dispose d'un droit d'accès, de rectification et d'effacement de ses données.",
        ] * 2, start=1)
    ]
    ocr_paragraphs("petits_caracteres.pdf", "Conditions générales", texts, "DejaVuSans", 8,
                   heading="Conditions générales de vente",
                   ttf="/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")


def ocr_anglais() -> None:
    """Texte anglais en Helvetica 11 pt."""
    ocr_paragraphs("anglais.pdf", "Quarterly review", OCR_ANGLAIS[1:], "Helvetica", 11,
                   heading=OCR_ANGLAIS[0])


def ocr_documents() -> None:
    OCR.mkdir(exist_ok=True)
    ocr_lettre()
    ocr_facture()
    ocr_colonnes()
    ocr_tableau_dense()
    ocr_petits_caracteres()
    ocr_anglais()


def pas_un_pdf() -> None:
    (OUT / "pas_un_pdf.pdf").write_text("Ceci est un fichier texte, pas un PDF.\n", encoding="utf-8")


def main() -> None:
    texte_simple()
    multipage("multipage_100.pdf", 100, "Cent pages")
    multipage("volumineux_600.pdf", 600, "Six cents pages")
    signets()
    formats_mixtes()
    vectoriel()
    tableau()
    scan_simule()
    chiffre()
    pdf20()
    organisation()
    edition()
    police_cff()
    encadre()
    signature_factice()
    corrompu()
    pas_un_pdf()
    ocr_documents()
    for f in sorted(OUT.glob("*.pdf")):
        print(f"{f.name:24} {f.stat().st_size / 1024:8.1f} Kio")


if __name__ == "__main__":
    main()
