#!/usr/bin/env python3
"""Génère le jeu de PDF de test publics de PDF Studio.

Usage : python3 tests/fixtures/generate.py [amelioration]
(avec « amelioration », seul le jeu du mode Amélioration est régénéré).
Dépendances : reportlab, pikepdf, pillow, fonttools
(pip install reportlab pikepdf pillow fonttools).

Les fichiers produits sont déterministes (dates et identifiants figés) pour que
leur régénération ne modifie pas le dépôt, sauf chiffre.pdf (cf. chiffre()). Les documents personnels de
l'utilisateur ne sont jamais générés ici : ils vivent dans tests/fixtures/private/,
exclu du dépôt.
"""

from __future__ import annotations

import io
import math
import random
import sys
from pathlib import Path

import pikepdf
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont, ImageOps
from reportlab.lib import colors
from reportlab.lib.pagesizes import A4, A5, landscape, letter
from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
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


def tableaux() -> None:
    """Tableaux à éditer : grille avec cellules fusionnées, cellules vides et
    fonds de cellule ; tableau à filets horizontaux seuls (sans filet vertical)."""
    doc = SimpleDocTemplate(str(OUT / "tableaux.pdf"), pagesize=A4, title="Tableaux",
                            author="PDF Studio — jeu de test", invariant=1)
    styles = getSampleStyleSheet()
    grid = [
        ["Trimestre", "Ventes", "", "Charges", ""],
        ["", "France", "Export", "Fixes", "Variables"],
        ["T1", "120", "45", "80", "30"],
        ["T2", "135", "", "80", "34"],
        ["T3", "", "52", "82", ""],
        ["T4", "160", "61", "85", "41"],
    ]
    first = Table(grid, colWidths=[3 * cm, 2.6 * cm, 2.6 * cm, 2.6 * cm, 2.6 * cm])
    first.setStyle(TableStyle([
        ("GRID", (0, 0), (-1, -1), 0.5, colors.black),
        ("SPAN", (0, 0), (0, 1)),
        ("SPAN", (1, 0), (2, 0)),
        ("SPAN", (3, 0), (4, 0)),
        ("BACKGROUND", (0, 0), (-1, 1), colors.HexColor("#dde4f0")),
        ("BACKGROUND", (0, 5), (-1, 5), colors.HexColor("#f4f4f4")),
        ("FONTNAME", (0, 0), (-1, 1), "Helvetica-Bold"),
        ("ALIGN", (1, 2), (-1, -1), "RIGHT"),
        ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
    ]))
    lined = [
        ["Ville", "Habitants", "Superficie"],
        ["Lyon", "522 000", "47,9 km²"],
        ["Nantes", "320 000", "65,2 km²"],
        ["Rennes", "222 000", "50,4 km²"],
    ]
    second = Table(lined, colWidths=[4 * cm, 3.5 * cm, 3.5 * cm])
    second.setStyle(TableStyle([
        ("LINEABOVE", (0, 0), (-1, 0), 1.2, colors.black),
        ("LINEBELOW", (0, 0), (-1, 0), 0.6, colors.black),
        ("LINEBELOW", (0, 1), (-1, 2), 0.3, colors.grey),
        ("LINEBELOW", (0, -1), (-1, -1), 1.2, colors.black),
        ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
        ("ALIGN", (1, 1), (-1, -1), "RIGHT"),
    ]))
    doc.build([
        Paragraph("Résultats trimestriels", styles["Title"]),
        Spacer(1, cm),
        first,
        Spacer(1, 1.5 * cm),
        Paragraph("Villes", styles["Heading2"]),
        second,
        Spacer(1, cm),
        Paragraph("Source : données fictives pour les tests.", styles["Normal"]),
    ])


def tableau_styles() -> None:
    """Tableau à filets dont certaines cellules mêlent des textes de corps et
    de graisse différents (titre en gras, description et référence en corps
    plus petit, comme un devis détaillé) : chacun forme un bloc de texte."""
    doc = SimpleDocTemplate(str(OUT / "tableau_styles.pdf"), pagesize=A4, title="Devis détaillé",
                            author="PDF Studio — jeu de test", invariant=1)
    styles = getSampleStyleSheet()
    title = ParagraphStyle("titre", parent=styles["Normal"], fontName="Helvetica-Bold", fontSize=10, leading=12)
    small = ParagraphStyle("detail", parent=styles["Normal"], fontName="Helvetica", fontSize=7, leading=9)
    rows = [["Désignation", "Quantité", "Total"]]
    for i, (name, detail) in enumerate([
        ("Audit initial", "Analyse de l'existant et rapport de synthèse remis sous huit jours."),
        ("Mise en place", "Installation, paramétrage et recette avec le client sur site."),
    ], start=1):
        rows.append([
            [Paragraph(name, title), Paragraph(detail, small), Paragraph(f"Réf. DV-{i:03d}", small)],
            str(i),
            f"{i * 450:.2f} €",
        ])
    rows.append(["Option seule", "1", "90,00 €"])
    table = Table(rows, colWidths=[8 * cm, 3 * cm, 3 * cm])
    table.setStyle(TableStyle([
        ("GRID", (0, 0), (-1, -1), 0.5, colors.black),
        ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("ALIGN", (1, 1), (-1, -1), "RIGHT"),
    ]))
    doc.build([Paragraph("Devis détaillé", styles["Title"]), Spacer(1, cm), table])


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


def formulaire() -> None:
    """Contenu logé dans des formulaires PDF (objets externes /Form), comme
    dans les PDF exportés ou imprimés par macOS : page 1, tout le texte dans
    un formulaire à groupe de transparence, dont un bloc dans un formulaire
    imbriqué ; les deux formulaires et la page nomment G1 trois polices
    différentes ; un pied de page reste hors formulaire. Page 2 : un
    formulaire sans texte (logo), qui doit rester entier."""
    pdf = pikepdf.new()

    def font(base: str) -> pikepdf.Object:
        return pdf.make_indirect(pikepdf.Dictionary(
            Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1,
            BaseFont=pikepdf.Name("/" + base), Encoding=pikepdf.Name.WinAnsiEncoding,
        ))

    def show(text: str) -> bytes:
        escaped = text.encode("cp1252").replace(b"\\", b"\\\\")
        return b"(" + escaped.replace(b"(", b"\\(").replace(b")", b"\\)") + b") Tj "

    def form(content: bytes, bbox, resources: pikepdf.Dictionary, **extra) -> pikepdf.Object:
        stream = pdf.make_stream(content)
        stream.Type = pikepdf.Name.XObject
        stream.Subtype = pikepdf.Name.Form
        stream.BBox = pikepdf.Array(bbox)
        stream.Resources = resources
        for key, value in extra.items():
            stream[pikepdf.Name("/" + key)] = value
        return stream

    nested = form(
        b"BT /G1 10 Tf 1 0 0 1 4 16 Tm " + show("Bloc dans un formulaire imbriqué") + b"ET\n",
        [0, 0, 300, 40],
        pikepdf.Dictionary(Font=pikepdf.Dictionary(G1=font("Courier"))),
    )
    main = form(
        b"q 0 g BT /G2 18 Tf 1 0 0 1 56 760 Tm " + show("Devis fictif n° 0042") + b"ET Q\n"
        + b"0.2 0.2 0.2 rg 56 748 483 0.6 re f\n"
        + b"0 g BT /G1 11 Tf 14 TL 1 0 0 1 56 720 Tm "
        + show("Ce paragraphe est logé dans un formulaire, comme le fait macOS")
        + b"T* " + show("quand il enregistre un document au format PDF : chaque page")
        + b"T* " + show("n'est alors qu'un objet externe dessiné par l'opérateur Do.")
        + b"ET\n"
        + b"q 1 0 0 1 56 600 cm /Fm2 Do Q\n",
        [30, 30, 565, 812],
        pikepdf.Dictionary(
            Font=pikepdf.Dictionary(G1=font("Helvetica"), G2=font("Helvetica-Bold")),
            XObject=pikepdf.Dictionary(Fm2=nested),
        ),
        Group=pikepdf.Dictionary(S=pikepdf.Name.Transparency, I=True, K=False),
    )
    logo = form(b"0.1 0.3 0.6 rg 0 0 100 60 re f\n", [0, 0, 100, 60], pikepdf.Dictionary())

    pdf.add_blank_page(page_size=(595, 842))
    first = pdf.pages[0]
    first.obj.Resources = pikepdf.Dictionary(
        Font=pikepdf.Dictionary(G1=font("Times-Roman")),
        XObject=pikepdf.Dictionary(Fm1=main),
    )
    first.obj.Contents = pdf.make_stream(
        b"q Q q 0 0 595 842 re W n /Fm1 Do Q\n"
        + b"BT /G1 8 Tf 1 0 0 1 56 40 Tm " + show("Pied de page hors formulaire") + b"ET\n"
    )
    pdf.add_blank_page(page_size=(595, 842))
    second = pdf.pages[1]
    second.obj.Resources = pikepdf.Dictionary(
        Font=pikepdf.Dictionary(F1=font("Helvetica")),
        XObject=pikepdf.Dictionary(Logo=logo),
    )
    second.obj.Contents = pdf.make_stream(
        b"BT /F1 12 Tf 1 0 0 1 56 760 Tm " + show("Page avec un logo") + b"ET\n"
        + b"q 1 0 0 1 400 700 cm /Logo Do Q\n"
    )
    save(pdf, "formulaire.pdf")


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


# MARK: - Retouche d'un scan (jalon 7)

FONTS = OUT.parent.parent / "app" / "Resources" / "Fonts"


def scan_retouche() -> None:
    """Facture scannée en couleur à 200 ppp : texte Caladea et Carlito, tableau
    à filets, logo, ligne de signature ; légèrement inclinée, floue et bruitée.
    Écrit aussi scan_retouche.mots.tsv : les mots et leur emprise dans l'espace
    de la page (ce qu'aurait lu la reconnaissance), pour les tests sans Vision."""
    rng = random.Random(SEED + 7)
    dpi = 200
    w, h = int(8.27 * dpi), int(11.69 * dpi)
    img = Image.new("RGB", (w, h), (242, 239, 230))
    draw = ImageDraw.Draw(img)
    serif = ImageFont.truetype(str(FONTS / "Caladea-Regular.ttf"), round(11 * dpi / 72))
    sans_bold = ImageFont.truetype(str(FONTS / "Carlito-Bold.ttf"), round(16 * dpi / 72))
    sans = ImageFont.truetype(str(FONTS / "Carlito-Regular.ttf"), round(10 * dpi / 72))
    ink = (28, 30, 48)
    words = []  # (texte, ligne, x, y_base, police)
    line_number = 0

    def write(x, y, text, font, fill=ink):
        nonlocal line_number
        cursor = x
        for word in text.split(" "):
            draw.text((cursor, y), word, fill=fill, font=font, anchor="ls")
            words.append((word, line_number, cursor, y, font))
            cursor += font.getlength(word + " ")
        line_number += 1

    write(160, 260, "Facture n° 2026-0412", sans_bold)
    write(160, 360, "Montant total : 1 250,00 € TTC", serif)
    write(160, 420, "Échéance : 30 avril 2026", serif)
    write(160, 480, "Référence client : DUR-0412", serif)
    # Tableau 3 × 3 à filets.
    x0, y0, cw, rh = 160, 600, 360, 70
    for i in range(4):
        draw.line([(x0, y0 + i * rh), (x0 + 3 * cw, y0 + i * rh)], fill=ink, width=3)
        draw.line([(x0 + i * cw, y0), (x0 + i * cw, y0 + 3 * rh)], fill=ink, width=3)
    cells = [["Désignation", "Quantité", "Prix"], ["Audit", "2", "800,00"], ["Formation", "1", "450,00"]]
    for r, row in enumerate(cells):
        for c, text in enumerate(row):
            write(x0 + c * cw + 20, y0 + r * rh + 46, text, sans)
    # Logo : disque rouge et carré bleu.
    draw.ellipse([1180, 150, 1300, 270], fill=(200, 40, 40))
    draw.rectangle([1320, 170, 1420, 270], fill=(40, 60, 170))
    # Ligne de signature.
    draw.line([(900, 1500), (1400, 1500)], fill=ink, width=3)
    write(160, 1100, "Merci de votre confiance.", serif)

    angle = 0.4
    img = img.rotate(angle, resample=Image.BICUBIC, fillcolor=(242, 239, 230))
    img = img.filter(ImageFilter.GaussianBlur(0.7))
    pixels = img.load()
    for _ in range(3000):
        x, y = rng.randrange(w), rng.randrange(h)
        v = rng.randrange(-25, 25)
        r, g, b = pixels[x, y]
        pixels[x, y] = (max(0, min(255, r + v)), max(0, min(255, g + v)), max(0, min(255, b + v)))
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=85)
    buf.seek(0)

    # Emprise des mots, tournée comme l'image (sens trigonométrique autour du
    # centre), puis dans l'espace de la page (points, ordonnées vers le haut).
    import math
    theta = math.radians(angle)
    cx, cy = w / 2, h / 2
    scale = 72 / dpi
    page_h = A4[1]

    def to_page(x, y):
        dx, dy = x - cx, y - cy
        rx = cx + dx * math.cos(theta) + dy * math.sin(theta)
        ry = cy - dx * math.sin(theta) + dy * math.cos(theta)
        return rx * scale * (A4[0] / (w * scale)), page_h - ry * scale * (page_h / (h * scale))

    rows = []
    for word, line, x, y, font in words:
        ascent, descent = font.getmetrics()
        right = x + font.getlength(word)
        corners = [(x, y + descent), (right, y + descent), (right, y - ascent), (x, y - ascent)]
        quad = [v for corner in corners for v in to_page(*corner)]
        rows.append("\t".join([word, str(line)] + [f"{v:.2f}" for v in quad]))
    (OUT / "scan_retouche.mots.tsv").write_text("\n".join(rows) + "\n", encoding="utf-8")

    c = new_canvas("scan_retouche.pdf", title="Scan à retoucher")
    from reportlab.lib.utils import ImageReader
    c.drawImage(ImageReader(buf), 0, 0, width=A4[0], height=A4[1])
    c.showPage()
    c.save()


# MARK: - Mode Amélioration : scans à redresser et à nettoyer
#
# Chaque page est d'abord dessinée « droite » et propre, à la résolution du
# scan, en relevant tout ce qu'elle contient (la vérité terrain) ; elle est
# ensuite dégradée comme par un scanner ou un téléphone : papier teinté,
# éclairage inégal, inclinaison, page retournée, perspective, flou, bruit,
# compression JPEG. Le fichier .json de même nom décrit la page droite et la
# transformation subie. Conventions : rectangles [x0, y0, x1, y1] en points
# sur la page droite, origine en haut à gauche ; inclinaison en degrés,
# positive quand les lignes descendent vers la droite dans l'image (comme
# ocr::preprocess::estimate_skew) ; quarts de tour dans le sens horaire.
# Textes originaux, écrits pour ce jeu.

AMELIORATION = OUT / "amelioration"

AM_NOTE = [
    "Afin d'assurer la continuité du service pendant la période estivale, chaque "
    "équipe établira son planning de congés avant le 15 mai. Les demandes seront "
    "validées par le responsable de service dans un délai de dix jours ouvrés.",
    "Une permanence minimale de deux personnes est requise du 14 juillet au 25 août. "
    "Les collaborateurs qui souhaitent fractionner leurs congés sont invités à le "
    "signaler dès que possible, afin de faciliter les arbitrages.",
    "Les jours de réduction du temps de travail non pris au 31 décembre seront "
    "perdus, sauf accord écrit de la direction des ressources humaines.",
]

AM_COMITE = [
    "Le comité de pilotage s'est réuni le 9 septembre en présence des responsables "
    "des trois sites. L'avancement global du programme atteint 64 %, contre 58 % "
    "prévus au planning de référence.",
    "La migration des postes de travail du site de Nantes est terminée ; celle de "
    "Lille débutera le 6 octobre. Le budget engagé s'élève à 412 300 €, soit 47 % "
    "de l'enveloppe votée.",
    "Deux risques sont suivis : le délai de livraison des serveurs et la "
    "disponibilité des équipes pendant la clôture comptable de fin d'année.",
]

AM_PARC = [
    "Au cœur du massif, le sentier des crêtes offre une boucle de 14 km accessible "
    "d'avril à novembre. Comptez cinq heures de marche et 620 m de dénivelé.",
    "Les gardes du parc proposent chaque samedi une sortie commentée sur la faune "
    "d'altitude : chamois, marmottes et gypaètes barbus y sont régulièrement observés.",
    "Le refuge de la Combe accueille les randonneurs de juin à septembre ; la "
    "réservation est obligatoire pour la nuitée et le repas du soir.",
]

AM_STATUS = [
    "The network upgrade programme remains on schedule for completion in the first "
    "quarter. Fibre links between the three main buildings were commissioned on "
    "October 2, and the legacy switches will be retired after a two-week overlap.",
    "Spending to date is $286,400 against a budget of $540,000. The only significant "
    "variance concerns cabling labour, which ran 9% above estimate because of "
    "asbestos checks in the older wing.",
    "Next steps: migrate the wireless controllers, update the disaster recovery plan, "
    "and run a full failover test before the end of November.",
]

AM_MEMO = [
    "Please note that the visitor badge system will be replaced over the weekend of "
    "March 14. From Monday, all guests must be registered online at least one day "
    "before their visit.",
    "Reception staff will print badges on arrival and collect them at departure. "
    "Contractors working on site for more than a week will receive a temporary "
    "access card valid for thirty days.",
    "Questions about the new procedure can be sent to the facilities team, who will "
    "also run two short training sessions for assistants on March 12 and 13.",
]


def am_round(values, digits=2):
    return [round(float(v), digits) for v in values]


def am_clamp(value) -> int:
    return max(0, min(255, int(round(value))))


def am_smooth_noise(w: int, h: int, cell: int, rng: random.Random) -> Image.Image:
    """Bruit lisse : valeurs uniformes sur une grille de `cell` pixels, interpolées."""
    cell = max(1, cell)
    gw, gh = max(2, w // cell + 2), max(2, h // cell + 2)
    small = Image.frombytes("L", (gw, gh), rng.randbytes(gw * gh))
    return small.resize((w, h), Image.BICUBIC)


def am_gaussian_noise(size, sigma: float, rng: random.Random) -> Image.Image:
    """Bruit gaussien d'écart type `sigma`, centré sur 128 (déterministe)."""
    from statistics import NormalDist
    normal = NormalDist()
    lut = [am_clamp(128 + sigma * normal.inv_cdf((u + 0.5) / 256)) for u in range(256)]
    return Image.frombytes("L", size, rng.randbytes(size[0] * size[1])).point(lut)


def am_mottle(w: int, h: int, dpi: int, rng: random.Random) -> Image.Image:
    """Nuances d'un aplat imprimé puis scanné : taches claires et sombres de
    tailles variées (128 = couleur de base). Une zone morte laisse environ un
    tiers des pixels à la couleur de base, qui reste la plus présente."""
    coarse = am_smooth_noise(w, h, int(dpi * 0.35), rng)
    fine = am_smooth_noise(w, h, int(dpi * 0.12), rng)
    noise = Image.blend(coarse, fine, 0.35)

    def shape(v):
        d = v - 128
        if abs(d) <= 16:
            return 128
        return am_clamp(128 + (d - math.copysign(16, d)) * 2.4)

    return noise.point([shape(v) for v in range(256)])


def am_photo(w: int, h: int, rng: random.Random) -> Image.Image:
    """Photographie de paysage synthétique : ciel en dégradé, soleil, deux
    chaînes de montagnes texturées, prairie ; texture fine partout."""
    sky = ImageOps.colorize(Image.linear_gradient("L").resize((w, h)), (64, 112, 186), (214, 226, 236))
    img = sky.convert("RGB")
    sun = Image.new("L", (w, h), 0)
    ImageDraw.Draw(sun).ellipse([w * 0.68, h * 0.12, w * 0.8, h * 0.12 + w * 0.12], fill=255)
    sun = sun.filter(ImageFilter.GaussianBlur(w * 0.01))
    img.paste((255, 244, 206), (0, 0), sun)

    def ridge(base: float, roughness: float):
        n = 64
        heights = [0.0] * (n + 1)
        step, amplitude = n, roughness
        heights[0], heights[n] = rng.uniform(-1, 1) * roughness, rng.uniform(-1, 1) * roughness
        while step > 1:
            half = step // 2
            for i in range(half, n, step):
                heights[i] = (heights[i - half] + heights[i + half]) / 2 + rng.uniform(-1, 1) * amplitude
            step, amplitude = half, amplitude * 0.55
        return [(w * i / n, h * (base - heights[i])) for i in range(n + 1)] + [(w, h), (0, h)]

    def texture(c1, c2, cell):
        noise = Image.blend(am_smooth_noise(w, h, cell, rng), am_smooth_noise(w, h, max(2, cell // 6), rng), 0.4)
        return ImageOps.colorize(ImageOps.autocontrast(noise), c1, c2)

    for base, rough, c1, c2, cell in [
        (0.48, 0.16, (92, 104, 132), (150, 160, 182), w // 10),
        (0.62, 0.12, (46, 70, 52), (104, 124, 88), w // 14),
        (0.8, 0.03, (70, 112, 44), (150, 176, 82), w // 20),
    ]:
        mask = Image.new("L", (w, h), 0)
        ImageDraw.Draw(mask).polygon(ridge(base, rough), fill=255)
        img.paste(texture(c1, c2, cell), (0, 0), mask)
    grain = am_gaussian_noise((w, h), 10, rng)
    bands = [ImageChops.add(band, grain, 1.0, -128) for band in img.split()]
    return Image.merge("RGB", bands).filter(ImageFilter.GaussianBlur(0.6))


class AmPage:
    """Page droite dessinée à `dpi` points par pouce, et relevé de son contenu."""

    def __init__(self, dpi: int, size=A4, seed: int = 0):
        self.dpi, self.size = dpi, size
        self.w, self.h = round(size[0] * dpi / 72), round(size[1] * dpi / 72)
        self.img = Image.new("RGB", (self.w, self.h), (255, 255, 255))
        self.draw = ImageDraw.Draw(self.img)
        self.rng = random.Random(seed)
        self.lines, self.flats, self.photos, self.gradients, self.rules = [], [], [], [], []
        self.occupied = []

    def px(self, value: float) -> float:
        return value * self.dpi / 72

    def pt(self, value: float) -> float:
        return value * 72 / self.dpi

    def box(self, rect):
        return [round(self.px(v)) for v in rect]

    def font(self, name: str, size: float) -> ImageFont.FreeTypeFont:
        return ImageFont.truetype(str(FONTS / f"{name}.ttf"), max(1, round(self.px(size))))

    def text(self, x, baseline, text, font="Carlito-Regular", size=11.0, fill=(28, 28, 34),
             on="papier", align="left"):
        face = self.font(font, size)
        X, Y = self.px(x), self.px(baseline)
        length = face.getlength(text)
        if align == "right":
            X -= length
        elif align == "center":
            X -= length / 2
        self.draw.text((X, Y), text, fill=fill, font=face, anchor="ls")
        ascent, descent = face.getmetrics()
        rect = [self.pt(X), self.pt(Y - ascent), self.pt(X + length), self.pt(Y + descent)]
        self.lines.append({"texte": text, "rect": am_round(rect), "corps_pt": size, "police": font,
                           "couleur": list(fill), "fond": on})
        self.occupied.append(rect)
        return rect

    def paragraph(self, x, baseline, width, text, font="Caladea-Regular", size=11.0,
                  leading=None, fill=(28, 28, 34), on="papier") -> float:
        """Texte coupé en lignes de `width` points au plus ; rend la ligne de
        base suivant le paragraphe."""
        leading = leading or size * 1.32
        face = self.font(font, size)
        line = []
        for word in text.split():
            if line and face.getlength(" ".join(line + [word])) > self.px(width):
                self.text(x, baseline, " ".join(line), font, size, fill, on)
                baseline += leading
                line = []
            line.append(word)
        if line:
            self.text(x, baseline, " ".join(line), font, size, fill, on)
            baseline += leading
        return baseline

    def flat(self, ident, rect, color, nuances=24, border=None):
        """Aplat imprimé d'une couleur unie, perturbé de nuances (taches)."""
        x0, y0, x1, y1 = self.box(rect)
        delta = am_mottle(x1 - x0, y1 - y0, self.dpi, self.rng)
        amplitude = [nuances * 0.8, nuances * 0.9, nuances * 1.1]
        bands = [delta.point([am_clamp(c + (v - 128) * a / 127) for v in range(256)])
                 for c, a in zip(color, amplitude)]
        self.img.paste(Image.merge("RGB", bands), (x0, y0))
        if border:
            self.frame(rect, *border)
        self.flats.append({"id": ident, "rect": am_round(rect), "couleur": list(color), "nuances": nuances})
        self.occupied.append(list(rect))

    def rule(self, x0, y0, x1, y1, width=0.5, color=(40, 40, 40)):
        """Filet horizontal ou vertical d'épaisseur `width` points, éventuellement
        inférieure au pixel (couverture partielle, comme un vrai rendu)."""
        thickness = self.px(width)
        horizontal = abs(y1 - y0) < abs(x1 - x0)
        full, part = int(thickness), thickness - int(thickness)
        if horizontal:
            Y = self.px(y0) - thickness / 2
            X0, X1 = round(self.px(x0)), round(self.px(x1))
            start = round(Y)
            for i in range(full):
                self.img.paste(color, (X0, start + i, X1, start + i + 1))
            if part > 0.05:
                mask = Image.new("L", (X1 - X0, 1), round(255 * part))
                self.img.paste(color, (X0, start + full), mask)
            rect = [x0, y0 - width / 2, x1, y0 + width / 2]
        else:
            X = self.px(x0) - thickness / 2
            Y0, Y1 = round(self.px(y0)), round(self.px(y1))
            start = round(X)
            for i in range(full):
                self.img.paste(color, (start + i, Y0, start + i + 1, Y1))
            if part > 0.05:
                mask = Image.new("L", (1, Y1 - Y0), round(255 * part))
                self.img.paste(color, (start + full, Y0), mask)
            rect = [x0 - width / 2, y0, x0 + width / 2, y1]
        self.rules.append({"rect": am_round(rect), "epaisseur_pt": width, "couleur": list(color)})
        self.occupied.append(rect)

    def frame(self, rect, width, color):
        x0, y0, x1, y1 = rect
        self.rule(x0, y0, x1, y0, width, color)
        self.rule(x0, y1, x1, y1, width, color)
        self.rule(x0, y0, x0, y1, width, color)
        self.rule(x1, y0, x1, y1, width, color)

    def photo(self, rect):
        x0, y0, x1, y1 = self.box(rect)
        self.img.paste(am_photo(x1 - x0, y1 - y0, self.rng), (x0, y0))
        self.photos.append({"rect": am_round(rect)})
        self.occupied.append(list(rect))

    def gradient(self, rect, start, end):
        """Dégradé volontaire, de gauche à droite."""
        x0, y0, x1, y1 = self.box(rect)
        ramp = Image.linear_gradient("L").rotate(90, expand=True)
        ramp = ramp.resize((x1 - x0, y1 - y0), Image.BICUBIC)
        self.img.paste(ImageOps.colorize(ramp, start, end), (x0, y0))
        self.gradients.append({"rect": am_round(rect), "de": list(start), "a": list(end)})
        self.occupied.append(list(rect))


def am_light(w: int, h: int, factor) -> Image.Image:
    """Carte d'éclairage (255 = pleine lumière) : `factor(u, v)` pour u, v de 0 à 1."""
    gw, gh = max(2, w // 8), max(2, h // 8)
    values = [am_clamp(255 * factor(x / (gw - 1), y / (gh - 1))) for y in range(gh) for x in range(gw)]
    small = Image.new("L", (gw, gh))
    small.putdata(values)
    return small.resize((w, h), Image.BICUBIC)


def am_multiply(img: Image.Image, light: Image.Image) -> Image.Image:
    return ImageChops.multiply(img, Image.merge("RGB", (light, light, light)))


class AmScan:
    """Dégradation d'une page droite comme par un scanner, et transformation
    qui envoie un point de la page droite (pixels) dans l'image scannée."""

    def __init__(self, page: AmPage, *, paper=(255, 255, 255), lid=(232, 232, 230),
                 light=None, light_note="uniforme", angle=0.0, quarter=0, expand=False,
                 blur=0.7, noise=4.0, chroma=1.5, jpeg=85, gray=False, verso=None,
                 show_through=0.0, seed=0):
        rng = random.Random(seed)
        self.page = page
        self.meta = {"papier": list(paper), "couvercle": list(lid), "eclairage": light_note,
                     "inclinaison_deg": angle, "quart_de_tour": quarter, "perspective": False,
                     "courbure_px": 0, "flou_px": blur,
                     "bruit": noise, "jpeg": jpeg, "niveaux_de_gris": gray,
                     "transparence_verso": show_through}
        img = page.img
        if verso is not None and show_through > 0:
            back = verso.img.transpose(Image.FLIP_LEFT_RIGHT).filter(ImageFilter.GaussianBlur(page.px(1.2)))
            back = Image.blend(Image.new("RGB", img.size, (255, 255, 255)), back, show_through)
            img = ImageChops.multiply(img, back)
        img = ImageChops.multiply(img, Image.new("RGB", img.size, paper))
        w, h = img.size
        transpose = {1: Image.ROTATE_270, 2: Image.ROTATE_180, 3: Image.ROTATE_90}
        if quarter:
            img = img.transpose(transpose[quarter])
        qw, qh = img.size
        if angle:
            img = img.rotate(-angle, resample=Image.BICUBIC, expand=expand, fillcolor=lid)
        fw, fh = img.size
        theta = math.radians(angle)

        def forward(x, y):
            # Quarts de tour (sens horaire, ordonnées vers le bas).
            if quarter == 1:
                x, y = h - y, x
            elif quarter == 2:
                x, y = w - x, h - y
            elif quarter == 3:
                x, y = y, w - x
            # Inclinaison autour du centre (sens horaire pour un angle positif).
            dx, dy = x - qw / 2, y - qh / 2
            return (fw / 2 + dx * math.cos(theta) - dy * math.sin(theta),
                    fh / 2 + dx * math.sin(theta) + dy * math.cos(theta))

        self.forward = forward
        if light is not None:
            img = am_multiply(img, am_light(fw, fh, light))
        if blur:
            img = img.filter(ImageFilter.GaussianBlur(blur))
        if gray:
            img = img.convert("L")
            img = ImageChops.add(img, am_gaussian_noise(img.size, noise, rng), 1.0, -128)
        else:
            shared = am_gaussian_noise(img.size, noise, rng)
            bands = []
            for band in img.split():
                band = ImageChops.add(band, shared, 1.0, -128)
                if chroma:
                    band = ImageChops.add(band, am_gaussian_noise(img.size, chroma, rng), 1.0, -128)
                bands.append(band)
            img = Image.merge("RGB", bands)
        buffer = io.BytesIO()
        img.save(buffer, format="JPEG", quality=jpeg, optimize=True)
        self.jpeg = buffer.getvalue()
        self.image = Image.open(io.BytesIO(self.jpeg))
        self.image.load()

    def corners(self):
        p = self.page
        return [am_round(self.forward(x, y), 1) for x, y in [(0, 0), (p.w, 0), (p.w, p.h), (0, p.h)]]

    def inside(self, rect, margin=3) -> bool:
        x0, y0, x1, y1 = [self.page.px(v) for v in rect]
        fw, fh = self.image.size
        return all(margin <= X <= fw - margin and margin <= Y <= fh - margin
                   for X, Y in (self.forward(x, y) for x, y in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]))

    def dominant(self, rect, exclude, inset=4.0):
        """Couleur la plus présente de la zone dans l'image scannée (pic d'un
        histogramme en cases de 6 niveaux, puis moyenne autour du pic), hors
        des lignes de texte posées dessus."""
        x0, y0, x1, y1 = rect[0] + inset, rect[1] + inset, rect[2] - inset, rect[3] - inset
        rng = random.Random(1)
        image = self.image.convert("RGB")
        samples = []
        for _ in range(6000):
            x, y = rng.uniform(x0, x1), rng.uniform(y0, y1)
            if any(e[0] - 2 <= x <= e[2] + 2 and e[1] - 2 <= y <= e[3] + 2 for e in exclude):
                continue
            X, Y = self.forward(self.page.px(x), self.page.px(y))
            if 0 <= X < image.width and 0 <= Y < image.height:
                samples.append(image.getpixel((int(X), int(Y))))
        bins = {}
        for s in samples:
            key = (s[0] // 6, s[1] // 6, s[2] // 6)
            bins[key] = bins.get(key, 0) + 1
        peak = max(bins, key=lambda k: sum(bins.get((k[0] + a, k[1] + b, k[2] + c), 0)
                                           for a in (-1, 0, 1) for b in (-1, 0, 1) for c in (-1, 0, 1)))
        near = [s for s in samples if all(abs(s[i] // 6 - peak[i]) <= 1 for i in range(3))]
        return [round(sum(s[i] for s in near) / len(near)) for i in range(3)]

    def blank_zones(self, count=10, size=40.0):
        """Carrés de papier vierge de la page droite, visibles dans le scan."""
        p = self.page
        pw, ph = p.size
        candidates = []
        y = 24.0
        while y + size <= ph - 24:
            x = 24.0
            while x + size <= pw - 24:
                rect = [x, y, x + size, y + size]
                clear = all(rect[2] < o[0] - 8 or rect[0] > o[2] + 8 or rect[3] < o[1] - 8 or rect[1] > o[3] + 8
                            for o in p.occupied)
                if clear and self.inside(rect, 6):
                    candidates.append(rect)
                x += size / 2
            y += size / 2
        chosen = []
        while candidates and len(chosen) < count:
            def spread(r):
                if not chosen:
                    return -abs(r[0] - pw / 2) - abs(r[1] - ph / 2)
                return min(abs(r[0] - c[0]) + abs(r[1] - c[1]) for c in chosen)
            best = max(candidates, key=spread)
            chosen.append(best)
            candidates.remove(best)
        return [am_round(r) for r in chosen]

    def truth(self, page_points):
        p = self.page
        for line in p.lines:
            assert self.inside(line["rect"], 1), f"ligne coupée par le cadrage : {line['texte']}"
        flats = []
        for flat in p.flats:
            on_top = [l["rect"] for l in p.lines if l["fond"] == flat["id"]]
            flats.append(dict(flat, dominante_scan=self.dominant(flat["rect"], on_top)))
        width, height = self.image.size
        return dict(
            format_pt=am_round(p.size), page_pdf_pt=am_round(page_points), ppp=p.dpi,
            image_px=[width, height], coins_page_px=self.corners(), a_verifier=abs(self.meta["inclinaison_deg"] or 0) > 15,
            **self.meta, zones_fond=self.blank_zones(), aplats=flats, photos=p.photos,
            degrades=p.gradients, filets=p.rules, lignes=p.lines,
        )


def am_write(name: str, title: str, description: str, langues, scans) -> None:
    """Écrit le PDF (une image JPEG par page, telle quelle, comme un scanner)
    et sa vérité terrain."""
    import json
    pdf = pikepdf.new()
    pages = []
    for scan in scans:
        width, height = scan.image.size
        points = (width * 72 / scan.page.dpi, height * 72 / scan.page.dpi)
        image = pikepdf.Stream(pdf, scan.jpeg)
        image.Type, image.Subtype = pikepdf.Name.XObject, pikepdf.Name.Image
        image.Width, image.Height, image.BitsPerComponent = width, height, 8
        image.ColorSpace = pikepdf.Name.DeviceGray if scan.image.mode == "L" else pikepdf.Name.DeviceRGB
        image.Filter = pikepdf.Name.DCTDecode
        page = pdf.add_blank_page(page_size=points)
        page.obj.Resources = pikepdf.Dictionary(XObject=pikepdf.Dictionary(Im0=image))
        page.obj.Contents = pdf.make_stream(b"q %.4f 0 0 %.4f 0 0 cm /Im0 Do Q" % points)
        pages.append(scan.truth(points))
    pdf.docinfo["/Title"] = title
    pdf.docinfo["/Author"] = "PDF Studio — jeu de test"
    save(pdf, f"amelioration/{name}.pdf")
    truth = {"fichier": f"{name}.pdf", "description": description, "langues": langues,
             "unites": "rectangles [x0, y0, x1, y1] en points sur la page droite, origine en haut "
                       "à gauche ; coins_page_px : coins de la page droite (haut gauche, haut droit, "
                       "bas droit, bas gauche) dans l'image scannée, en pixels ; inclinaison positive "
                       "quand les lignes descendent vers la droite ; quarts de tour dans le sens horaire",
             "pages": pages}
    (AMELIORATION / f"{name}.json").write_text(json.dumps(truth, ensure_ascii=False, indent=1) + "\n",
                                                encoding="utf-8")


def am_letter(page: AmPage, paragraphs, sender, place_date, subject, signature) -> None:
    """Lettre : expéditeur, date, objet, paragraphes, signature."""
    pw, _ = page.size
    left = 72.0
    y = 92.0
    page.text(left, y, sender[0], "Caladea-Bold", 12)
    for line in sender[1:]:
        y += 15
        page.text(left, y, line, "Caladea-Regular", 10.5)
    page.text(pw - left, 196, place_date, "Caladea-Regular", 11, align="right")
    page.text(left, 240, subject, "Caladea-Bold", 11.5)
    y = 276.0
    for text in paragraphs:
        y = page.paragraph(left, y, pw - 2 * left, text, "Caladea-Regular", 11.5) + 8
    page.text(pw - 230, y + 30, signature[0], "Caladea-Regular", 11.5)
    page.text(pw - 230, y + 46, signature[1], "Caladea-Italic", 10.5)


def am_report(page: AmPage, title, paragraphs, font="Carlito-Regular", size=11.0, top=96.0) -> float:
    pw, _ = page.size
    left = 72.0
    page.text(left, top, title, font.replace("Regular", "Bold"), size * 1.7)
    y = top + size * 3
    for text in paragraphs:
        y = page.paragraph(left, y, pw - 2 * left, text, font, size) + size * 0.7
    return y


def am_01_lettre_inclinee() -> None:
    page = AmPage(300, seed=SEED + 101)
    am_letter(page, OCR_LETTRE, ["Société Durand & Fils", "12, rue des Lilas", "69003 Lyon"],
              "Lyon, le 2 avril 2026", "Objet : confirmation de commande n° 2026-0412",
              ["Claire Martin", "Directrice administrative"])
    scan = AmScan(page, paper=(236, 236, 232), angle=2.3, blur=0.9, noise=4, jpeg=80, seed=1,
                  light=lambda u, v: 1.0 - 0.05 * u, light_note="léger assombrissement vers la droite (5 %)")
    am_write("01_lettre_inclinee", "Lettre inclinée", "Lettre en français, 300 ppp, inclinée de 2,3°, "
             "papier gris clair.", ["fr"], [scan])


def am_02_rapport_incline() -> None:
    page = AmPage(200, seed=SEED + 102)
    y = am_report(page, "Project Status Report", AM_STATUS)
    for item in ["Fibre backbone: completed", "Core switches: in progress (70%)",
                 "Wireless controllers: planned for November"]:
        page.text(90, y, "•  " + item, "Carlito-Regular", 11)
        y += 16
    scan = AmScan(page, paper=(244, 242, 236), angle=-7.5, blur=0.7, noise=4, jpeg=80, seed=2)
    am_write("02_rapport_incline", "Rapport incliné", "Rapport en anglais, 200 ppp, incliné de −7,5°.",
             ["en"], [scan])


def am_03_hors_plafond() -> None:
    page = AmPage(200, seed=SEED + 103)
    am_report(page, "Archives : du papier au texte", [OCR_ARTICLE], "Caladea-Regular", 11.5, top=120)
    scan = AmScan(page, paper=(246, 244, 238), lid=(226, 226, 224), angle=18.0, expand=True, blur=0.7,
                  noise=4, jpeg=80, seed=3)
    am_write("03_hors_plafond", "Inclinaison hors plafond", "Article en français, 200 ppp, posé de "
             "travers (18°, au-delà du plafond de 15°) : la page doit être marquée « à vérifier », "
             "sans correction automatique. L'image montre toute la page sur le fond du scanner.",
             ["fr"], [scan])


def am_04_page_droite() -> None:
    page = AmPage(300, seed=SEED + 104)
    am_report(page, "Visitor Badges: New Procedure", AM_MEMO, "Carlito-Regular", 11.5)
    scan = AmScan(page, paper=(253, 253, 252), angle=0.05, blur=0.5, noise=1.5, chroma=0.5, jpeg=92, seed=4)
    am_write("04_page_droite", "Page droite et propre", "Note en anglais, 300 ppp, déjà droite (0,05°, "
             "sous le seuil de 0,1°) et propre : aucune correction ni dégradation attendue.", ["en"], [scan])


def am_05_fond_jauni() -> None:
    page = AmPage(200, seed=SEED + 105)
    pw, _ = page.size
    y = am_report(page, "Note de service", AM_NOTE, "Caladea-Regular", 11)
    # Formulaire : filets fins à remplir.
    y += 10
    for label in ["Nom et prénom", "Service", "Dates souhaitées", "Visa du responsable"]:
        page.text(72, y, label, "Caladea-Regular", 10)
        page.rule(200, y + 2, pw - 72, y + 2, 0.3, (60, 60, 60))
        y += 26
    y += 16
    page.rule(72, y, pw - 72, y, 0.5, (30, 30, 30))
    y += 16
    for text in ["Article 1. Les demandes incomplètes ne seront pas traitées.",
                 "Article 2. Toute modification après validation suit la même procédure.",
                 "Article 3. Les plannings validés sont affichés dans chaque service."]:
        y = page.paragraph(72, y, pw - 144, text, "Carlito-Regular", 7) + 2
    scan = AmScan(page, paper=(240, 226, 188), angle=0.6, blur=0.8, noise=5, jpeg=78, seed=5,
                  light=lambda u, v: (1.0 - 0.35 * math.exp(-u / 0.06)) * (1.0 - 0.06 * v),
                  light_note="ombre de reliure à gauche (−35 % au bord), léger dégradé vertical")
    am_write("05_fond_jauni", "Papier jauni", "Note de service en français, 200 ppp, papier jauni, "
             "ombre de reliure, filets de 0,3 pt et texte en 7 pt.", ["fr"], [scan])


def am_06_photocopie_grise() -> None:
    page = AmPage(200, seed=SEED + 106)
    am_report(page, OCR_ANGLAIS[0], OCR_ANGLAIS[1:] + AM_STATUS[:2], "Caladea-Regular", 11)
    scan = AmScan(page, paper=(212, 212, 212), angle=-0.8, gray=True, blur=0.9, noise=7, jpeg=70, seed=6,
                  light=lambda u, v: 1.0 - 0.32 * u ** 1.5 - 0.08 * v,
                  light_note="éclairage très inégal : −32 % au bord droit, −8 % en bas")
    am_write("06_photocopie_grise", "Photocopie grise", "Rapport en anglais, photocopie en niveaux de "
             "gris à 200 ppp, papier gris, éclairage très inégal.", ["en"], [scan])


def am_07_bandeaux_couleur() -> None:
    page = AmPage(200, seed=SEED + 107)
    pw, ph = page.size
    page.flat("bandeau", [0, 36, pw, 120], (35, 90, 170), nuances=28)
    page.text(72, 88, "Compte rendu du comité de pilotage", "Carlito-Bold", 22, (255, 255, 255), "bandeau")
    page.text(72, 108, "Programme de modernisation — séance du 9 septembre", "Carlito-Regular", 11,
              (232, 240, 252), "bandeau")
    y = 160.0
    for text in AM_COMITE:
        y = page.paragraph(72, y, pw - 144, text, "Caladea-Regular", 11) + 8
    box = [72, y + 6, pw - 72, y + 120]
    page.flat("encadre", box, (252, 226, 196), nuances=18, border=(1.0, (214, 120, 40)))
    page.text(88, y + 30, "À retenir", "Carlito-Bold", 12, (120, 52, 10), "encadre")
    yy = y + 50
    for text in ["Prochaine réunion le 14 octobre à 10 h.",
                 "Validation du budget complémentaire attendue avant le 30 septembre.",
                 "Point d'étape sur la migration de Lille à la prochaine séance."]:
        page.text(88, yy, "– " + text, "Carlito-Regular", 10.5, (70, 40, 20), "encadre")
        yy += 17
    page.flat("pied", [0, ph - 70, pw, ph - 30], (40, 140, 90), nuances=26)
    page.text(pw / 2, ph - 45, "Direction des systèmes d'information — document interne", "Carlito-Regular",
              9.5, (255, 255, 255), "pied", align="center")
    scan = AmScan(page, paper=(246, 244, 238), angle=1.2, blur=0.8, noise=5, chroma=2.5, jpeg=80, seed=7,
                  light=lambda u, v: 1.0 - 0.07 * v, light_note="léger dégradé vertical (−7 % en bas)")
    am_write("07_bandeaux_couleur", "Bandeaux de couleur", "Compte rendu en français, 200 ppp : bandeau "
             "bleu aux nuances multiples sous du texte blanc, encadré orangé bordé, pied de page vert.",
             ["fr"], [scan])


def am_08_tableau_cellules() -> None:
    page = AmPage(200, seed=SEED + 108)
    pw, _ = page.size
    page.text(72, 96, "INVOICE", "Carlito-Bold", 24, (30, 60, 120))
    for i, (label, value) in enumerate([("Invoice no.", "INV-2026-0193"), ("Date", "March 3, 2026"),
                                        ("Customer", "Harbour Logistics Ltd")]):
        page.text(72, 128 + 16 * i, label, "Carlito-Bold", 10.5)
        page.text(160, 128 + 16 * i, value, "Carlito-Regular", 10.5)
    columns = [72, 132, 330, 380, 450, pw - 72]
    rows = [["Ref.", "Description", "Qty", "Unit price", "Amount"],
            ["N-101", "Managed switch, 48 ports", "2", "$1,240.00", "$2,480.00"],
            ["N-207", "Wireless access point", "6", "$315.00", "$1,890.00"],
            ["S-330", "Rack-mounted UPS, 3 kVA", "1", "$2,150.00", "$2,150.00"],
            ["C-012", "Patch cables, pack of 24", "4", "$58.00", "$232.00"],
            ["L-900", "Installation (hours)", "12", "$95.00", "$1,140.00"],
            ["", "", "", "Total", "$7,892.00"]]
    top, height = 200.0, 24.0
    fills = {0: ("entete", (30, 60, 120), 30), 2: ("ligne_2", (222, 233, 247), 14),
             4: ("ligne_4", (222, 233, 247), 14), 6: ("total", (232, 232, 232), 12)}
    for r, row in enumerate(rows):
        y0 = top + r * height
        if r in fills:
            ident, color, nuances = fills[r]
            page.flat(ident, [columns[0], y0, columns[-1], y0 + height], color, nuances)
        if r == 3:
            page.flat("cellule_surlignee", [columns[4], y0, columns[5], y0 + height], (255, 234, 128), 20)
    for r in range(len(rows) + 1):
        page.rule(columns[0], top + r * height, columns[-1], top + r * height, 0.5, (70, 70, 70))
    for x in columns:
        page.rule(x, top, x, top + len(rows) * height, 0.5, (70, 70, 70))
    for r, row in enumerate(rows):
        y0 = top + r * height
        on = {0: "entete", 2: "ligne_2", 4: "ligne_4", 6: "total"}.get(r, "papier")
        for c, value in enumerate(row):
            if not value:
                continue
            cell_on = "cellule_surlignee" if (r == 3 and c == 4) else on
            fill = (255, 255, 255) if r == 0 else (28, 28, 34)
            font = "Carlito-Bold" if r in (0, 6) else "Carlito-Regular"
            if c >= 2:
                page.text(columns[c + 1] - 6, y0 + 16, value, font, 10, fill, cell_on, align="right")
            else:
                page.text(columns[c] + 6, y0 + 16, value, font, 10, fill, cell_on)
    page.paragraph(72, top + len(rows) * height + 40, pw - 144,
                   "Payment due within 30 days. Please quote the invoice number with your payment. "
                   "Late payments incur interest at 1.5% per month.", "Carlito-Regular", 9.5)
    scan = AmScan(page, paper=(245, 244, 240), angle=-1.6, blur=0.8, noise=5, chroma=2.5, jpeg=80, seed=8)
    am_write("08_tableau_cellules", "Tableau à cellules colorées", "Facture en anglais, 200 ppp : en-tête "
             "bleu foncé sous du texte blanc, lignes alternées bleu clair, cellule surlignée en jaune, "
             "ligne de total grise, quadrillage de 0,5 pt.", ["en"], [scan])


def am_09_photo_et_degrade() -> None:
    page = AmPage(200, seed=SEED + 109)
    pw, _ = page.size
    page.gradient([0, 40, pw, 118], (24, 128, 140), (118, 64, 150))
    page.text(72, 92, "Parc naturel régional", "Carlito-Bold", 26, (255, 255, 255), "degrade")
    page.photo([72, 140, pw - 72, 430])
    page.text(72, 446, "Le lac et la chaîne des crêtes au lever du jour.", "Caladea-Italic", 9.5)
    y = 480.0
    for text in AM_PARC:
        y = page.paragraph(72, y, pw - 144, text, "Caladea-Regular", 11) + 8
    box = [72, y + 4, pw - 72, y + 84]
    page.flat("encadre_vert", box, (212, 238, 218), nuances=20)
    page.text(88, y + 28, "Informations pratiques", "Carlito-Bold", 11.5, (24, 90, 50), "encadre_vert")
    page.text(88, y + 46, "Accueil du parc : 04 76 00 12 34, tous les jours de 9 h à 18 h.",
              "Carlito-Regular", 10.5, (30, 50, 36), "encadre_vert")
    page.text(88, y + 63, "Parkings gratuits au départ des sentiers balisés.", "Carlito-Regular", 10.5,
              (30, 50, 36), "encadre_vert")
    scan = AmScan(page, paper=(248, 246, 240), angle=0.9, blur=0.7, noise=4, chroma=2.0, jpeg=82, seed=9)
    am_write("09_photo_et_degrade", "Photo et dégradé", "Dépliant en français, 200 ppp : photographie et "
             "dégradé volontaire (à ne pas aplatir), encadré vert uni (à aplatir).", ["fr"], [scan])


def am_10_basse_qualite() -> None:
    page = AmPage(150, seed=SEED + 110)
    am_report(page, "Note de service", AM_NOTE + [OCR_ARTICLE], "Caladea-Regular", 11)
    verso = AmPage(150, seed=SEED + 111)
    am_report(verso, "Compte rendu", AM_COMITE * 2, "Carlito-Regular", 11)
    scan = AmScan(page, paper=(226, 222, 206), angle=1.1, gray=True, blur=1.1, noise=11, jpeg=35, seed=10,
                  verso=verso, show_through=0.1,
                  light=lambda u, v: 1.0 - 0.12 * (1 - v) * u, light_note="assombri vers le haut à droite (−12 %)")
    am_write("10_basse_qualite", "Scan de basse qualité", "Note en français, 150 ppp en niveaux de gris, "
             "flou, bruit fort, JPEG de qualité 35, verso visible par transparence.", ["fr"], [scan])


def am_11_orientation() -> None:
    first = AmPage(200, seed=SEED + 112)
    am_report(first, "Visitor Badges: New Procedure", AM_MEMO, "Carlito-Regular", 11)
    upside_down = AmScan(first, paper=(246, 245, 240), quarter=2, blur=0.7, noise=4, jpeg=80, seed=11)
    second = AmPage(200, seed=SEED + 113)
    am_report(second, "Note de service", AM_NOTE, "Caladea-Regular", 11)
    sideways = AmScan(second, paper=(246, 245, 240), quarter=1, angle=0.8, blur=0.7, noise=4, jpeg=80, seed=12)
    am_write("11_orientation", "Pages retournées", "Page 1 en anglais, à l'envers (deux quarts de tour) ; "
             "page 2 en français, couchée (un quart de tour) et inclinée de 0,8°. 200 ppp.",
             ["en", "fr"], [upside_down, sideways])


def am_homography(src, dst):
    """Homographie qui envoie les quatre points `src` sur `dst` (coefficients
    a…h, avec i = 1)."""
    rows, rhs = [], []
    for (x, y), (u, v) in zip(src, dst):
        rows.append([x, y, 1, 0, 0, 0, -u * x, -u * y])
        rhs.append(u)
        rows.append([0, 0, 0, x, y, 1, -v * x, -v * y])
        rhs.append(v)
    n = 8
    m = [row + [b] for row, b in zip(rows, rhs)]
    for col in range(n):
        pivot = max(range(col, n), key=lambda r: abs(m[r][col]))
        m[col], m[pivot] = m[pivot], m[col]
        for r in range(n):
            if r != col:
                factor = m[r][col] / m[col][col]
                m[r] = [a - factor * b for a, b in zip(m[r], m[col])]
    return [m[i][n] / m[i][i] for i in range(n)]


class AmPhoto(AmScan):
    """Page photographiée sur un bureau : courbure de la feuille, perspective,
    éclairage en coin et ombre, fond de bois."""

    def __init__(self, page: AmPage, quad, size, bend, seed):
        rng = random.Random(seed)
        self.page = page
        w, h = page.img.size
        paper = (244, 241, 233)
        img = ImageChops.multiply(page.img, Image.new("RGB", (w, h), paper))

        # Courbure : les lignes se creusent vers le milieu de la page.
        def bend_at(x, y):
            return bend * (1 - (2 * x / w - 1) ** 2) * (0.55 + 0.45 * abs(2 * y / h - 1))

        mesh, step = [], 64
        for y0 in range(0, h, step):
            for x0 in range(0, w, step):
                x1, y1 = min(w, x0 + step), min(h, y0 + step)
                quad_src = [(x0, y0 - bend_at(x0, y0)), (x0, y1 - bend_at(x0, y1)),
                            (x1, y1 - bend_at(x1, y1)), (x1, y0 - bend_at(x1, y0))]
                mesh.append(((x0, y0, x1, y1), [v for p in quad_src for v in p]))
        img = img.transform((w, h), Image.MESH, mesh, Image.BICUBIC, fillcolor=paper)

        fw, fh = size
        page_corners = [(0, 0), (w, 0), (w, h), (0, h)]
        to_photo = am_homography(page_corners, quad)
        to_page = am_homography(quad, page_corners)

        def apply(hm, x, y):
            d = hm[6] * x + hm[7] * y + 1
            return ((hm[0] * x + hm[1] * y + hm[2]) / d, (hm[3] * x + hm[4] * y + hm[5]) / d)

        desk = ImageOps.colorize(ImageOps.autocontrast(Image.blend(
            am_smooth_noise(fw, fh, fw // 6, rng).resize((fw, fh)),
            am_smooth_noise(fw // 40, fh, 3, rng).resize((fw, fh), Image.BICUBIC), 0.6)),
            (96, 62, 38), (168, 120, 78))
        warped = img.transform((fw, fh), Image.PERSPECTIVE, to_page, Image.BICUBIC)
        mask = Image.new("L", (w, h), 255).transform((fw, fh), Image.PERSPECTIVE, to_page, Image.BICUBIC)
        shadow = mask.filter(ImageFilter.GaussianBlur(fw * 0.012))
        shadow = ImageChops.offset(shadow, int(fw * 0.008), int(fw * 0.012)).point(lambda v: 255 - v * 0.45)
        photo = ImageChops.multiply(desk, Image.merge("RGB", (shadow, shadow, shadow)))
        photo.paste(warped, (0, 0), mask)
        photo = am_multiply(photo, am_light(fw, fh, lambda u, v: 1.0 - 0.28 * ((u - 0.1) ** 2 + (v - 0.05) ** 2)))
        photo = photo.filter(ImageFilter.GaussianBlur(1.0))
        shared = am_gaussian_noise((fw, fh), 5, rng)
        photo = Image.merge("RGB", [ImageChops.add(b, shared, 1.0, -128) for b in photo.split()])
        buffer = io.BytesIO()
        photo.save(buffer, format="JPEG", quality=82, optimize=True)
        self.jpeg = buffer.getvalue()
        self.image = Image.open(io.BytesIO(self.jpeg))
        self.image.load()

        def forward(x, y):
            return apply(to_photo, x, y + bend_at(x, y))

        self.forward = forward
        self.meta = {"papier": list(paper), "couvercle": None, "eclairage": "photo : lumière en coin "
                     "(haut gauche), ombre portée", "inclinaison_deg": None, "quart_de_tour": 0,
                     "perspective": True, "courbure_px": bend, "flou_px": 1.0, "bruit": 5, "jpeg": 82,
                     "niveaux_de_gris": False, "transparence_verso": 0.0}


def am_12_photo_perspective() -> None:
    page = AmPage(200, seed=SEED + 114)
    am_letter(page, OCR_LETTRE, ["Atelier Lefèvre", "8, place de l'Église", "33000 Bordeaux"],
              "Bordeaux, le 18 mars 2026", "Objet : devis de rénovation n° D-2026-077",
              ["Paul Lefèvre", "Gérant"])
    size = (2400, 3200)
    quad = [(330, 260), (2110, 360), (2230, 2960), (180, 2890)]
    scan = AmPhoto(page, quad, size, bend=22, seed=13)
    am_write("12_photo_perspective", "Page photographiée", "Lettre en français photographiée sur un "
             "bureau : perspective, courbure de la feuille, lumière en coin et ombre portée. Sert à "
             "évaluer l'option facultative de correction de perspective.", ["fr"], [scan])


def am_13_lot_50_pages() -> None:
    """50 pages tirées en boucle de trois fichiers précédents (anglais en
    couleur, bandeaux, basse qualité en gris) : les images sont partagées, le
    fichier reste léger, mais chaque page est traitée."""
    import json
    sources = [AMELIORATION / f"{name}.pdf" for name in
               ("02_rapport_incline", "07_bandeaux_couleur", "10_basse_qualite")]
    pdf = pikepdf.new()
    copied, origin = [], []
    for source in sources:
        src = pikepdf.open(source)
        for index, page in enumerate(src.pages):
            pdf.pages.append(page)
            copied.append(len(pdf.pages) - 1)
            origin.append({"fichier": source.name, "page": index + 1})
    base = list(range(len(pdf.pages)))
    pages = []
    for n in range(50):
        k = base[n % len(base)]
        if n >= len(base):
            pdf.pages.append(pdf.pages[k])
        pages.append(origin[k])
    pdf.docinfo["/Title"] = "Lot de 50 pages"
    pdf.docinfo["/Author"] = "PDF Studio — jeu de test"
    save(pdf, "amelioration/13_lot_50_pages.pdf")
    truth = {"fichier": "13_lot_50_pages.pdf", "description": "50 pages tirées en boucle des fichiers "
             "02, 07 et 10 (images partagées) : traitement par lots en arrière-plan et annulation. La "
             "vérité terrain de chaque page est celle de son fichier d'origine.", "pages": pages}
    (AMELIORATION / "13_lot_50_pages.json").write_text(json.dumps(truth, ensure_ascii=False, indent=1) + "\n",
                                                        encoding="utf-8")


def amelioration_documents() -> None:
    AMELIORATION.mkdir(exist_ok=True)
    am_01_lettre_inclinee()
    am_02_rapport_incline()
    am_03_hors_plafond()
    am_04_page_droite()
    am_05_fond_jauni()
    am_06_photocopie_grise()
    am_07_bandeaux_couleur()
    am_08_tableau_cellules()
    am_09_photo_et_degrade()
    am_10_basse_qualite()
    am_11_orientation()
    am_12_photo_perspective()
    am_13_lot_50_pages()
    for f in sorted(AMELIORATION.glob("*.pdf")):
        print(f"amelioration/{f.name:28} {f.stat().st_size / 1024:8.1f} Kio")


def pas_un_pdf() -> None:
    (OUT / "pas_un_pdf.pdf").write_text("Ceci est un fichier texte, pas un PDF.\n", encoding="utf-8")


def main() -> None:
    if sys.argv[1:] == ["amelioration"]:
        amelioration_documents()
        return
    texte_simple()
    multipage("multipage_100.pdf", 100, "Cent pages")
    multipage("volumineux_600.pdf", 600, "Six cents pages")
    signets()
    formats_mixtes()
    vectoriel()
    tableau()
    tableaux()
    tableau_styles()
    scan_simule()
    scan_retouche()
    chiffre()
    pdf20()
    organisation()
    edition()
    police_cff()
    encadre()
    formulaire()
    signature_factice()
    corrompu()
    pas_un_pdf()
    ocr_documents()
    amelioration_documents()
    for f in sorted(OUT.glob("*.pdf")):
        print(f"{f.name:24} {f.stat().st_size / 1024:8.1f} Kio")


if __name__ == "__main__":
    main()
