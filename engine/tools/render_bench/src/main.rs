//! Banc d'essai des moteurs de rendu (cf. docs/RENDER_BENCHMARK.md).
//!
//! Mesure, pour chaque document : le temps d'ouverture (pages mesurées comprises),
//! le temps de rendu par page à une résolution donnée, les échecs, et l'écart
//! visuel entre MuPDF et PDFium quand les deux sont disponibles.
//!
//! Usage :
//!   render_bench [--pdfium DOSSIER] [--dpi 150] [--max-pages 20]
//!                [--out DOSSIER] [--anonymize] FICHIER_OU_DOSSIER...
//!
//! `--anonymize` remplace les noms de fichiers par doc-01, doc-02… dans le
//! rapport, pour pouvoir publier les mesures faites sur des documents privés.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;

struct Options {
    pdfium_dir: Option<PathBuf>,
    dpi: f32,
    max_pages: usize,
    out: Option<PathBuf>,
    anonymize: bool,
    inputs: Vec<PathBuf>,
}

/// Image RGBA rendue par l'un des moteurs.
struct Raster {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct EngineRun {
    opened: bool,
    open_time: Duration,
    pages: usize,
    render_times: Vec<Duration>,
    failures: usize,
    rasters: Vec<Option<Raster>>,
}

impl EngineRun {
    fn mean_ms(&self) -> Option<f64> {
        if self.render_times.is_empty() {
            return None;
        }
        let total: Duration = self.render_times.iter().sum();
        Some(total.as_secs_f64() * 1000.0 / self.render_times.len() as f64)
    }

    fn max_ms(&self) -> Option<f64> {
        self.render_times
            .iter()
            .max()
            .map(|d| d.as_secs_f64() * 1000.0)
    }
}

fn main() {
    let opts = match parse_args() {
        Ok(opts) => opts,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let files = collect_pdfs(&opts.inputs);
    if files.is_empty() {
        eprintln!("Aucun fichier PDF trouvé.");
        std::process::exit(2);
    }
    if let Some(out) = &opts.out {
        std::fs::create_dir_all(out).expect("création du dossier de sortie");
    }

    let pdfium = opts.pdfium_dir.as_ref().map(|dir| {
        let lib = Pdfium::pdfium_platform_library_name_at_path(dir);
        Pdfium::new(
            Pdfium::bind_to_library(&lib)
                .unwrap_or_else(|e| panic!("chargement de PDFium ({}) : {e}", lib.display())),
        )
    });

    println!(
        "Rendu à {} ppp, {} pages au plus par document. PDFium : {}.\n",
        opts.dpi,
        opts.max_pages,
        if pdfium.is_some() { "oui" } else { "non" }
    );
    println!("| Document | Pages | Ouverture MuPDF (ms) | Ouverture PDFium (ms) | Rendu MuPDF moy./max (ms) | Rendu PDFium moy./max (ms) | Écart moyen (/255) | Pixels divergents | Échecs MuPDF / PDFium |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");

    let mut all_mupdf = Vec::new();
    let mut all_pdfium = Vec::new();
    for (n, file) in files.iter().enumerate() {
        let label = if opts.anonymize {
            format!("doc-{:02}", n + 1)
        } else {
            file.file_name().unwrap().to_string_lossy().into_owned()
        };
        let password = password_for(file);
        let mupdf = run_mupdf(file, password, &opts);
        let pdfium_run = pdfium
            .as_ref()
            .map(|p| run_pdfium(p, file, password, &opts));

        let (diff, divergent) = match &pdfium_run {
            Some(p) => compare(&label, &mupdf, p, opts.out.as_deref()),
            None => (None, None),
        };
        println!(
            "| {label} | {} | {} | {} | {} | {} | {} | {} | {} / {} |",
            mupdf.pages,
            fmt_open(&mupdf),
            pdfium_run.as_ref().map_or("—".into(), fmt_open),
            fmt_render(&mupdf),
            pdfium_run.as_ref().map_or("—".into(), fmt_render),
            diff.map_or("—".into(), |d| format!("{d:.2}")),
            divergent.map_or("—".into(), |d| format!("{:.2} %", d * 100.0)),
            mupdf.failures,
            pdfium_run
                .as_ref()
                .map_or("—".into(), |p| p.failures.to_string()),
        );
        all_mupdf.extend(mupdf.render_times.iter().copied());
        if let Some(p) = pdfium_run {
            all_pdfium.extend(p.render_times.iter().copied());
        }
    }

    println!();
    println!("Rendu moyen toutes pages : MuPDF {}", fmt_mean(&all_mupdf));
    if pdfium.is_some() {
        println!(
            "Rendu moyen toutes pages : PDFium {}",
            fmt_mean(&all_pdfium)
        );
    }
}

fn run_mupdf(file: &Path, password: Option<&str>, opts: &Options) -> EngineRun {
    let mut run = EngineRun::default();
    let start = Instant::now();
    let doc = match pdf_core::Document::open_file(file, password) {
        Ok(doc) => doc,
        Err(_) => {
            run.failures += 1;
            return run;
        }
    };
    run.open_time = start.elapsed();
    run.opened = true;
    run.pages = doc.page_count() as usize;

    for index in 0..run.pages.min(opts.max_pages) {
        let start = Instant::now();
        let result = doc
            .page_content(index as u32)
            .and_then(|content| render::render_at_dpi(&content, opts.dpi));
        match result {
            Ok(bitmap) => {
                run.render_times.push(start.elapsed());
                run.rasters.push(Some(Raster {
                    width: bitmap.width,
                    height: bitmap.height,
                    rgba: bitmap.pixels,
                }));
            }
            Err(_) => {
                run.failures += 1;
                run.rasters.push(None);
            }
        }
    }
    run
}

fn run_pdfium(pdfium: &Pdfium, file: &Path, password: Option<&str>, opts: &Options) -> EngineRun {
    let mut run = EngineRun::default();
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(_) => {
            run.failures += 1;
            return run;
        }
    };
    // Ouverture comparable à celle de MuPDF : lecture et mesure de toutes les pages.
    let start = Instant::now();
    let doc = match pdfium.load_pdf_from_byte_slice(&bytes, password) {
        Ok(doc) => doc,
        Err(_) => {
            run.failures += 1;
            return run;
        }
    };
    let pages = doc.pages();
    run.pages = pages.len() as usize;
    for page in pages.iter() {
        let _ = (page.width().value, page.height().value);
    }
    run.open_time = start.elapsed();
    run.opened = true;

    let config = PdfRenderConfig::new()
        .scale_page_by_factor(opts.dpi / 72.0)
        .render_form_data(true);
    for index in 0..run.pages.min(opts.max_pages) {
        let start = Instant::now();
        let result = pages.get(index as PdfPageIndex).and_then(|page| {
            page.render_with_config(&config).map(|bitmap| Raster {
                width: bitmap.width() as u32,
                height: bitmap.height() as u32,
                rgba: bitmap.as_rgba_bytes(),
            })
        });
        match result {
            Ok(raster) => {
                run.render_times.push(start.elapsed());
                run.rasters.push(Some(raster));
            }
            Err(_) => {
                run.failures += 1;
                run.rasters.push(None);
            }
        }
    }
    run
}

/// Écart moyen par canal et part des pixels dont un canal diffère de plus de 40,
/// sur la zone commune des deux rendus (ils peuvent différer d'un pixel par arrondi).
fn compare(
    label: &str,
    a: &EngineRun,
    b: &EngineRun,
    out: Option<&Path>,
) -> (Option<f64>, Option<f64>) {
    let mut sum = 0u64;
    let mut channels = 0u64;
    let mut divergent = 0u64;
    let mut pixels = 0u64;
    let mut worst: Option<(f64, usize)> = None;

    for (index, (ra, rb)) in a.rasters.iter().zip(&b.rasters).enumerate() {
        let (Some(ra), Some(rb)) = (ra, rb) else {
            continue;
        };
        let w = ra.width.min(rb.width) as usize;
        let h = ra.height.min(rb.height) as usize;
        let mut page_sum = 0u64;
        for y in 0..h {
            let row_a = &ra.rgba[y * ra.width as usize * 4..][..w * 4];
            let row_b = &rb.rgba[y * rb.width as usize * 4..][..w * 4];
            for (pa, pb) in row_a.chunks_exact(4).zip(row_b.chunks_exact(4)) {
                let mut max = 0;
                for c in 0..3 {
                    let d = pa[c].abs_diff(pb[c]);
                    page_sum += d as u64;
                    max = max.max(d);
                }
                if max > 40 {
                    divergent += 1;
                }
            }
        }
        sum += page_sum;
        channels += (w * h * 3) as u64;
        pixels += (w * h) as u64;
        let page_mean = page_sum as f64 / (w * h * 3).max(1) as f64;
        if worst.is_none_or(|(m, _)| page_mean > m) {
            worst = Some((page_mean, index));
        }
    }

    if let (Some(out), Some((_, index))) = (out, worst) {
        save_side_by_side(out, label, index, a, b);
    }
    if channels == 0 {
        return (None, None);
    }
    (
        Some(sum as f64 / channels as f64),
        Some(divergent as f64 / pixels as f64),
    )
}

fn save_side_by_side(out: &Path, label: &str, index: usize, a: &EngineRun, b: &EngineRun) {
    let (Some(Some(ra)), Some(Some(rb))) = (a.rasters.get(index), b.rasters.get(index)) else {
        return;
    };
    let width = ra.width + rb.width;
    let height = ra.height.max(rb.height);
    let mut canvas = image::RgbaImage::from_pixel(width, height, image::Rgba([128, 128, 128, 255]));
    for (raster, x0) in [(ra, 0), (rb, ra.width)] {
        let img = image::RgbaImage::from_raw(raster.width, raster.height, raster.rgba.clone())
            .expect("dimensions cohérentes");
        image::imageops::overlay(&mut canvas, &img, x0 as i64, 0);
    }
    let path = out.join(format!("{label}-page{:03}-mupdf-pdfium.png", index + 1));
    if let Err(e) = canvas.save(&path) {
        eprintln!("écriture de {} : {e}", path.display());
    }
}

fn fmt_open(run: &EngineRun) -> String {
    if run.opened {
        format!("{:.1}", run.open_time.as_secs_f64() * 1000.0)
    } else {
        "échec".into()
    }
}

fn fmt_render(run: &EngineRun) -> String {
    match (run.mean_ms(), run.max_ms()) {
        (Some(mean), Some(max)) => format!("{mean:.1} / {max:.1}"),
        _ => "—".into(),
    }
}

fn fmt_mean(times: &[Duration]) -> String {
    if times.is_empty() {
        return "—".into();
    }
    let total: Duration = times.iter().sum();
    format!(
        "{:.1} ms sur {} pages",
        total.as_secs_f64() * 1000.0 / times.len() as f64,
        times.len()
    )
}

/// Mot de passe des fichiers de test chiffrés.
fn password_for(file: &Path) -> Option<&'static str> {
    (file.file_name()? == "chiffre.pdf").then_some("motdepasse")
}

fn collect_pdfs(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(input)
                .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
                .unwrap_or_default();
            entries.sort();
            files.extend(entries.into_iter().filter(|p| is_pdf(p)));
        } else if is_pdf(input) {
            files.push(input.clone());
        }
    }
    files
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        pdfium_dir: None,
        dpi: 150.0,
        max_pages: 20,
        out: None,
        anonymize: false,
        inputs: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} attend une valeur"));
        match arg.as_str() {
            "--pdfium" => opts.pdfium_dir = Some(value("--pdfium")?.into()),
            "--dpi" => opts.dpi = value("--dpi")?.parse().map_err(|_| "--dpi invalide")?,
            "--max-pages" => {
                opts.max_pages = value("--max-pages")?
                    .parse()
                    .map_err(|_| "--max-pages invalide")?
            }
            "--out" => opts.out = Some(value("--out")?.into()),
            "--anonymize" => opts.anonymize = true,
            "-h" | "--help" => {
                return Err(
                    "render_bench [--pdfium DOSSIER] [--dpi 150] [--max-pages 20] \
                            [--out DOSSIER] [--anonymize] FICHIER_OU_DOSSIER..."
                        .into(),
                )
            }
            _ => opts.inputs.push(arg.into()),
        }
    }
    Ok(opts)
}
