//! Retouche d'un scan de test (`scan_retouche.pdf`), avec la couche de texte
//! qu'aurait lue la reconnaissance (`scan_retouche.mots.tsv`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use edit::{LayerWord, ObjectEdit, PageEdits, ScanText, TextLayer};

use crate::content::objects::ObjectKind;
use crate::editing::ObjectInfo;
use crate::writer::{write_document, OutputPage, SourceData};
use crate::Document;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn init_fonts() {
    fonts::catalog::set_bundled_directories(vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/Resources/Fonts")
    ]);
}

fn layer() -> TextLayer {
    let text = std::fs::read_to_string(fixtures().join("scan_retouche.mots.tsv")).unwrap();
    let words = text
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let mut quad = [0.0f32; 8];
            for (k, v) in fields[2..10].iter().enumerate() {
                quad[k] = v.parse().unwrap();
            }
            LayerWord {
                text: fields[0].to_owned(),
                quad,
                confidence: 0.95,
                line: fields[1].parse().unwrap(),
            }
        })
        .collect();
    TextLayer {
        words,
        engine: "test".into(),
    }
}

fn open() -> Document {
    Document::open_file(&fixtures().join("scan_retouche.pdf"), None).unwrap()
}

fn line<'a>(objects: &'a [ObjectInfo], text: &str) -> &'a ObjectInfo {
    objects
        .iter()
        .find(|o| o.text.as_deref() == Some(text))
        .unwrap_or_else(|| {
            let texts: Vec<_> = objects.iter().filter_map(|o| o.text.as_deref()).collect();
            panic!("ligne « {text} » absente : {texts:?}")
        })
}

fn single(id: u32, edit: ObjectEdit, revision: u64) -> PageEdits {
    let mut objects = BTreeMap::new();
    objects.insert(id, edit);
    PageEdits::new(objects, revision)
}

#[test]
fn objets_d_un_scan() {
    let mut doc = open();
    let layer = layer();
    // Sans couche de texte, la page n'est qu'une image.
    let plain = doc.page_objects(0, None, None).unwrap();
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].kind, ObjectKind::Image);

    let objects = doc.page_objects(0, None, Some(&layer)).unwrap();
    assert!(objects.iter().all(|o| o.scanned), "{objects:?}");
    let montant = line(&objects, "Montant total : 1 250,00 € TTC");
    assert!(montant.editable && montant.movable);
    assert!(
        (montant.bounds.x - 55.0).abs() < 3.0,
        "{:?}",
        montant.bounds
    );
    let tables: Vec<_> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .collect();
    assert_eq!(tables.len(), 1, "{objects:?}");
    let grid = tables[0].grid.as_ref().unwrap();
    assert_eq!((grid.rows, grid.columns), (3, 3));
    // Chaque texte du tableau est dans sa cellule.
    let audit = line(&objects, "Audit");
    assert_eq!(audit.table, Some(tables[0].id));
    assert!(grid.cells.iter().any(|c| c.objects == vec![audit.id]));
    let images = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Image)
        .count();
    let rulings = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Path)
        .count();
    assert_eq!((images, rulings), (1, 1), "{objects:?}");
    // Les numéros ne dépendent que du scan et de sa couche.
    let again = open().page_objects(0, None, Some(&layer)).unwrap();
    let ids = |o: &[ObjectInfo]| o.iter().map(|o| o.id).collect::<Vec<_>>();
    assert_eq!(ids(&objects), ids(&again));
}

#[test]
fn retrouve_la_police_et_reecrit_le_texte() {
    init_fonts();
    let mut doc = open();
    let layer = layer();
    let objects = doc.page_objects(0, None, Some(&layer)).unwrap();
    let montant = line(&objects, "Montant total : 1 250,00 € TTC").clone();
    let (style, candidates) = doc
        .scan_style(0, montant.id, Some(&layer))
        .unwrap()
        .unwrap();
    assert_eq!(style.family, "Caladea", "{candidates:?}");
    assert!((style.size - 11.0).abs() < 1.2, "{style:?}");
    assert!(
        candidates.len() >= 2 && candidates[0].score > 0.7,
        "{candidates:?}"
    );
    assert!(style.color[2] > style.color[0], "encre bleutée : {style:?}");

    let new_text = "Montant total : 1 380,00 € TTC";
    let edits = single(
        montant.id,
        ObjectEdit {
            scan: Some(ScanText {
                text: new_text.into(),
                style: style.clone(),
                measured: style,
                candidates,
            }),
            ..ObjectEdit::default()
        },
        1,
    );
    let objects = doc.page_objects(0, Some(&edits), Some(&layer)).unwrap();
    let edited = line(&objects, new_text);
    assert_eq!(edited.id, montant.id);
    assert!(edited.scan.is_some());

    // Couche de texte à jour : le nouveau mot, les autres gardés à leur
    // place.
    let effective = doc.effective_layer(0, Some(&edits), &layer).unwrap();
    let texts: Vec<&str> = effective.words.iter().map(|w| w.text.as_str()).collect();
    assert!(
        texts.contains(&"380,00") && !texts.contains(&"250,00"),
        "{texts:?}"
    );
    assert_eq!(effective.words.len(), layer.words.len());
    let ttc = |l: &TextLayer| l.words.iter().find(|w| w.text == "TTC").unwrap().quad[0];
    assert!((ttc(&effective) - ttc(&layer)).abs() < 3.0);

    // Pixels : seuls ceux de la ligne changent ; le nouveau montant est
    // dessiné à la place de l'ancien.
    let scan = doc.scan_model(0, Some(&layer)).unwrap().unwrap();
    let retouched = doc.retouched(0, &scan, &layer, &edits);
    assert!(!retouched.patches.is_empty());
    let line_zone = super::retouch::block_zone(scan.block(montant.id).unwrap());
    for patch in &retouched.patches {
        assert!(
            line_zone.inflate(4).contains_rect(&patch.rect),
            "{:?} hors de {line_zone:?}",
            patch.rect
        );
    }
    let changed: usize = retouched.patches.iter().map(|p| p.changed.count()).sum();
    assert!(changed > 200, "{changed}");

    // L'aperçu se construit.
    doc.edited_page_content(0, &edits, Some(&layer)).unwrap();
}

#[test]
fn supprime_et_deplace_des_objets() {
    let mut doc = open();
    let layer = layer();
    let objects = doc.page_objects(0, None, Some(&layer)).unwrap();
    let logo = objects
        .iter()
        .find(|o| o.kind == ObjectKind::Image)
        .unwrap()
        .clone();
    let table = objects
        .iter()
        .find(|o| o.kind == ObjectKind::Table)
        .unwrap()
        .clone();
    let mut map = BTreeMap::new();
    map.insert(
        logo.id,
        ObjectEdit {
            deleted: true,
            ..ObjectEdit::default()
        },
    );
    map.insert(
        table.id,
        ObjectEdit {
            dy: -100.0,
            ..ObjectEdit::default()
        },
    );
    let edits = PageEdits::new(map, 2);
    let after = doc.page_objects(0, Some(&edits), Some(&layer)).unwrap();
    assert!(after.iter().all(|o| o.id != logo.id));
    let moved = after.iter().find(|o| o.id == table.id).unwrap();
    assert!((moved.bounds.y - table.bounds.y - 100.0).abs() < 1.0);
    let audit = line(&after, "Audit");
    let audit_before = line(&objects, "Audit");
    assert!((audit.bounds.y - audit_before.bounds.y - 100.0).abs() < 1.0);
    // Les mots du tableau descendent dans la couche.
    let effective = doc.effective_layer(0, Some(&edits), &layer).unwrap();
    let y = |l: &TextLayer| l.words.iter().find(|w| w.text == "Audit").unwrap().quad[1];
    assert!((y(&layer) - y(&effective) - 100.0).abs() < 1.0);

    // Le logo a disparu : sa zone est couleur papier.
    let scan = doc.scan_model(0, Some(&layer)).unwrap().unwrap();
    let retouched = doc.retouched(0, &scan, &layer, &edits);
    let mut image = (*scan.base).clone();
    for patch in &retouched.patches {
        for y in patch.rect.y0..patch.rect.y1 {
            for x in patch.rect.x0..patch.rect.x1 {
                if patch.changed.get(x, y) {
                    image.set(x, y, patch.pixels.get(x - patch.rect.x0, y - patch.rect.y0));
                }
            }
        }
    }
    let element = scan.elements.iter().find(|e| e.id == logo.id).unwrap();
    let z = element.zone;
    let dark = (z.y0..z.y1)
        .flat_map(|y| (z.x0..z.x1).map(move |x| (x, y)))
        .filter(|&(x, y)| image.luma(x, y) < 180.0)
        .count();
    assert!(
        dark * 200 < z.area(),
        "{dark} pixels sombres sur {}",
        z.area()
    );
}

#[test]
fn enregistre_les_retouches() {
    init_fonts();
    let mut doc = open();
    let layer = layer();
    let objects = doc.page_objects(0, None, Some(&layer)).unwrap();
    let echeance = line(&objects, "Échéance : 30 avril 2026").clone();
    let (style, candidates) = doc
        .scan_style(0, echeance.id, Some(&layer))
        .unwrap()
        .unwrap();
    let edits = Arc::new(single(
        echeance.id,
        ObjectEdit {
            scan: Some(ScanText {
                text: "Échéance : 15 mai 2026".into(),
                measured: style.clone(),
                style,
                candidates,
            }),
            ..ObjectEdit::default()
        },
        3,
    ));
    let bytes = std::fs::read(fixtures().join("scan_retouche.pdf")).unwrap();
    let output = write_document(
        &[SourceData::new(bytes, None)],
        &[OutputPage::Source {
            source: 0,
            index: 0,
            quarter_turns: 0,
        }],
        &[Some(edits)],
        &[Some(Arc::new(layer))],
    )
    .unwrap();
    let saved = Document::from_bytes(&output, None).unwrap();
    let text = saved.plain_text(0).unwrap();
    assert!(
        text.contains("15 mai 2026") && !text.contains("30 avril"),
        "{text}"
    );
    // La page garde son image, un calque s'y ajoute.
    let raw = mupdf::Document::from_bytes(&output, "application/pdf").unwrap();
    let raw = mupdf::pdf::PdfDocument::try_from(raw).unwrap();
    let page = raw.find_page(0).unwrap();
    let xobjects = page
        .get_dict_inheritable("Resources")
        .unwrap()
        .unwrap()
        .get_dict("XObject")
        .unwrap()
        .unwrap();
    let names: Vec<String> = xobjects
        .dict_iter()
        .unwrap()
        .map(|entry| String::from_utf8(entry.unwrap().0.as_name().unwrap()).unwrap())
        .collect();
    assert!(
        names.len() >= 2 && names.iter().any(|n| n.starts_with("PSRT")),
        "{names:?}"
    );
}

#[test]
#[ignore = "écrit des images pour l'examen visuel"]
fn apercu_visuel() {
    init_fonts();
    let out = std::env::var("SCAN_DEBUG_DIR").unwrap_or_else(|_| "/tmp".into());
    let mut doc = open();
    let layer = layer();
    let objects = doc.page_objects(0, None, Some(&layer)).unwrap();
    let mut map = BTreeMap::new();
    for (old, new) in [
        (
            "Montant total : 1 250,00 € TTC",
            "Montant total : 1 380,00 € TTC",
        ),
        ("Échéance : 30 avril 2026", "Échéance : 15 septembre 2026"),
        ("Facture n° 2026-0412", "Facture n° 2026-0587"),
        ("Audit", "Conseil"),
    ] {
        let o = line(&objects, old).clone();
        let (style, candidates) = doc.scan_style(0, o.id, Some(&layer)).unwrap().unwrap();
        eprintln!("{old} → {style:?}\n   {candidates:?}");
        map.insert(
            o.id,
            ObjectEdit {
                scan: Some(ScanText {
                    text: new.into(),
                    measured: style.clone(),
                    style,
                    candidates,
                }),
                ..ObjectEdit::default()
            },
        );
    }
    let logo = objects
        .iter()
        .find(|o| o.kind == ObjectKind::Image)
        .unwrap();
    map.insert(
        logo.id,
        ObjectEdit {
            dx: -300.0,
            ..ObjectEdit::default()
        },
    );
    let edits = PageEdits::new(map, 9);
    let scan = doc.scan_model(0, Some(&layer)).unwrap().unwrap();
    let retouched = doc.retouched(0, &scan, &layer, &edits);
    let mut image = (*scan.base).clone();
    for patch in &retouched.patches {
        for y in patch.rect.y0..patch.rect.y1 {
            for x in patch.rect.x0..patch.rect.x1 {
                if patch.changed.get(x, y) {
                    image.set(x, y, patch.pixels.get(x - patch.rect.x0, y - patch.rect.y0));
                }
            }
        }
    }
    let crop = scan::Rect::new(100, 120, 1500, 900);
    for (name, img) in [("avant", &*scan.base), ("apres", &image)] {
        let part = img.crop(crop);
        let mut ppm = format!("P6 {} {} 255\n", part.width, part.height).into_bytes();
        ppm.extend_from_slice(&part.data);
        std::fs::write(format!("{out}/scan_{name}.ppm"), ppm).unwrap();
    }
}

/// Couche de texte lue d'un fichier `.mots.tsv` (texte, ligne, quadrilatère).
fn layer_from(path: &Path) -> TextLayer {
    let text = std::fs::read_to_string(path).unwrap();
    TextLayer {
        words: text
            .lines()
            .map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                let mut quad = [0.0f32; 8];
                for (k, v) in fields[2..10].iter().enumerate() {
                    quad[k] = v.parse().unwrap();
                }
                LayerWord {
                    text: fields[0].to_owned(),
                    quad,
                    confidence: 0.95,
                    line: fields[1].parse().unwrap(),
                }
            })
            .collect(),
        engine: "Vision".into(),
    }
}

fn write_ppm(image: &scan::RgbImage, path: &str) {
    let mut ppm = format!("P6 {} {} 255\n", image.width, image.height).into_bytes();
    ppm.extend_from_slice(&image.data);
    std::fs::write(path, ppm).unwrap();
}

/// Blocs des scans privés (`tests/fixtures/private/*.pN.mots.tsv`, écrits
/// sur le Mac par `couche_reconnue`) : leurs emprises sont dessinées sur le
/// scan, et le paragraphe le plus long est réécrit (un mot allongé), pour
/// l'examen visuel. Rien du contenu n'est affiché.
#[test]
#[ignore = "documents privés, examen visuel"]
fn blocs_des_scans_prives() {
    init_fonts();
    let out = std::env::var("SCAN_DEBUG_DIR").unwrap_or_else(|_| "/tmp".into());
    let dir = fixtures().join("private");
    let mut layers: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".mots.tsv"))
        .collect();
    layers.sort();
    for path in layers {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let stem = name.trim_end_matches(".mots.tsv");
        let (doc_name, page) = stem.rsplit_once(".p").unwrap();
        let index: u32 = page.parse::<u32>().unwrap() - 1;
        let mut doc = Document::open_file(&dir.join(format!("{doc_name}.pdf")), None).unwrap();
        let layer = layer_from(&path);
        let start = std::time::Instant::now();
        let scan = doc.scan_model(index, Some(&layer)).unwrap().unwrap();
        let elapsed = start.elapsed();
        let lines: usize = scan.blocks.iter().map(|b| b.lines.len()).sum();
        let paragraphs = scan.blocks.iter().filter(|b| b.paragraph).count();
        eprintln!(
            "{}… p{} : {} mots, {} lignes, {} blocs dont {} paragraphes ({:.0} ms)",
            doc_name.chars().take(10).collect::<String>(),
            index + 1,
            layer.words.len(),
            lines,
            scan.blocks.len(),
            paragraphs,
            elapsed.as_secs_f64() * 1000.0
        );
        let mut image = (*scan.base).clone();
        let colors = [
            [220, 0, 0],
            [0, 150, 0],
            [0, 0, 220],
            [180, 0, 180],
            [0, 140, 140],
            [200, 110, 0],
        ];
        let frame = |image: &mut scan::RgbImage, r: scan::Rect, c: [u8; 3], w: i32| {
            for t in 0..w {
                for x in r.x0..r.x1 {
                    image.set(x, r.y0 - t, c);
                    image.set(x, r.y1 + t, c);
                }
                for y in r.y0..r.y1 {
                    image.set(r.x0 - t, y, c);
                    image.set(r.x1 + t, y, c);
                }
            }
        };
        for (k, block) in scan.blocks.iter().enumerate() {
            let c = colors[k % colors.len()];
            for line in &block.lines {
                frame(&mut image, line.core, c, 2);
            }
            frame(&mut image, block.core.inflate(6), c, 5);
        }
        write_ppm(&image, &format!("{out}/blocs-{stem}.ppm"));

        // Réécriture du plus long paragraphe : un mot de sa deuxième ligne
        // est allongé, le reste suit.
        let Some(block) = scan
            .blocks
            .iter()
            .filter(|b| b.paragraph)
            .max_by_key(|b| b.lines.len())
        else {
            continue;
        };
        let target = block.lines[1].words[block.lines[1].words.len() / 2];
        let mut changed = false;
        let words: Vec<String> = block
            .text
            .split(' ')
            .map(|w| {
                if !changed
                    && block.words().position(|i| i == target).is_some()
                    && w.contains(&layer.words[target].text)
                {
                    changed = true;
                    format!("{w}{w}")
                } else {
                    w.to_owned()
                }
            })
            .collect();
        let new_text = words.join(" ");
        let (style, candidates) = doc
            .scan_style(index, block.id, Some(&layer))
            .unwrap()
            .unwrap();
        let edits = single(
            block.id,
            ObjectEdit {
                scan: Some(ScanText {
                    text: new_text,
                    measured: style.clone(),
                    style,
                    candidates,
                }),
                ..ObjectEdit::default()
            },
            1,
        );
        let start = std::time::Instant::now();
        let retouched = doc.retouched(index, &scan, &layer, &edits);
        eprintln!(
            "   réécriture : {} calques, {:.0} ms, débordement {}",
            retouched.patches.len(),
            start.elapsed().as_secs_f64() * 1000.0,
            retouched.overflow.contains(&block.id)
        );
        let mut after = (*scan.base).clone();
        for patch in &retouched.patches {
            for y in patch.rect.y0..patch.rect.y1 {
                for x in patch.rect.x0..patch.rect.x1 {
                    if patch.changed.get(x, y) {
                        after.set(x, y, patch.pixels.get(x - patch.rect.x0, y - patch.rect.y0));
                    }
                }
            }
        }
        let crop = block.core.inflate(40).intersect(&after.bounds());
        let crop = scan::Rect::new(
            crop.x0,
            crop.y0,
            crop.x1,
            (crop.y1 + 150).min(after.height as i32),
        );
        write_ppm(&scan.base.crop(crop), &format!("{out}/avant-{stem}.ppm"));
        write_ppm(&after.crop(crop), &format!("{out}/apres-{stem}.ppm"));
    }
}
