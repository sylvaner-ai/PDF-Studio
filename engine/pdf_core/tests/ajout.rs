//! Objets ajoutés à une page (textes, images) : emprise, aperçu et
//! enregistrement, sur `tests/fixtures/edition.pdf` (page 1 droite, page 2
//! tournée de 90°).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use edit::{AddedImage, AddedObject, AddedText, FontChoice, PageEdits, ADDED_ID};
use pdf_core::content::geometry::Point;
use pdf_core::content::objects::ObjectKind;
use pdf_core::{write_edited_pdf, AddedInfo, Document, OutputPage, PageContent, SourceData};

fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/edition.pdf"))
        .unwrap()
}

/// PNG non compressé de `width` × `height` pixels RVBA, de couleur `rgba`.
fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let mut row = vec![0u8];
    for _ in 0..width {
        row.extend_from_slice(&rgba);
    }
    let raw = row.repeat(height as usize);
    // zlib, blocs non compressés.
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Point de la page PDF affiché en (`x`, `y`) sur la page `index`.
fn page_point(doc: &mut Document, index: u32, x: f64, y: f64) -> (f32, f32) {
    let to_display = doc.display_matrix(index).unwrap();
    let p = to_display.inverse().unwrap().apply(Point::new(x, y));
    (p.x as f32, p.y as f32)
}

/// Objets ajoutés à la page `index` : un texte dont le coin supérieur
/// gauche est affiché en (100, 100), une image rouge affichée dans le cadre
/// (300, 150, 380, 190).
fn additions(doc: &mut Document, index: u32, text: &str, font: FontChoice) -> Arc<PageEdits> {
    let (x, y) = page_point(doc, index, 100.0, 100.0);
    let (x0, y0) = page_point(doc, index, 300.0, 150.0);
    let (x1, y1) = page_point(doc, index, 380.0, 190.0);
    let mut added = BTreeMap::new();
    added.insert(
        ADDED_ID,
        AddedObject::Text(AddedText {
            text: text.to_owned(),
            font,
            size: 20.0,
            color: [0.0, 0.0, 1.0],
            x,
            y,
        }),
    );
    added.insert(
        ADDED_ID + 1,
        AddedObject::Image(AddedImage::new(
            png(8, 4, [220, 0, 0, 255]),
            [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)],
        )),
    );
    Arc::new(PageEdits::with_added(BTreeMap::new(), added, 3))
}

/// Couleur du pixel (`x`, `y`) d'une page rendue à 100 %.
fn pixel(content: &PageContent, x: usize, y: usize) -> [u8; 3] {
    let pixmap = content
        .display_list()
        .to_pixmap(
            &mupdf::Matrix::IDENTITY,
            &mupdf::Colorspace::device_rgb(),
            false,
        )
        .unwrap();
    let n = pixmap.n() as usize;
    let stride = pixmap.stride() as usize;
    let at = y * stride + x * n;
    let s = pixmap.samples();
    [s[at], s[at + 1], s[at + 2]]
}

fn is_red(color: [u8; 3]) -> bool {
    color[0] > 180 && color[1] < 60 && color[2] < 60
}

#[test]
fn objets_ajoutes_affiches_droits_sur_une_page_droite_ou_tournee() {
    for index in [0, 1] {
        let mut doc = Document::from_bytes(&fixture(), None).unwrap();
        let before = doc.page_objects(index, None, None).unwrap().len();
        let edits = additions(
            &mut doc,
            index,
            "Ajouté\nà la main",
            FontChoice::Standard {
                name: "Helvetica".to_owned(),
            },
        );
        let objects = doc.page_objects(index, Some(&edits), None).unwrap();
        assert_eq!(objects.len(), before + 2);
        let text = &objects[before];
        assert_eq!(text.id, ADDED_ID);
        assert_eq!(text.kind, ObjectKind::Text);
        assert!(text.editable && text.movable);
        assert_eq!(text.text.as_deref(), Some("Ajouté\nà la main"));
        assert_eq!(text.font_name.as_deref(), Some("Helvetica"));
        assert!(!text.substituted && text.missing.is_empty());
        assert!(matches!(
            text.added,
            Some(AddedInfo::Text { size, .. }) if size == 20.0
        ));
        // Deux lignes de 20 pt, à partir du coin demandé, à l'endroit.
        let b = text.bounds;
        assert!(
            (b.x - 100.0).abs() < 0.01 && (b.y - 100.0).abs() < 0.01,
            "{b:?}"
        );
        assert!(b.width > 60.0 && b.width < 120.0, "{b:?}");
        assert!(b.height > 44.0 && b.height < 56.0, "{b:?}");
        let image = &objects[before + 1];
        assert_eq!(image.kind, ObjectKind::Image);
        assert_eq!(image.added, Some(AddedInfo::Image));
        let b = image.bounds;
        assert!(
            (b.x - 300.0).abs() < 0.01 && (b.y - 150.0).abs() < 0.01,
            "{b:?}"
        );
        assert!((b.width - 80.0).abs() < 0.01 && (b.height - 40.0).abs() < 0.01);

        // Aperçu : l'image est dessinée dans son cadre, le texte à sa place.
        let preview = doc.edited_page_content(index, &edits, None).unwrap();
        assert!(is_red(pixel(&preview, 340, 170)));
        assert!(!is_red(pixel(&preview, 340, 200)));
        let original = doc.page_content(index).unwrap();
        assert!(!is_red(pixel(&original, 340, 170)));

        // Enregistré : même dessin, texte lisible.
        let mut pages = vec![
            OutputPage::Source {
                source: 0,
                index: 0,
                quarter_turns: 0,
            },
            OutputPage::Source {
                source: 0,
                index: 1,
                quarter_turns: 0,
            },
        ];
        let mut all = vec![None, None];
        all[index as usize] = Some(Arc::clone(&edits));
        let bytes = write_edited_pdf(&[SourceData::new(fixture(), None)], &pages, &all).unwrap();
        let saved = Document::from_bytes(&bytes, None).unwrap();
        assert!(is_red(pixel(&saved.page_content(index).unwrap(), 340, 170)));
        let text = saved.plain_text(index).unwrap();
        assert!(
            text.contains("Ajouté") && text.contains("à la main"),
            "{text}"
        );

        // Page tournée d'un quart de tour par l'organisation : les objets
        // tournent avec elle, comme le reste de son contenu.
        if let OutputPage::Source { quarter_turns, .. } = &mut pages[index as usize] {
            *quarter_turns = 1;
        }
        let bytes = write_edited_pdf(&[SourceData::new(fixture(), None)], &pages, &all).unwrap();
        let turned = Document::from_bytes(&bytes, None).unwrap();
        let height = doc.page_sizes()[index as usize].height as usize;
        // Le point (340, 170) de la page droite passe en (hauteur − 170, 340).
        assert!(is_red(pixel(
            &turned.page_content(index).unwrap(),
            height - 170,
            340
        )));
    }
}

#[test]
fn caracteres_hors_winansi_et_police_introuvable() {
    let mut doc = Document::from_bytes(&fixture(), None).unwrap();
    // Ω n'est pas dans WinAnsi : le clone URW d'Helvetica est intégré.
    let edits = additions(
        &mut doc,
        0,
        "Ω ≈ 3 €",
        FontChoice::Standard {
            name: "Helvetica".to_owned(),
        },
    );
    let objects = doc.page_objects(0, Some(&edits), None).unwrap();
    let text = objects.iter().find(|o| o.id == ADDED_ID).unwrap();
    assert!(text.missing.is_empty(), "{:?}", text.missing);
    let pages = [OutputPage::Source {
        source: 0,
        index: 0,
        quarter_turns: 0,
    }];
    let bytes = write_edited_pdf(
        &[SourceData::new(fixture(), None)],
        &pages,
        &[Some(Arc::clone(&edits))],
    )
    .unwrap();
    let saved = Document::from_bytes(&bytes, None).unwrap();
    let plain = saved.plain_text(0).unwrap();
    assert!(plain.contains("Ω ≈ 3 €"), "{plain}");

    // Une police du Mac introuvable est remplacée par la police standard la
    // plus proche, ce que l'application signale.
    let edits = additions(
        &mut doc,
        0,
        "Bonjour",
        FontChoice::Installed {
            postscript_name: "PoliceInexistante-Bold".to_owned(),
        },
    );
    let objects = doc.page_objects(0, Some(&edits), None).unwrap();
    let text = objects.iter().find(|o| o.id == ADDED_ID).unwrap();
    assert!(text.substituted);
    assert_eq!(text.font_name.as_deref(), Some("Helvetica-Bold"));
}

#[test]
fn image_illisible_refusee() {
    assert_eq!(pdf_core::image_size(&png(8, 4, [0; 4])).unwrap(), (8, 4));
    assert!(pdf_core::image_size(b"pas une image").is_err());
}
