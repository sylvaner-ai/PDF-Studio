//! Banc d'essai du mode Amélioration (cf. docs/AMELIORATION.md).
//!
//! Chaque page du jeu de scans `tests/fixtures/amelioration` est améliorée
//! comme dans l'application (`EditSession::enhance_page`), puis comparée à sa
//! vérité terrain : inclinaison mesurée et corrigée, écart à la vérité,
//! inclinaison résiduelle de la page redressée (mesurée de nouveau sur son
//! rendu, mise à l'endroit), sens trouvé, conclusion et durée. Les pages
//! améliorées sont ensuite appliquées et le document exporté, pour relever
//! la taille du fichier. Le lot de 50 pages est traité à la suite, page par
//! page, pour le temps total.
//!
//! Usage :
//!   enhance_bench [DOSSIER_DU_JEU] [--out RAPPORT.md]
//!
//! À lancer en mode release pour des durées représentatives.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ocr::{preprocess, GrayImage};
use pdfstudio_ffi::{
    default_enhancement_settings, EditSession, EnhancedPage, PdfDocument, RenderedPage,
    StraighteningStatus,
};
use serde_json::Value;

/// Une page mesurée.
struct Row {
    file: String,
    page: u32,
    truth_angle: Option<f64>,
    truth_turns: u8,
    expected_check: bool,
    measured: Option<f64>,
    corrected: f64,
    residual: Option<f64>,
    turns: u8,
    status: StraighteningStatus,
    elapsed: Duration,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/amelioration");
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            _ => dir = PathBuf::from(arg),
        }
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} : {e}", dir.display()))
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".pdf").map(str::to_owned)
        })
        .collect();
    names.sort();

    let mut rows = Vec::new();
    let mut sizes = Vec::new();
    let mut batch = None;
    for name in &names {
        let truth = read_json(&dir.join(format!("{name}.json")));
        let document = open(&dir.join(format!("{name}.pdf")));
        if truth["pages"][0].get("fichier").is_some() {
            batch = Some(run_batch(&dir, name, &document, &truth));
            continue;
        }
        let session = EditSession::new(Arc::clone(&document));
        let mut enhanced = Vec::new();
        for (position, page) in session.pages().iter().enumerate() {
            let expected = &truth["pages"][position];
            let (row, page) = measure(&session, page.id, name, position as u32, expected);
            rows.push(row);
            enhanced.push(page);
        }
        sizes.push((
            name.clone(),
            file_size(&dir, name),
            exported_size(&session, enhanced),
        ));
    }

    let report = report(&rows, &sizes, batch);
    print!("{report}");
    if let Some(out) = out {
        std::fs::write(&out, report).unwrap_or_else(|e| panic!("{} : {e}", out.display()));
    }
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} : {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn open(path: &Path) -> Arc<PdfDocument> {
    PdfDocument::open(path.to_string_lossy().into_owned(), None)
        .unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

fn file_size(dir: &Path, name: &str) -> u64 {
    std::fs::metadata(dir.join(format!("{name}.pdf"))).map_or(0, |m| m.len())
}

/// Améliore la page `id` et la compare à sa vérité terrain `expected`.
fn measure(
    session: &EditSession,
    id: u64,
    file: &str,
    page: u32,
    expected: &Value,
) -> (Row, Arc<EnhancedPage>) {
    let start = Instant::now();
    let enhanced = session
        .enhance_page(id, default_enhancement_settings(), None)
        .expect("amélioration");
    let elapsed = start.elapsed();
    let report = enhanced.report();
    let dpi = expected["ppp"].as_f64().unwrap_or(200.0) as f32;
    let residual = report.changes_pixels.then(|| {
        let rendered = enhanced
            .render(dpi / 72.0, i32::from(report.quarter_turns) * 90)
            .expect("rendu");
        skew(&rendered, dpi)
    });
    let row = Row {
        file: file.to_owned(),
        page: page + 1,
        truth_angle: expected["inclinaison_deg"].as_f64(),
        truth_turns: (4 - expected["quart_de_tour"].as_u64().unwrap_or(0) as u8) % 4,
        expected_check: expected["a_verifier"].as_bool().unwrap_or(false),
        measured: report.measured_angle.map(f64::from),
        corrected: f64::from(report.corrected_angle),
        residual,
        turns: report.quarter_turns,
        status: report.status,
        elapsed,
    };
    (row, enhanced)
}

/// Inclinaison des lignes de texte de la page rendue, en degrés.
fn skew(page: &RenderedPage, dpi: f32) -> f64 {
    let image = GrayImage::from_rgba(page.width, page.height, page.bytes_per_row, &page.pixels);
    let image = preprocess::flatten_background(&image, dpi);
    preprocess::measure_skew(&image, dpi, 45.0).degrees
}

/// Taille du document exporté, pages améliorées appliquées.
fn exported_size(session: &EditSession, pages: Vec<Arc<EnhancedPage>>) -> u64 {
    let source = session
        .enhanced_document(pages.clone())
        .expect("document des pages améliorées")
        .map(|document| session.add_source(document));
    session
        .apply_enhancements(pages, source)
        .expect("application");
    session.export_pdf().expect("export").bytes.len() as u64
}

/// Lot : chaque page améliorée à la suite ; temps total et pages conformes.
fn run_batch(
    dir: &Path,
    name: &str,
    document: &Arc<PdfDocument>,
    truth: &Value,
) -> (String, usize, usize, Duration) {
    let session = EditSession::new(Arc::clone(document));
    let pages = session.pages();
    let start = Instant::now();
    let mut conform = 0;
    for (position, page) in pages.iter().enumerate() {
        let origin = &truth["pages"][position];
        let file = origin["fichier"].as_str().unwrap().trim_end_matches(".pdf");
        let index = origin["page"].as_u64().unwrap() as usize - 1;
        let expected = &read_json(&dir.join(format!("{file}.json")))["pages"][index];
        let report = session
            .enhance_page(page.id, default_enhancement_settings(), None)
            .expect("amélioration")
            .report();
        let angle = expected["inclinaison_deg"].as_f64().unwrap_or(0.0);
        if (f64::from(report.corrected_angle) - angle).abs() < 0.2 {
            conform += 1;
        }
    }
    (name.to_owned(), pages.len(), conform, start.elapsed())
}

fn status(status: StraighteningStatus) -> &'static str {
    match status {
        StraighteningStatus::Straight => "droite",
        StraighteningStatus::Corrected => "corrigée",
        StraighteningStatus::ToCheck => "à vérifier",
        StraighteningStatus::Unmeasurable => "non mesurable",
        StraighteningStatus::Disabled => "désactivé",
        StraighteningStatus::NotScanned => "pas un scan",
        StraighteningStatus::Retouched => "retouchée",
    }
}

fn degrees(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |v| format!("{v:+.2}°").replace('.', ","))
}

fn report(
    rows: &[Row],
    sizes: &[(String, u64, u64)],
    batch: Option<(String, usize, usize, Duration)>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "| Fichier | Page | Vérité | Mesurée | Corrigée | Écart | Résidu | Sens (attendu) | Conclusion | Durée |");
    let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|---|---:|");
    let mut worst_error: f64 = 0.0;
    let mut worst_residual: f64 = 0.0;
    let mut failures = Vec::new();
    for row in rows {
        let error = row
            .truth_angle
            .filter(|_| row.status == StraighteningStatus::Corrected)
            .map(|t| row.corrected - t);
        if let Some(error) = error {
            worst_error = worst_error.max(error.abs());
        }
        if let Some(residual) = row.residual {
            worst_residual = worst_residual.max(residual.abs());
        }
        let ok = row.truth_angle.is_none_or(|truth| {
            if row.expected_check {
                row.status == StraighteningStatus::ToCheck
            } else if truth.abs() < 0.1 {
                row.corrected == 0.0
            } else {
                error.is_some_and(|e| e.abs() < 0.2) && row.residual.is_some_and(|r| r.abs() < 0.2)
            }
        }) && row.turns == row.truth_turns;
        if !ok {
            failures.push(format!("{} p{}", row.file, row.page));
        }
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} ({}) | {} | {} ms |",
            row.file,
            row.page,
            degrees(row.truth_angle),
            degrees(row.measured),
            degrees(Some(row.corrected)),
            degrees(error),
            degrees(row.residual),
            row.turns,
            row.truth_turns,
            status(row.status),
            row.elapsed.as_millis()
        );
    }
    let total: Duration = rows.iter().map(|row| row.elapsed).sum();
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Écart maximal à la vérité : {}. Résidu maximal : {}. Durée moyenne : {} ms par page.",
        degrees(Some(worst_error)).trim_start_matches('+'),
        degrees(Some(worst_residual)).trim_start_matches('+'),
        total.as_millis() / rows.len().max(1) as u128
    );
    let _ = writeln!(
        out,
        "Pages hors critères : {}.",
        if failures.is_empty() {
            "aucune".to_owned()
        } else {
            failures.join(", ")
        }
    );
    if let Some((name, count, conform, elapsed)) = batch {
        let _ = writeln!(
            out,
            "Lot `{name}` : {conform}/{count} pages conformes, {:.1} s au total, à la suite.",
            elapsed.as_secs_f64()
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "| Fichier | Taille d'origine | Après redressement |");
    let _ = writeln!(out, "|---|---:|---:|");
    for (name, before, after) in sizes {
        let _ = writeln!(
            out,
            "| {name} | {} Ko | {} Ko |",
            before / 1024,
            after / 1024
        );
    }
    out
}
