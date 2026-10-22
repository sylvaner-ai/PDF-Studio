#!/usr/bin/env python3
"""Génère le jeu de PDF de test publics de PDF Studio.

Usage : python3 tests/fixtures/generate.py
Dépendances : reportlab, pikepdf, pillow (pip install reportlab pikepdf pillow).

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
    signature_factice()
    corrompu()
    pas_un_pdf()
    for f in sorted(OUT.glob("*.pdf")):
        print(f"{f.name:24} {f.stat().st_size / 1024:8.1f} Kio")


if __name__ == "__main__":
    main()
