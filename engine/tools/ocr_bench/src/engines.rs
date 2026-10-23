//! Les trois moteurs comparés, derrière le trait `ocr::OcrEngine` :
//!
//! - Vision d'Apple, par un petit programme Swift (`vision/main.swift`) qui
//!   partage le code de l'application (`app/Recognition/VisionTextRecognizer.swift`) ;
//! - Tesseract, par sa commande `tesseract` (sortie TSV) ;
//! - `ocrs`, moteur Rust à réseaux de neurones (runtime rten), dans ce processus.
//!
//! Les deux moteurs externes reçoivent l'image dans un fichier PNG temporaire,
//! effacé aussitôt après la lecture ; leur temps de lecture comprend donc le
//! lancement du programme et le chargement de ses modèles.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use ocr::{GrayImage, OcrEngine, OcrError, Point, Quad, RecognizedWord};

/// Fichier PNG temporaire, effacé à la fin de sa portée.
struct TempPng(PathBuf);

impl TempPng {
    fn write(image: &GrayImage) -> Result<Self, OcrError> {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ocr_bench_{}_{n}.png", std::process::id()));
        let buffer = image::GrayImage::from_raw(image.width, image.height, image.pixels.clone())
            .ok_or_else(|| OcrError::Failed("dimensions incohérentes".into()))?;
        buffer
            .save(&path)
            .map_err(|err| OcrError::Failed(format!("écriture de l'image : {err}")))?;
        Ok(Self(path))
    }
}

impl Drop for TempPng {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(command: &mut Command) -> Result<String, OcrError> {
    let output = command
        .output()
        .map_err(|err| OcrError::Unavailable(format!("{err}")))?;
    if !output.status.success() {
        return Err(OcrError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// MARK: - Tesseract

/// Tesseract, par sa commande.
pub struct Tesseract {
    program: PathBuf,
    tessdata: Option<PathBuf>,
    /// Résolution de l'image en cours, transmise à Tesseract.
    pub dpi: Cell<f32>,
}

impl Tesseract {
    /// Moteur lancé par `program`, avec ses modèles dans `tessdata` (sinon ceux
    /// de l'installation).
    pub fn new(program: PathBuf, tessdata: Option<PathBuf>) -> Self {
        Self {
            program,
            tessdata,
            dpi: Cell::new(300.0),
        }
    }

    /// Version, première ligne de `tesseract --version`.
    pub fn version(&self) -> String {
        run(Command::new(&self.program).arg("--version"))
            .ok()
            .and_then(|out| out.lines().next().map(str::to_owned))
            .unwrap_or_else(|| "absent".into())
    }
}

impl OcrEngine for Tesseract {
    fn name(&self) -> String {
        "Tesseract".into()
    }

    fn recognize(
        &self,
        image: &GrayImage,
        languages: &[String],
    ) -> Result<Vec<RecognizedWord>, OcrError> {
        let png = TempPng::write(image)?;
        let langs: Vec<&str> = languages
            .iter()
            .filter_map(|l| match l.split('-').next() {
                Some("fr") => Some("fra"),
                Some("en") => Some("eng"),
                _ => None,
            })
            .collect();
        let mut command = Command::new(&self.program);
        command
            .arg(&png.0)
            .arg("stdout")
            .args(["-l", &langs.join("+")])
            .args(["--oem", "1", "--psm", "3"])
            .args(["--dpi", &format!("{}", self.dpi.get().round())]);
        if let Some(dir) = &self.tessdata {
            command.arg("--tessdata-dir").arg(dir);
        }
        // Sortie TSV demandée par un paramètre : l'argument `tsv` exige le
        // dossier `configs` de Tesseract, absent d'un dossier de modèles seul.
        command.args(["-c", "tessedit_create_tsv=1"]);
        Ok(parse_tsv(&run(&mut command)?))
    }
}

/// Mots d'une sortie TSV de Tesseract (niveau 5).
fn parse_tsv(tsv: &str) -> Vec<RecognizedWord> {
    let mut lines: HashMap<(u32, u32, u32, u32), u32> = HashMap::new();
    let mut words = Vec::new();
    for row in tsv.lines().skip(1) {
        let fields: Vec<&str> = row.split('\t').collect();
        if fields.len() < 12 || fields[0] != "5" {
            continue;
        }
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let n = |i: usize| fields[i].parse::<f32>().unwrap_or(0.0);
        let key = (n(1) as u32, n(2) as u32, n(3) as u32, n(4) as u32);
        let next = lines.len() as u32;
        let line = *lines.entry(key).or_insert(next);
        let (left, top, width, height) = (n(6), n(7), n(8), n(9));
        words.push(RecognizedWord {
            text: text.to_owned(),
            quad: Quad::from_rect(left, top, left + width, top + height),
            confidence: (n(10) / 100.0).clamp(0.0, 1.0),
            line,
        });
    }
    words
}

// MARK: - ocrs

/// `ocrs`, dans ce processus.
pub struct Ocrs {
    engine: ocrs::OcrEngine,
}

impl Ocrs {
    /// Charge les modèles de détection et de reconnaissance de `dir`
    /// (`text-detection.rten`, `text-recognition.rten`).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let load = |name: &str| {
            rten::Model::load_file(dir.join(name)).map_err(|err| format!("{name} : {err}"))
        };
        let engine = ocrs::OcrEngine::new(ocrs::OcrEngineParams {
            detection_model: Some(load("text-detection.rten")?),
            recognition_model: Some(load("text-recognition.rten")?),
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;
        Ok(Self { engine })
    }
}

impl OcrEngine for Ocrs {
    fn name(&self) -> String {
        "ocrs".into()
    }

    fn recognize(
        &self,
        image: &GrayImage,
        _languages: &[String],
    ) -> Result<Vec<RecognizedWord>, OcrError> {
        use ocrs::TextItem;
        let fail = |err: &dyn std::fmt::Display| OcrError::Failed(err.to_string());
        let source = ocrs::ImageSource::from_bytes(&image.pixels, (image.width, image.height))
            .map_err(|err| fail(&err))?;
        let input = self
            .engine
            .prepare_input(source)
            .map_err(|err| fail(&err))?;
        let detected = self.engine.detect_words(&input).map_err(|err| fail(&err))?;
        let lines = self.engine.find_text_lines(&input, &detected);
        let texts = self
            .engine
            .recognize_text(&input, &lines)
            .map_err(|err| fail(&err))?;
        let mut words = Vec::new();
        for (line, text) in texts.iter().enumerate() {
            let Some(text) = text else { continue };
            for word in text.words() {
                let rect = word.bounding_rect();
                words.push(RecognizedWord {
                    text: word.to_string(),
                    quad: Quad::from_rect(
                        rect.left() as f32,
                        rect.top() as f32,
                        rect.right() as f32,
                        rect.bottom() as f32,
                    ),
                    // ocrs ne donne pas de confiance.
                    confidence: 1.0,
                    line: line as u32,
                });
            }
        }
        Ok(words)
    }
}

// MARK: - Vision

/// Vision d'Apple, par le programme `vision_ocr`.
pub struct Vision {
    program: PathBuf,
}

impl Vision {
    /// Moteur lancé par `program`.
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    /// Version de macOS, qui fixe celle de Vision.
    pub fn version(&self) -> String {
        run(Command::new(&self.program).arg("--version"))
            .map(|out| out.trim().to_owned())
            .unwrap_or_else(|_| "absent".into())
    }
}

impl OcrEngine for Vision {
    fn name(&self) -> String {
        "Vision".into()
    }

    fn recognize(
        &self,
        image: &GrayImage,
        languages: &[String],
    ) -> Result<Vec<RecognizedWord>, OcrError> {
        let png = TempPng::write(image)?;
        let output = run(Command::new(&self.program)
            .args(["--languages", &languages.join(",")])
            .arg(&png.0))?;
        parse_vision(&output)
    }
}

/// Réponse de `vision_ocr` : tableau JSON de mots (`text`, `confidence`,
/// `line`, `quad` : huit nombres, coins supérieur gauche, supérieur droit,
/// inférieur droit, inférieur gauche, en pixels depuis le haut à gauche).
fn parse_vision(json: &str) -> Result<Vec<RecognizedWord>, OcrError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|err| OcrError::Failed(err.to_string()))?;
    let items = value
        .as_array()
        .ok_or_else(|| OcrError::Failed("réponse inattendue".into()))?;
    let mut words = Vec::with_capacity(items.len());
    for item in items {
        let quad: Vec<f32> = item["quad"]
            .as_array()
            .map(|q| {
                q.iter()
                    .filter_map(|v| v.as_f64())
                    .map(|v| v as f32)
                    .collect()
            })
            .unwrap_or_default();
        if quad.len() != 8 {
            continue;
        }
        let p = |i: usize| Point {
            x: quad[i],
            y: quad[i + 1],
        };
        words.push(RecognizedWord {
            text: item["text"].as_str().unwrap_or_default().to_owned(),
            quad: Quad {
                top_left: p(0),
                top_right: p(2),
                bottom_right: p(4),
                bottom_left: p(6),
            },
            confidence: item["confidence"].as_f64().unwrap_or(0.0) as f32,
            line: item["line"].as_u64().unwrap_or(0) as u32,
        });
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_la_sortie_de_tesseract() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   4\t1\t1\t1\t1\t0\t10\t10\t200\t30\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t10\t10\t80\t30\t96.5\tBonjour\n\
                   5\t1\t1\t1\t1\t2\t100\t12\t60\t28\t40\tà\n\
                   5\t1\t1\t1\t2\t1\t10\t60\t50\t30\t90\ttous\n";
        let words = parse_tsv(tsv);
        assert_eq!(words.len(), 3);
        assert_eq!(words[1].text, "à");
        assert!((words[1].confidence - 0.4).abs() < 1e-6);
        assert_eq!(words[0].line, words[1].line);
        assert_ne!(words[0].line, words[2].line);
        assert_eq!(words[2].quad.bounds(), (10.0, 60.0, 60.0, 90.0));
    }

    #[test]
    fn lit_la_sortie_de_vision() {
        let json = r#"[{"text":"Été","confidence":0.5,"line":3,"quad":[1,2,11,2,11,8,1,8]}]"#;
        let words = parse_vision(json).unwrap();
        assert_eq!(words[0].text, "Été");
        assert_eq!(words[0].line, 3);
        assert_eq!(words[0].quad.bounds(), (1.0, 2.0, 11.0, 8.0));
    }
}
