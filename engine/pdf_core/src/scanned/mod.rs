//! Pages scannées : l'image du scan devient une page d'objets (lignes de
//! texte reconnues, tableaux, éléments graphiques, filets) que l'on peut
//! supprimer, déplacer et, pour le texte, réécrire.
//!
//! Une page est traitée comme un scan si elle ne contient pas de texte
//! visible, qu'une image en couvre au moins la moitié et qu'elle a une
//! couche de texte reconnue. Les retouches travaillent sur les pixels du
//! scan, rendus à la résolution de l'image (cf. [`retouch`]) ; le contenu
//! de la page n'est jamais réécrit : les pixels modifiés sont posés
//! par-dessus, en calques d'image alignés sur ceux du scan.

mod output;
mod retouch;

use std::sync::Arc;

use edit::{LayerWord, ScanCandidate, ScanStyle, TextLayer};
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::{Colorspace, Device, Pixmap};
use scan::line::{observe, LineFrame, LineInk};
use scan::rulings::{dark_mask, detect, Ruling};
use scan::{Rect as PixelRect, RgbImage};

use crate::content::geometry::{Matrix, Point, Rect};
use crate::content::objects::ObjectKind;
use crate::editing::PageModel;
use crate::error::Result;

pub(crate) use output::{compose, infos, write_patches};
pub(crate) use retouch::{retouch, Patch, Retouched};

/// Bit qui distingue les numéros des objets d'un scan de ceux du contenu de
/// la page (ordre d'empilement, toujours plus petits).
pub(crate) const SCAN_ID: u32 = 0x4000_0000;

/// Résolution maximale du rendu d'un scan, en pixels par point (600 ppp).
const MAX_SCALE: f64 = 600.0 / 72.0;

/// Nombre maximal de pixels du rendu d'un scan.
const MAX_PIXELS: f64 = 60_000_000.0;

/// Part de la page qu'une image doit couvrir pour être un scan.
const SCAN_SHARE: f64 = 0.5;

/// Vrai si `id` désigne un objet d'un scan.
pub(crate) fn is_scan_id(id: u32) -> bool {
    id & SCAN_ID != 0
}

/// Correspondance entre l'espace de la page PDF et les pixels du rendu du
/// scan, alignés sur ceux de l'image.
///
/// Les pixels suivent la page droite, sans sa rotation `/Rotate` : les
/// objets d'un scan (et leurs numéros) ne changent pas quand on fait
/// pivoter la page.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    /// Espace de la page PDF → page droite (origine en haut à gauche, sans
    /// rotation).
    to_canon: Matrix,
    from_canon: Matrix,
    /// Espace de la page PDF → espace d'affichage (rotation comprise).
    to_display: Matrix,
    /// Coin supérieur gauche du rendu, sur la page droite.
    origin: (f64, f64),
    /// Pixels par point.
    pub scale: f64,
    /// Largeur du rendu en pixels.
    pub width: usize,
    /// Hauteur du rendu en pixels.
    pub height: usize,
}

impl Placement {
    /// Pixel d'un point de l'espace de la page PDF.
    pub fn to_pixels(self, x: f64, y: f64) -> (f64, f64) {
        let p = self.to_canon.apply(Point::new(x, y));
        (
            (p.x - self.origin.0) * self.scale,
            (p.y - self.origin.1) * self.scale,
        )
    }

    /// Point de l'espace de la page PDF d'un pixel.
    pub fn to_page(self, x: f64, y: f64) -> (f64, f64) {
        let p = self.from_canon.apply(Point::new(
            self.origin.0 + x / self.scale,
            self.origin.1 + y / self.scale,
        ));
        (p.x, p.y)
    }

    /// Rectangle de pixels, sur la page droite.
    fn canon_rect(&self, rect: &PixelRect) -> Rect {
        Rect::new(
            self.origin.0 + f64::from(rect.x0) / self.scale,
            self.origin.1 + f64::from(rect.y0) / self.scale,
            self.origin.0 + f64::from(rect.x1) / self.scale,
            self.origin.1 + f64::from(rect.y1) / self.scale,
        )
    }

    /// Passage de la page droite à l'espace d'affichage.
    fn canon_to_display(&self) -> Matrix {
        self.from_canon.then(&self.to_display)
    }

    /// Rectangle de pixels, dans l'espace d'affichage.
    pub fn display_rect(&self, rect: &PixelRect) -> Rect {
        self.canon_to_display().apply_rect(self.canon_rect(rect))
    }

    /// Déplacement de l'espace de la page PDF, en pixels entiers.
    pub fn pixel_offset(&self, dx: f64, dy: f64) -> (i32, i32) {
        let (x, y) = self.to_canon.apply_vector(dx, dy);
        (
            (x * self.scale).round() as i32,
            (y * self.scale).round() as i32,
        )
    }

    /// Déplacement en pixels, dans l'espace de la page PDF.
    pub fn page_offset(&self, dx: i32, dy: i32) -> (f64, f64) {
        self.from_canon
            .apply_vector(f64::from(dx) / self.scale, f64::from(dy) / self.scale)
    }

    /// Matrice qui place un calque de pixels `rect` dans l'espace
    /// d'affichage (carré unité d'une image, rangée 0 en haut).
    pub fn display_image_matrix(&self, rect: &PixelRect) -> Matrix {
        let r = self.canon_rect(rect);
        Matrix::new(r.width(), 0.0, 0.0, r.height(), r.x0, r.y0).then(&self.canon_to_display())
    }

    /// Matrice qui place un calque de pixels `rect` dans l'espace de la page
    /// PDF (opérateur `cm` avant `Do` : rangée 0 de l'image en haut).
    pub fn page_image_matrix(&self, rect: &PixelRect) -> Matrix {
        let r = self.canon_rect(rect);
        Matrix::new(r.width(), 0.0, 0.0, -r.height(), r.x0, r.y1).then(&self.from_canon)
    }

    /// Matrice du rendu : espace d'affichage → pixels.
    fn render_matrix(&self) -> Option<Matrix> {
        let from_display = self.to_display.inverse()?;
        let s = self.scale;
        Some(from_display.then(&self.to_canon).then(&Matrix::new(
            s,
            0.0,
            0.0,
            s,
            -self.origin.0 * s,
            -self.origin.1 * s,
        )))
    }
}

/// Image du scan d'une page et sa place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScanImage {
    /// Objet de la page qui dessine l'image.
    pub object: u32,
    /// Placement de ses pixels.
    pub placement: Placement,
}

/// Vrai si le texte de la page est une couche invisible posée sur un scan
/// (OCR d'un scanner, d'un autre logiciel ou d'un enregistrement précédent) :
/// aucun texte visible, du texte invisible, et une image qui couvre au moins
/// la moitié de la page. Une nouvelle reconnaissance remplace alors ce texte.
pub(crate) fn has_replaceable_text(
    page: &PdfObject,
    model: &PageModel,
    page_area: f64,
) -> Result<bool> {
    Ok(model.has_only_invisible_text() && find_scan(page, model, page_area)?.is_some())
}

/// Cherche l'image d'un scan sur la page décrite par `model` : aucun texte
/// visible, une image qui couvre au moins la moitié de la page.
pub(crate) fn find_scan(
    page: &PdfObject,
    model: &PageModel,
    page_area: f64,
) -> Result<Option<ScanImage>> {
    let objects = model.objects();
    if objects
        .iter()
        .any(|o| o.kind == ObjectKind::Text || o.kind == ObjectKind::Table)
    {
        return Ok(None);
    }
    let to_display = model.display_matrix();
    let page_box = crate::editing::page_box(page)?;
    let to_canon = Matrix::new(1.0, 0.0, 0.0, -1.0, -page_box.x0, page_box.y1);
    let Some(image) = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Image)
        .max_by(|a, b| a.bbox.area().total_cmp(&b.bbox.area()))
    else {
        return Ok(None);
    };
    let display = to_canon.apply_rect(image.bbox);
    if display.area() < SCAN_SHARE * page_area || display.is_empty() {
        return Ok(None);
    }
    let Some((w, h)) = largest_image(page)? else {
        return Ok(None);
    };
    let Some(from_canon) = to_canon.inverse() else {
        return Ok(None);
    };
    let mut scale = (f64::from(w.max(h)) / display.width().max(display.height())).min(MAX_SCALE);
    let pixels = display.area() * scale * scale;
    if pixels > MAX_PIXELS {
        scale *= (MAX_PIXELS / pixels).sqrt();
    }
    let scale = scale.max(1.0);
    Ok(Some(ScanImage {
        object: image.id,
        placement: Placement {
            to_canon,
            from_canon,
            to_display,
            origin: (display.x0, display.y0),
            scale,
            width: (display.width() * scale).round().max(1.0) as usize,
            height: (display.height() * scale).round().max(1.0) as usize,
        },
    }))
}

/// Taille en pixels de la plus grande image des ressources de la page.
fn largest_image(page: &PdfObject) -> Result<Option<(u32, u32)>> {
    let Some(resources) = page.get_dict_inheritable("Resources")? else {
        return Ok(None);
    };
    let Some(xobjects) = resources.get_dict("XObject")? else {
        return Ok(None);
    };
    if !xobjects.is_dict()? {
        return Ok(None);
    }
    let mut best: Option<(u32, u32)> = None;
    for entry in xobjects.dict_iter()? {
        let (_, object) = entry?;
        let subtype = match object.get_dict("Subtype")? {
            Some(value) if value.is_name()? => value.as_name()?,
            _ => continue,
        };
        if subtype != b"Image" {
            continue;
        }
        let dimension = |key: &str| -> Result<u32> {
            Ok(match object.get_dict(key)? {
                Some(value) if value.is_number()? => value.as_int()?.max(0) as u32,
                _ => 0,
            })
        };
        let (w, h) = (dimension("Width")?, dimension("Height")?);
        if best.is_none_or(|(bw, bh)| u64::from(w) * u64::from(h) > u64::from(bw) * u64::from(bh)) {
            best = Some((w, h));
        }
    }
    Ok(best.filter(|(w, h)| *w > 0 && *h > 0))
}

/// Rend la page `index` (sans ses annotations) aux pixels du scan.
pub(crate) fn render(doc: &PdfDocument, index: u32, placement: &Placement) -> Result<RgbImage> {
    let page = doc.load_page(index as i32)?;
    let list = page.to_display_list(false)?;
    let (w, h) = (placement.width as i32, placement.height as i32);
    let mut pixmap = Pixmap::new_with_w_h(&Colorspace::device_rgb(), w, h, false)?;
    pixmap.samples_mut().fill(255);
    if let Some(m) = placement.render_matrix() {
        let device = Device::from_pixmap(&pixmap)?;
        let ctm = mupdf::Matrix::new(
            m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32,
        );
        list.run(
            &device,
            &ctm,
            mupdf::Rect::new(0.0, 0.0, w as f32, h as f32),
        )?;
    }
    let stride = pixmap.stride() as usize;
    let n = pixmap.n() as usize;
    let samples = pixmap.samples();
    let mut image = RgbImage::new(placement.width, placement.height, [255; 3]);
    for y in 0..placement.height {
        let row = &samples[y * stride..];
        for x in 0..placement.width {
            let i = x * n;
            image.set(x as i32, y as i32, [row[i], row[i + 1], row[i + 2]]);
        }
    }
    Ok(image)
}

/// Une ligne de texte reconnue du scan.
#[derive(Clone, Debug)]
pub(crate) struct ScanLine {
    pub id: u32,
    /// Mots de la couche de texte d'origine.
    pub words: Vec<usize>,
    /// Texte d'origine.
    pub text: String,
    /// Bande de la ligne, en pixels.
    pub frame: LineFrame,
    /// Emprise des mots, en pixels.
    pub core: PixelRect,
    /// Tableau dont la ligne occupe une cellule.
    pub table: Option<u32>,
}

impl ScanLine {
    /// Hauteur de la ligne (emprise des mots), en pixels.
    pub fn height(&self) -> f64 {
        self.core.height() as f64
    }
}

/// Un tableau trouvé dans le scan.
#[derive(Clone, Debug)]
pub(crate) struct ScanTable {
    pub id: u32,
    /// Structure, dans l'espace redressé (points, ordonnées vers le haut).
    pub table: layout::Table,
    /// Emprise, en pixels.
    pub zone: PixelRect,
    /// Lignes de texte de chaque cellule.
    pub cells: Vec<Vec<u32>>,
}

/// Un élément graphique ou un filet isolé du scan.
#[derive(Clone, Debug)]
pub(crate) struct ScanElement {
    pub id: u32,
    /// Image (logo, signature, tampon) ou tracé (filet).
    pub kind: ObjectKind,
    /// Emprise, en pixels.
    pub zone: PixelRect,
}

/// Objets d'une page scannée, établis à partir de son rendu et de sa couche
/// de texte d'origine.
pub(crate) struct ScanModel {
    pub image: ScanImage,
    /// Rendu de la page, aux pixels du scan.
    pub base: Arc<RgbImage>,
    /// Redressement vers l'espace des tableaux.
    pub deskew: Deskew,
    pub lines: Vec<ScanLine>,
    pub tables: Vec<ScanTable>,
    pub elements: Vec<ScanElement>,
}

/// Cellule d'un tableau : (tableau, cellule), par indice.
type Cell = Option<(usize, usize)>;

/// Mot de la couche placé dans les pixels.
struct PixelWord {
    corners: [(f64, f64); 4],
    rect: PixelRect,
    center: (f64, f64),
}

impl ScanModel {
    /// Objets du scan `image` dont `base` est le rendu, d'après la couche de
    /// texte `layer`.
    pub fn build(image: ScanImage, base: RgbImage, layer: &TextLayer) -> ScanModel {
        let placement = image.placement;
        let s = placement.scale;
        let words: Vec<PixelWord> = layer
            .words
            .iter()
            .map(|word| {
                let corners = [
                    word.bottom_left(),
                    word.bottom_right(),
                    word.top_right(),
                    word.top_left(),
                ]
                .map(|(x, y)| placement.to_pixels(f64::from(x), f64::from(y)));
                let (x0, y0, x1, y1) = bounds(&corners);
                PixelWord {
                    corners,
                    rect: PixelRect::covering(x0, y0, x1, y1),
                    center: ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                }
            })
            .collect();
        let skew = skew(layer, &words);

        // Filets.
        let dark = dark_mask(&base, 60.0);
        let rulings: Vec<Ruling> = detect(
            &dark,
            skew,
            (18.0 * s) as usize,
            (4.0 * s).ceil() as usize + 2,
        )
        .into_iter()
        .filter(|ruling| {
            let b = ruling.bounds();
            !words.iter().any(|w| w.rect.inflate(2).contains_rect(&b))
        })
        .collect();

        // Tableaux, dans l'espace redressé.
        let center = (base.width as f64 / 2.0, base.height as f64 / 2.0);
        let deskew = Deskew {
            angle: -skew.to_radians(),
            center,
            scale: s,
            height: base.height as f64,
        };
        let segments: Vec<layout::Segment> = rulings
            .iter()
            .map(|r| layout::Segment::new(deskew.apply(r.from), deskew.apply(r.to)))
            .collect();
        let boxes: Vec<layout::TextBox> = words
            .iter()
            .map(|w| {
                let points = w.corners.map(|p| deskew.apply(p));
                let (x0, y0, x1, y1) = bounds(&points);
                layout::TextBox {
                    bbox: layout::Rect::new(x0, y0, x1, y1),
                    baseline: y0 + 0.2 * (y1 - y0),
                    size: 0.8 * (y1 - y0),
                }
            })
            .collect();
        let found = layout::detect(&segments, &boxes);
        let mut ids = Ids::default();
        let mut tables: Vec<ScanTable> = found
            .into_iter()
            .map(|table| {
                let zone = deskew.pixel_rect(&table.bbox());
                ScanTable {
                    id: ids.next("table", &zone, ""),
                    zone,
                    cells: vec![Vec::new(); table.cells.len()],
                    table,
                }
            })
            .collect();

        // Lignes : mots d'une même ligne reconnue, séparés aux cellules et
        // aux grands écarts.
        let cell_of = |w: &PixelWord| -> Option<(usize, usize)> {
            let (x, y) = deskew.apply(w.center);
            tables
                .iter()
                .enumerate()
                .find_map(|(t, table)| table.table.cell_at(x, y).map(|c| (t, c)))
        };
        let mut groups: Vec<(Vec<usize>, Cell)> = Vec::new();
        for (i, word) in layer.words.iter().enumerate() {
            let cell = cell_of(&words[i]);
            let split = match groups.last() {
                Some((last, last_cell)) => {
                    let previous = *last.last().unwrap_or(&i);
                    let height = words[previous].rect.height().max(1) as f64;
                    let gap = gap_between(&words[previous], &words[i]);
                    layer.words[previous].line != word.line
                        || *last_cell != cell
                        || gap > 1.2 * height
                }
                None => true,
            };
            if split {
                groups.push((vec![i], cell));
            } else if let Some((last, _)) = groups.last_mut() {
                last.push(i);
            }
        }
        let mut lines = Vec::with_capacity(groups.len());
        for (members, cell) in groups {
            let text = members
                .iter()
                .map(|&i| layer.words[i].text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let corners: Vec<(f64, f64)> = members.iter().flat_map(|&i| words[i].corners).collect();
            let frame = line_frame(&members.iter().map(|&i| &words[i]).collect::<Vec<_>>());
            let (x0, y0, x1, y1) = bounds(&corners);
            let core = PixelRect::covering(x0, y0, x1, y1);
            let key: String = members
                .iter()
                .map(|&i| {
                    let q = &layer.words[i].quad;
                    format!("{}:{:.1}:{:.1};", layer.words[i].text, q[0], q[1])
                })
                .collect();
            let id = ids.next("line", &core, &key);
            if let Some((t, c)) = cell {
                tables[t].cells[c].push(id);
            }
            lines.push(ScanLine {
                id,
                words: members,
                text,
                frame,
                core,
                table: cell.map(|(t, _)| tables[t].id),
            });
        }

        // Éléments graphiques : encre hors des mots et des filets.
        let mut excluded: Vec<PixelRect> = words
            .iter()
            .map(|w| {
                w.rect
                    .inflate((w.rect.height() as f64 * 0.25).ceil() as i32)
            })
            .collect();
        excluded.extend(rulings.iter().map(|r| r.bounds().inflate(2)));
        let factor = ((s * 72.0 / 75.0).floor() as usize).max(1);
        let page_area = (base.width * base.height) as f64;
        let mut elements: Vec<ScanElement> = scan::elements::find(
            &base,
            &dark,
            &excluded,
            &scan::elements::Finding {
                factor,
                gap: (7.0 * s) as i32,
                min_size: (9.0 * s) as i32,
            },
        )
        .into_iter()
        .filter(|zone| (zone.area() as f64) < 0.4 * page_area)
        .map(|zone| ScanElement {
            id: ids.next("element", &zone, ""),
            kind: ObjectKind::Image,
            zone,
        })
        .collect();
        // Filets isolés, hors des tableaux.
        for ruling in &rulings {
            let zone = ruling.bounds();
            if tables.iter().any(|t| {
                t.zone
                    .inflate((2.0 * s).ceil() as i32 + 4)
                    .contains_rect(&zone)
            }) {
                continue;
            }
            elements.push(ScanElement {
                id: ids.next("ruling", &zone, ""),
                kind: ObjectKind::Path,
                zone,
            });
        }

        ScanModel {
            image,
            base: Arc::new(base),
            deskew,
            lines,
            tables,
            elements,
        }
    }

    /// Ligne `id`.
    pub fn line(&self, id: u32) -> Option<&ScanLine> {
        self.lines.iter().find(|line| line.id == id)
    }

    /// Mesure l'encre de la ligne `line` sur le scan d'origine.
    pub fn measure(&self, line: &ScanLine, layer: &TextLayer) -> LineGeometry {
        let frame = line.frame;
        let spans: Vec<(String, f64, f64)> = line
            .words
            .iter()
            .map(|&i| {
                let word = &layer.words[i];
                let (s0, s1) = word_span(&frame, word, &self.image.placement);
                (word.text.clone(), s0, s1)
            })
            .collect();
        let ink = observe(&self.base, &frame, &spans);
        // Emprise de l'encre de chaque mot, le long de la ligne.
        let obs = &ink.observation;
        let (w, h) = (obs.width, obs.height);
        let mut extents = Vec::with_capacity(spans.len());
        for (k, (_, s0, s1)) in spans.iter().enumerate() {
            let previous = if k == 0 {
                0.0
            } else {
                (spans[k - 1].2 + s0) / 2.0
            };
            let next = spans.get(k + 1).map_or(w as f64, |n| (s1 + n.1) / 2.0);
            let from = previous.max(0.0) as usize;
            let to = (next.ceil() as usize).min(w);
            let columns: Vec<usize> = (from..to)
                .filter(|&x| (0..h).any(|y| obs.ink[y * w + x] >= 0.35))
                .collect();
            match (columns.first(), columns.last()) {
                (Some(&a), Some(&b)) => extents.push((a as f64, b as f64 + 1.0)),
                _ => extents.push((*s0, *s1)),
            }
        }
        let mut gaps: Vec<f64> = extents
            .windows(2)
            .map(|p| p[1].0 - p[0].1)
            .filter(|g| *g > 0.0)
            .collect();
        gaps.sort_by(f64::total_cmp);
        let gap = gaps.get(gaps.len() / 2).copied();
        // Haut et bas des mots dans la bande.
        let ts: Vec<f64> = line
            .words
            .iter()
            .flat_map(|&i| {
                let word = &layer.words[i];
                [
                    word.bottom_left(),
                    word.bottom_right(),
                    word.top_right(),
                    word.top_left(),
                ]
            })
            .map(|(x, y)| {
                let (px, py) = self.image.placement.to_pixels(f64::from(x), f64::from(y));
                frame.to_line(px, py).1
            })
            .collect();
        let top = ts.iter().copied().fold(f64::INFINITY, f64::min);
        let bottom = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        LineGeometry {
            ink,
            extents,
            gap,
            top,
            bottom,
        }
    }

    /// Réglages mesurés de la ligne `line` et les polices candidates.
    pub fn match_style(
        &self,
        line: &ScanLine,
        layer: &TextLayer,
    ) -> (ScanStyle, Vec<ScanCandidate>) {
        let geometry = self.measure(line, layer);
        let catalog = fonts::catalog::shared();
        let found = fonts::match_line(&catalog, &geometry.ink.observation);
        let s = self.image.placement.scale as f32;
        let candidates: Vec<ScanCandidate> = found
            .candidates
            .iter()
            .map(|c| ScanCandidate {
                font: c.postscript_name.clone(),
                family: c.family.clone(),
                score: c.score,
                size: c.em / s,
                spacing: c.spacing / s,
                weight: c.weight,
                blur: c.blur / s,
                kerning: c.kerning,
            })
            .collect();
        let color = geometry.ink.color.map(|c| (c / 255.0).clamp(0.0, 1.0));
        // Grain de l'encre : celui du scan, mesuré sur le papier (au cœur des
        // traits, la dispersion vient surtout du flou).
        let noise = geometry
            .ink
            .ink_grain
            .min(geometry.ink.grain * 1.5)
            .min(40.0);
        let style = match candidates.first() {
            Some(best) => best.style(&ScanStyle {
                font: String::new(),
                family: String::new(),
                size: 0.0,
                spacing: 0.0,
                weight: 0.0,
                blur: 0.0,
                noise,
                color,
                kerning: true,
            }),
            None => fallback_style(&catalog, &geometry, s, color, noise),
        };
        (style, candidates)
    }
}

/// Réglages par défaut quand aucune police n'a pu être appariée (ligne trop
/// courte, caractères absents de toutes les polices) : police fournie, corps
/// tiré de la hauteur de l'encre.
fn fallback_style(
    catalog: &fonts::Catalog,
    geometry: &LineGeometry,
    scale: f32,
    color: [f32; 3],
    noise: f32,
) -> ScanStyle {
    let obs = &geometry.ink.observation;
    let (w, h) = (obs.width, obs.height);
    let top = (0..h)
        .find(|&y| (0..w).any(|x| obs.ink[y * w + x] >= 0.35))
        .unwrap_or(0) as f32;
    let cap = (obs.baseline - top).max(4.0);
    let face = catalog
        .find("Carlito-Regular")
        .or_else(|| (!catalog.faces().is_empty()).then_some(0));
    let (font, family) = face.map_or((String::new(), String::new()), |i| {
        let info = &catalog.faces()[i];
        (info.postscript_name.clone(), info.family.clone())
    });
    ScanStyle {
        font,
        family,
        size: cap / 0.7 / scale,
        spacing: 0.0,
        weight: 0.0,
        blur: 0.5 / scale,
        noise,
        color,
        kerning: true,
    }
}

/// Mesures d'une ligne : encre, emprise de chaque mot le long de la bande,
/// espace entre les mots, haut et bas des mots dans la bande.
pub(crate) struct LineGeometry {
    pub ink: LineInk,
    pub extents: Vec<(f64, f64)>,
    pub gap: Option<f64>,
    pub top: f64,
    pub bottom: f64,
}

/// Étendue d'un mot le long de la bande `frame`.
fn word_span(frame: &LineFrame, word: &LayerWord, placement: &Placement) -> (f64, f64) {
    let ss: Vec<f64> = [
        word.bottom_left(),
        word.bottom_right(),
        word.top_right(),
        word.top_left(),
    ]
    .iter()
    .map(|&(x, y)| {
        let (px, py) = placement.to_pixels(f64::from(x), f64::from(y));
        frame.to_line(px, py).0
    })
    .collect();
    (
        ss.iter().copied().fold(f64::INFINITY, f64::min),
        ss.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

/// Bande d'une ligne : suit la direction de ses mots, avec une marge.
fn line_frame(words: &[&PixelWord]) -> LineFrame {
    let first = words[0];
    let last = words[words.len() - 1];
    let start = first.corners[0];
    let end = last.corners[1];
    let (mut dx, mut dy) = (end.0 - start.0, end.1 - start.1);
    let len = dx.hypot(dy);
    if len < 1.0 {
        (dx, dy) = (1.0, 0.0);
    } else {
        (dx, dy) = (dx / len, dy / len);
    }
    let probe = LineFrame {
        origin: start,
        direction: (dx, dy),
        width: 0.0,
        height: 0.0,
    };
    let (mut s0, mut s1, mut t0, mut t1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for word in words {
        for &(x, y) in &word.corners {
            let (s, t) = probe.to_line(x, y);
            s0 = s0.min(s);
            s1 = s1.max(s);
            t0 = t0.min(t);
            t1 = t1.max(t);
        }
    }
    let height = (t1 - t0).max(1.0);
    let pad_t = 0.3 * height;
    let pad_s = 0.5 * height;
    let origin = probe.to_image(s0 - pad_s, t0 - pad_t);
    LineFrame {
        origin,
        direction: (dx, dy),
        width: s1 - s0 + 2.0 * pad_s,
        height: height + 2.0 * pad_t,
    }
}

/// Écart horizontal entre deux mots (pixels).
fn gap_between(a: &PixelWord, b: &PixelWord) -> f64 {
    f64::from(b.rect.x0 - a.rect.x1)
        .max(f64::from(a.rect.x0 - b.rect.x1))
        .max(0.0)
}

/// Inclinaison du texte (degrés) : médiane des pentes des mots de plus de
/// deux lettres.
fn skew(layer: &TextLayer, words: &[PixelWord]) -> f64 {
    let mut angles: Vec<f64> = layer
        .words
        .iter()
        .zip(words)
        .filter(|(word, _)| word.text.chars().count() >= 3)
        .map(|(_, w)| {
            let (a, b) = (w.corners[0], w.corners[1]);
            (b.1 - a.1).atan2(b.0 - a.0).to_degrees()
        })
        .filter(|a| a.abs() < 10.0)
        .collect();
    angles.sort_by(f64::total_cmp);
    angles.get(angles.len() / 2).copied().unwrap_or(0.0)
}

fn bounds(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    )
}

/// Redressement des pixels vers l'espace des tableaux : rotation autour du
/// centre de l'image, puis points, ordonnées vers le haut.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deskew {
    angle: f64,
    center: (f64, f64),
    scale: f64,
    height: f64,
}

impl Deskew {
    fn apply(&self, p: (f64, f64)) -> (f64, f64) {
        let (c, s) = (self.angle.cos(), self.angle.sin());
        let (x, y) = (p.0 - self.center.0, p.1 - self.center.1);
        let (rx, ry) = (x * c - y * s + self.center.0, x * s + y * c + self.center.1);
        (rx / self.scale, (self.height - ry) / self.scale)
    }

    fn invert(&self, p: (f64, f64)) -> (f64, f64) {
        let (rx, ry) = (p.0 * self.scale, self.height - p.1 * self.scale);
        let (c, s) = (self.angle.cos(), -self.angle.sin());
        let (x, y) = (rx - self.center.0, ry - self.center.1);
        (x * c - y * s + self.center.0, x * s + y * c + self.center.1)
    }

    /// Rectangle de pixels qui couvre un rectangle de l'espace redressé.
    pub fn pixel_rect(&self, rect: &layout::Rect) -> PixelRect {
        let corners = [
            (rect.x0, rect.y0),
            (rect.x1, rect.y0),
            (rect.x1, rect.y1),
            (rect.x0, rect.y1),
        ]
        .map(|p| self.invert(p));
        let (x0, y0, x1, y1) = bounds(&corners);
        PixelRect::covering(x0, y0, x1, y1)
    }
}

/// Numéros des objets d'un scan : empreinte de leur nature et de leur
/// emprise, stable d'une ouverture à l'autre.
#[derive(Default)]
struct Ids {
    taken: std::collections::HashSet<u32>,
}

impl Ids {
    fn next(&mut self, kind: &str, zone: &PixelRect, key: &str) -> u32 {
        let text = format!(
            "{kind}:{}:{}:{}:{}:{key}",
            zone.x0, zone.y0, zone.x1, zone.y1
        );
        let mut id = SCAN_ID | (fnv(text.as_bytes()) & (SCAN_ID - 1));
        while !self.taken.insert(id) {
            id = SCAN_ID | (id.wrapping_add(1) & (SCAN_ID - 1));
        }
        id
    }
}

/// Empreinte FNV-1a (stable, contrairement au hachage de la bibliothèque
/// standard).
pub(crate) fn fnv(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in bytes {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Empreinte du contenu d'une couche de texte, pour reconnaître une couche
/// déjà vue.
pub(crate) fn layer_key(layer: &TextLayer) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for word in &layer.words {
        mix(word.text.as_bytes());
        for v in word.quad {
            mix(&v.to_bits().to_le_bytes());
        }
        mix(&word.line.to_le_bytes());
    }
    hash
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn redressement_reversible() {
        let deskew = Deskew {
            angle: 0.02,
            center: (500.0, 700.0),
            scale: 300.0 / 72.0,
            height: 1400.0,
        };
        let p = deskew.apply((123.0, 456.0));
        let back = deskew.invert(p);
        assert!((back.0 - 123.0).abs() < 1e-9 && (back.1 - 456.0).abs() < 1e-9);
    }

    #[test]
    fn numeros_stables_et_distincts() {
        let mut ids = Ids::default();
        let zone = PixelRect::new(1, 2, 3, 4);
        let a = ids.next("line", &zone, "x");
        let b = ids.next("line", &zone, "x");
        assert!(is_scan_id(a) && is_scan_id(b) && a != b);
        assert_eq!(Ids::default().next("line", &zone, "x"), a);
    }
}
