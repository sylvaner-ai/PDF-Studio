//! Banc d'essai des moteurs de reconnaissance de texte (cf. docs/OCR_BENCHMARK.md).
//!
//! Pour chaque page d'un document natif, le texte d'origine et l'emprise de
//! chaque ligne servent de référence : la page est rastérisée, dégradée selon
//! plusieurs profils de numérisation (propre, dégradé, incliné, retourné,
//! 150 dpi), puis lue par chaque moteur, avec ou sans prétraitement. Le taux
//! de caractères corrects est mesuré ligne par ligne (`ocr::metrics`), ainsi
//! que le temps de lecture et la pertinence de la confiance annoncée. Les
//! pages sans texte (vrais scans) sont lues sans référence : seuls le temps,
//! le nombre de mots et la confiance sont relevés.
//!
//! Usage :
//!   ocr_bench [--engines vision,tesseract,ocrs] [--variants brut,préparé,binarisé]
//!             [--profiles propre,dégradé,incliné,retourné,couché,150] [--max-pages 1]
//!             [--vision PROGRAMME] [--tesseract PROGRAMME] [--tessdata DOSSIER]
//!             [--ocrs-models DOSSIER] [--out RAPPORT.md] [--csv DETAIL.csv]
//!             FICHIER_OU_DOSSIER... [--private FICHIER_OU_DOSSIER...]
//!
//! Les documents donnés après `--private` sont anonymisés dans le rapport
//! (« privé-01 »…) ; aucune image n'en est conservée.

mod degrade;
mod engines;
mod report;

use std::path::{Path, PathBuf};
use std::time::Instant;

use ocr::metrics::{self, Score, TruthLine, WordOutcome};
use ocr::{GrayImage, OcrEngine, Options as OcrOptions};

use degrade::Profile;
use engines::{Ocrs, Tesseract, Vision};

/// Langues attendues, dans l'ordre de préférence.
const LANGUAGES: [&str; 2] = ["fr-FR", "en-US"];

/// Prétraitement appliqué avant la lecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Image transmise telle quelle.
    Raw,
    /// Fond égalisé, page redressée et remise à l'endroit.
    Prepared,
    /// Comme `Prepared`, plus filtre médian et binarisation.
    Binarized,
}

impl Variant {
    const ALL: [Variant; 3] = [Variant::Raw, Variant::Prepared, Variant::Binarized];

    fn name(self) -> &'static str {
        match self {
            Variant::Raw => "brut",
            Variant::Prepared => "préparé",
            Variant::Binarized => "binarisé",
        }
    }

    fn parse(name: &str) -> Option<Variant> {
        match name {
            "brut" => Some(Variant::Raw),
            "préparé" | "prepare" => Some(Variant::Prepared),
            "binarisé" | "binarise" => Some(Variant::Binarized),
            _ => None,
        }
    }

    fn options(self, dpi: f32) -> OcrOptions {
        let languages = LANGUAGES.iter().map(|l| (*l).to_owned()).collect();
        match self {
            Variant::Raw => OcrOptions::raw(dpi, languages),
            Variant::Prepared => OcrOptions {
                denoise: false,
                binarize: false,
                ..OcrOptions::full(dpi, languages)
            },
            Variant::Binarized => OcrOptions::full(dpi, languages),
        }
    }
}

struct Options {
    engines: Vec<String>,
    variants: Vec<Variant>,
    profiles: Vec<Profile>,
    max_pages: usize,
    vision: Option<PathBuf>,
    tesseract: PathBuf,
    tessdata: Option<PathBuf>,
    ocrs_models: Option<PathBuf>,
    out: Option<PathBuf>,
    csv: Option<PathBuf>,
    public: Vec<PathBuf>,
    private: Vec<PathBuf>,
}

/// Une lecture d'un scan simulé, comparée à sa référence.
pub struct SimulatedRun {
    /// Nom du document (anonymisé s'il est privé).
    pub document: String,
    /// Le document est privé.
    pub private: bool,
    /// Page, à partir de 1.
    pub page: usize,
    /// Profil de numérisation.
    pub profile: Profile,
    /// Moteur.
    pub engine: String,
    /// Prétraitement.
    pub variant: Variant,
    /// Durée de la lecture, prétraitement compris, en millisecondes.
    pub millis: f64,
    /// Erreurs comptées.
    pub score: Score,
    /// Confiance et justesse de chaque mot.
    pub outcomes: Vec<WordOutcome>,
    /// La page a été remise à l'endroit comme il le fallait.
    pub orientation_ok: bool,
    /// Inclinaison mesurée, en degrés.
    pub skew: f64,
}

/// Une lecture d'un vrai scan, sans référence.
pub struct ScanRun {
    /// Nom du document (anonymisé s'il est privé).
    pub document: String,
    /// Page, à partir de 1.
    pub page: usize,
    /// Moteur.
    pub engine: String,
    /// Prétraitement.
    pub variant: Variant,
    /// Durée, en millisecondes.
    pub millis: f64,
    /// Mots lus.
    pub words: usize,
    /// Confiance moyenne.
    pub confidence: f32,
    /// Quarts de tour appliqués pour lire la page.
    pub quarter_turns: u8,
}

fn main() {
    let opts = match parse_args() {
        Ok(opts) => opts,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let mut available: Vec<Engine> = Vec::new();
    let mut versions: Vec<(String, String)> = Vec::new();
    for name in &opts.engines {
        match name.as_str() {
            "vision" => match &opts.vision {
                Some(program) => {
                    let vision = Vision::new(program.clone());
                    versions.push(("Vision".into(), vision.version()));
                    available.push(Engine::Vision(vision));
                }
                None => eprintln!("Vision ignoré : --vision non précisé"),
            },
            "tesseract" => {
                let tesseract = Tesseract::new(opts.tesseract.clone(), opts.tessdata.clone());
                let version = tesseract.version();
                if version == "absent" {
                    eprintln!("Tesseract ignoré : programme introuvable");
                    continue;
                }
                versions.push(("Tesseract".into(), version));
                available.push(Engine::Tesseract(tesseract));
            }
            "ocrs" => match opts.ocrs_models.as_deref().map(Ocrs::load) {
                Some(Ok(engine)) => {
                    versions.push(("ocrs".into(), "0.13 (modèles ocrs-models)".into()));
                    available.push(Engine::Ocrs(engine));
                }
                Some(Err(err)) => eprintln!("ocrs ignoré : {err}"),
                None => eprintln!("ocrs ignoré : --ocrs-models non précisé"),
            },
            other => eprintln!("moteur inconnu : {other}"),
        }
    }
    if available.is_empty() {
        eprintln!("Aucun moteur disponible.");
        std::process::exit(1);
    }

    let mut documents: Vec<(PathBuf, String, bool)> = collect_pdfs(&opts.public)
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (path, name, false)
        })
        .collect();
    for (i, path) in collect_pdfs(&opts.private).into_iter().enumerate() {
        documents.push((path, format!("privé-{:02}", i + 1), true));
    }
    if documents.is_empty() {
        eprintln!("Aucun PDF trouvé.");
        std::process::exit(1);
    }

    let mut simulated: Vec<SimulatedRun> = Vec::new();
    let mut scans: Vec<ScanRun> = Vec::new();
    for (doc_index, (path, name, private)) in documents.iter().enumerate() {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("{name} : illisible ({err})");
                continue;
            }
        };
        let mut document = match pdf_core::Document::from_bytes(&bytes, None) {
            Ok(document) => document,
            Err(err) => {
                eprintln!("{name} : {err}");
                continue;
            }
        };
        let pages = (document.page_count() as usize).min(opts.max_pages);
        for page in 0..pages {
            let index = page as u32;
            let lines = document.text_lines(index).unwrap_or_default();
            if lines.is_empty() {
                if !document.needs_recognition(index).unwrap_or(false) {
                    continue;
                }
                let Some(image) = render(&document, index, 300.0) else {
                    continue;
                };
                for engine in &available {
                    engine.set_dpi(300.0);
                    let engine = engine.get();
                    for &variant in opts.variants.iter().filter(|v| **v != Variant::Raw) {
                        let start = Instant::now();
                        let Ok(result) = ocr::recognize(engine, &image, &variant.options(300.0))
                        else {
                            eprintln!("{name} p. {} : échec de {}", page + 1, engine.name());
                            continue;
                        };
                        let run = ScanRun {
                            document: name.clone(),
                            page: page + 1,
                            engine: engine.name(),
                            variant,
                            millis: start.elapsed().as_secs_f64() * 1000.0,
                            words: result.words.len(),
                            confidence: result.mean_confidence(),
                            quarter_turns: result.quarter_turns,
                        };
                        eprintln!(
                            "{name} p. {} [scan] {} {} : {} mots, {:.0} ms",
                            page + 1,
                            run.engine,
                            variant.name(),
                            run.words,
                            run.millis
                        );
                        scans.push(run);
                    }
                }
                continue;
            }

            for &profile in &opts.profiles {
                let dpi = profile.dpi();
                let Some(image) = render(&document, index, dpi) else {
                    continue;
                };
                let scale = dpi / 72.0;
                let truth: Vec<TruthLine> = lines
                    .iter()
                    .map(|line| TruthLine {
                        text: line.text.clone(),
                        bounds: (
                            line.rect.x * scale,
                            line.rect.y * scale,
                            (line.rect.x + line.rect.width) * scale,
                            (line.rect.y + line.rect.height) * scale,
                        ),
                    })
                    .collect();
                let seed = (doc_index as u64) << 32 | (page as u64) << 8 | profile as u64;
                let (scan, to_upright) = degrade::degrade(&image, profile, seed);
                let expected_turns = profile.expected_turns();

                for engine in &available {
                    engine.set_dpi(dpi);
                    let engine = engine.get();
                    for &variant in &opts.variants {
                        let start = Instant::now();
                        let result = match ocr::recognize(engine, &scan, &variant.options(dpi)) {
                            Ok(result) => result,
                            Err(err) => {
                                eprintln!("{name} p. {} : {} — {err}", page + 1, engine.name());
                                continue;
                            }
                        };
                        let millis = start.elapsed().as_secs_f64() * 1000.0;
                        let words: Vec<_> = result
                            .words
                            .iter()
                            .cloned()
                            .map(|mut word| {
                                word.quad = word.quad.map(|p| to_upright.apply_point(p));
                                word
                            })
                            .collect();
                        let (score, outcomes) = metrics::score(&truth, &words);
                        eprintln!(
                            "{name} p. {} [{}] {} {} : {:.2} % ({} car.), {:.0} ms",
                            page + 1,
                            profile.name(),
                            engine.name(),
                            variant.name(),
                            score.char_accuracy() * 100.0,
                            score.chars,
                            millis
                        );
                        simulated.push(SimulatedRun {
                            document: name.clone(),
                            private: *private,
                            page: page + 1,
                            profile,
                            engine: engine.name(),
                            variant,
                            millis,
                            score,
                            outcomes,
                            orientation_ok: result.quarter_turns == expected_turns,
                            skew: result.skew_degrees,
                        });
                    }
                }
            }
        }
    }

    let report = report::markdown(&versions, &opts.profiles, &simulated, &scans);
    match &opts.out {
        Some(path) => {
            if let Err(err) = std::fs::write(path, &report) {
                eprintln!("écriture du rapport : {err}");
            }
        }
        None => print!("{report}"),
    }
    if let Some(path) = &opts.csv {
        if let Err(err) = std::fs::write(path, report::csv(&simulated)) {
            eprintln!("écriture du détail : {err}");
        }
    }
}

/// Un des moteurs comparés.
enum Engine {
    Vision(Vision),
    Tesseract(Tesseract),
    Ocrs(Ocrs),
}

impl Engine {
    fn get(&self) -> &dyn OcrEngine {
        match self {
            Engine::Vision(engine) => engine,
            Engine::Tesseract(engine) => engine,
            Engine::Ocrs(engine) => engine,
        }
    }

    /// Transmet la résolution à Tesseract, qui ne la lit pas dans l'image ;
    /// les autres moteurs n'en ont pas besoin.
    fn set_dpi(&self, dpi: f32) {
        if let Engine::Tesseract(tesseract) = self {
            tesseract.dpi.set(dpi);
        }
    }
}

/// Image en niveaux de gris d'une page, à la résolution donnée.
fn render(document: &pdf_core::Document, index: u32, dpi: f32) -> Option<GrayImage> {
    let content = document.page_content(index).ok()?;
    let bitmap = render::render_at_dpi(&content, dpi).ok()?;
    Some(GrayImage::from_rgba(
        bitmap.width,
        bitmap.height,
        bitmap.bytes_per_row,
        &bitmap.pixels,
    ))
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

fn parse_list<T>(value: &str, parse: impl Fn(&str) -> Option<T>) -> Result<Vec<T>, String> {
    value
        .split(',')
        .map(|item| parse(item.trim()).ok_or(format!("valeur inconnue : {item}")))
        .collect()
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        engines: vec!["vision".into(), "tesseract".into(), "ocrs".into()],
        variants: Variant::ALL.to_vec(),
        profiles: Profile::ALL.to_vec(),
        max_pages: 1,
        vision: None,
        tesseract: "tesseract".into(),
        tessdata: None,
        ocrs_models: None,
        out: None,
        csv: None,
        public: Vec::new(),
        private: Vec::new(),
    };
    let mut private = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} attend une valeur"));
        match arg.as_str() {
            "--engines" => {
                opts.engines = value("--engines")?
                    .split(',')
                    .map(|e| e.trim().to_lowercase())
                    .collect()
            }
            "--variants" => opts.variants = parse_list(&value("--variants")?, Variant::parse)?,
            "--profiles" => opts.profiles = parse_list(&value("--profiles")?, Profile::parse)?,
            "--max-pages" => {
                opts.max_pages = value("--max-pages")?
                    .parse()
                    .map_err(|_| "--max-pages invalide")?
            }
            "--vision" => opts.vision = Some(value("--vision")?.into()),
            "--tesseract" => opts.tesseract = value("--tesseract")?.into(),
            "--tessdata" => opts.tessdata = Some(value("--tessdata")?.into()),
            "--ocrs-models" => opts.ocrs_models = Some(value("--ocrs-models")?.into()),
            "--out" => opts.out = Some(value("--out")?.into()),
            "--csv" => opts.csv = Some(value("--csv")?.into()),
            "--private" => private = true,
            "-h" | "--help" => {
                return Err("ocr_bench [--engines vision,tesseract,ocrs] \
                            [--variants brut,préparé,binarisé] \
                            [--profiles propre,dégradé,incliné,retourné,couché,150] [--max-pages 1] \
                            [--vision PROGRAMME] [--tesseract PROGRAMME] [--tessdata DOSSIER] \
                            [--ocrs-models DOSSIER] [--out RAPPORT.md] [--csv DETAIL.csv] \
                            FICHIER_OU_DOSSIER... [--private FICHIER_OU_DOSSIER...]"
                    .into())
            }
            _ if private => opts.private.push(arg.into()),
            _ => opts.public.push(arg.into()),
        }
    }
    Ok(opts)
}
