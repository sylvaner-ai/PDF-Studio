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
    for i, chapter in enumerate(chapters):
        for part in range(2):
            page += 1
            key = f"p{page}"
            c.bookmarkPage(key)
            if part == 0:
                c.addOutlineEntry(f"{i + 1}. {chapter}", key, level=0)
            c.addOutlineEntry(f"{i + 1}.{part + 1} Section {part + 1}", f"{key}", level=1)
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
    corrompu()
    pas_un_pdf()
    for f in sorted(OUT.glob("*.pdf")):
        print(f"{f.name:24} {f.stat().st_size / 1024:8.1f} Kio")


if __name__ == "__main__":
    main()
