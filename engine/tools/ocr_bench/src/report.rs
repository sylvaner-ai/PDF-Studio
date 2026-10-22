//! Rapport du banc d'essai, en Markdown, et détail des lectures en CSV.

use std::collections::BTreeMap;
use std::fmt::Write;

use ocr::metrics::Score;

use crate::degrade::Profile;
use crate::{ScanRun, SimulatedRun, Variant};

/// Seuils de confiance examinés.
const THRESHOLDS: [f32; 5] = [0.3, 0.5, 0.6, 0.7, 0.9];

fn percent(value: f64) -> String {
    format!("{:.2} %", value * 100.0)
}

/// Erreurs cumulées des lectures retenues par `keep`.
fn total<'a>(runs: impl Iterator<Item = &'a SimulatedRun>) -> (Score, f64, usize) {
    let mut score = Score::default();
    let mut millis = 0.0;
    let mut count = 0;
    for run in runs {
        score.add(&run.score);
        millis += run.millis;
        count += 1;
    }
    (score, millis, count)
}

/// Moteurs et variantes présents, dans l'ordre d'apparition.
fn combinations(runs: &[SimulatedRun]) -> Vec<(String, Variant)> {
    let mut seen: Vec<(String, Variant)> = Vec::new();
    for run in runs {
        let key = (run.engine.clone(), run.variant);
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    seen
}

/// Pour chaque moteur, la variante qui lit le mieux l'ensemble des pages.
fn best_variants(runs: &[SimulatedRun]) -> Vec<(String, Variant)> {
    let mut best: BTreeMap<String, (Variant, f64)> = BTreeMap::new();
    for (engine, variant) in combinations(runs) {
        let (score, _, _) = total(
            runs.iter()
                .filter(|r| r.engine == engine && r.variant == variant),
        );
        let accuracy = score.char_accuracy();
        if best.get(&engine).is_none_or(|(_, top)| accuracy > *top) {
            best.insert(engine, (variant, accuracy));
        }
    }
    best.into_iter()
        .map(|(engine, (variant, _))| (engine, variant))
        .collect()
}

fn is_english(document: &str) -> bool {
    document.contains("anglais")
}

/// Rapport complet.
pub fn markdown(
    versions: &[(String, String)],
    profiles: &[Profile],
    runs: &[SimulatedRun],
    scans: &[ScanRun],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Mesures du banc d'essai OCR\n");
    let _ = writeln!(out, "Moteurs :\n");
    for (engine, version) in versions {
        let _ = writeln!(out, "- {engine} : {version}");
    }
    let documents: Vec<&str> = {
        let mut names: Vec<&str> = runs.iter().map(|r| r.document.as_str()).collect();
        names.dedup();
        names
    };
    let _ = writeln!(
        out,
        "\n{} documents à texte de référence, {} lectures de scans simulés ; {} lectures de vrais scans.\n",
        documents.len(),
        runs.len(),
        scans.len()
    );

    // Synthèse.
    let _ = writeln!(
        out,
        "## Taux de caractères corrects, par profil de numérisation\n"
    );
    let mut header = String::from("| Moteur | Prétraitement |");
    let mut rule = String::from("|---|---|");
    for profile in profiles {
        let _ = write!(header, " {} |", profile.name());
        rule.push_str("---:|");
    }
    header.push_str(" Mots corrects | Temps moyen par page |");
    rule.push_str("---:|---:|");
    let _ = writeln!(out, "{header}\n{rule}");
    for (engine, variant) in combinations(runs) {
        let mine = |r: &&SimulatedRun| r.engine == engine && r.variant == variant;
        let mut line = format!("| {engine} | {} |", variant.name());
        for &profile in profiles {
            let (score, _, count) =
                total(runs.iter().filter(mine).filter(|r| r.profile == profile));
            if count == 0 {
                line.push_str(" — |");
            } else {
                let _ = write!(line, " {} |", percent(score.char_accuracy()));
            }
        }
        let (score, millis, count) = total(runs.iter().filter(mine));
        let (_, millis_300, count_300) = total(
            runs.iter()
                .filter(mine)
                .filter(|r| r.profile != Profile::LowResolution),
        );
        let mean = if count_300 > 0 {
            millis_300 / count_300 as f64
        } else {
            millis / count.max(1) as f64
        };
        let _ = writeln!(
            line,
            " {} | {:.0} ms |",
            percent(score.word_accuracy()),
            mean
        );
        out.push_str(&line);
    }
    let _ = writeln!(
        out,
        "\nTemps moyen : pages à 300 dpi, prétraitement compris ; pour Vision et \
         Tesseract, il comprend aussi le lancement du programme et le chargement \
         de ses modèles.\n"
    );

    // Critère d'acceptation.
    let _ = writeln!(
        out,
        "## Critère d'acceptation : scans propres en français (≥ 98 %)\n"
    );
    let _ = writeln!(
        out,
        "| Moteur | Prétraitement | Documents publics | Documents privés |\n|---|---|---:|---:|"
    );
    for (engine, variant) in combinations(runs) {
        let base = |private: bool| {
            total(runs.iter().filter(|r| {
                r.engine == engine
                    && r.variant == variant
                    && r.profile == Profile::Clean
                    && r.private == private
                    && !is_english(&r.document)
            }))
        };
        let cell = |(score, _, count): (Score, f64, usize)| {
            if count == 0 {
                "—".to_owned()
            } else {
                percent(score.char_accuracy())
            }
        };
        let _ = writeln!(
            out,
            "| {engine} | {} | {} | {} |",
            variant.name(),
            cell(base(false)),
            cell(base(true))
        );
    }

    // Détail par document.
    let best = best_variants(runs);
    let _ = writeln!(
        out,
        "\n## Détail par document (meilleur prétraitement de chaque moteur)\n"
    );
    let mut header = String::from("| Document |");
    let mut rule = String::from("|---|");
    for (engine, variant) in &best {
        for profile in profiles {
            let _ = write!(
                header,
                " {engine} ({}) {} |",
                variant.name(),
                profile.name()
            );
            rule.push_str("---:|");
        }
    }
    let _ = writeln!(out, "{header}\n{rule}");
    for document in &documents {
        let mut line = format!("| {document} |");
        for (engine, variant) in &best {
            for &profile in profiles {
                let (score, _, count) = total(runs.iter().filter(|r| {
                    r.document == *document
                        && &r.engine == engine
                        && r.variant == *variant
                        && r.profile == profile
                }));
                if count == 0 {
                    line.push_str(" — |");
                } else {
                    let _ = write!(line, " {} |", percent(score.char_accuracy()));
                }
            }
        }
        let _ = writeln!(out, "{line}");
    }

    // Confiance.
    let _ = writeln!(
        out,
        "\n## Confiance annoncée et mots faux (meilleur prétraitement)\n\n\
         Pour chaque seuil : part des mots signalés (confiance inférieure), part de \
         mots faux parmi eux, et part des mots faux que le seuil signale.\n"
    );
    let _ = writeln!(
        out,
        "| Moteur | Mots faux | Seuil | Mots signalés | Faux parmi les signalés | Faux signalés |\n|---|---:|---:|---:|---:|---:|"
    );
    for (engine, variant) in &best {
        let outcomes: Vec<_> = runs
            .iter()
            .filter(|r| &r.engine == engine && r.variant == *variant)
            .flat_map(|r| r.outcomes.iter())
            .collect();
        if outcomes.is_empty() {
            continue;
        }
        let wrong = outcomes.iter().filter(|o| !o.correct).count();
        let n = outcomes.len() as f64;
        for threshold in THRESHOLDS {
            let flagged: Vec<_> = outcomes
                .iter()
                .filter(|o| o.confidence < threshold)
                .collect();
            let flagged_wrong = flagged.iter().filter(|o| !o.correct).count();
            let _ = writeln!(
                out,
                "| {engine} | {} | {threshold} | {} | {} | {} |",
                percent(wrong as f64 / n),
                percent(flagged.len() as f64 / n),
                if flagged.is_empty() {
                    "—".to_owned()
                } else {
                    percent(flagged_wrong as f64 / flagged.len() as f64)
                },
                if wrong == 0 {
                    "—".to_owned()
                } else {
                    percent(flagged_wrong as f64 / wrong as f64)
                },
            );
        }
    }

    // Orientation et inclinaison.
    let _ = writeln!(out, "\n## Orientation et inclinaison\n");
    let _ = writeln!(
        out,
        "| Moteur | Prétraitement | Pages retournées remises à l'endroit | Pages couchées remises à l'endroit | Écart moyen d'inclinaison (page à 4°) |\n|---|---|---:|---:|---:|"
    );
    for (engine, variant) in combinations(runs) {
        if variant == Variant::Raw {
            continue;
        }
        let upside: Vec<_> = runs
            .iter()
            .filter(|r| {
                r.engine == engine && r.variant == variant && r.profile == Profile::UpsideDown
            })
            .collect();
        let tilted: Vec<_> = runs
            .iter()
            .filter(|r| r.engine == engine && r.variant == variant && r.profile == Profile::Tilted)
            .collect();
        let sideways: Vec<_> = runs
            .iter()
            .filter(|r| {
                r.engine == engine && r.variant == variant && r.profile == Profile::Sideways
            })
            .collect();
        let righted = |runs: &[&SimulatedRun]| {
            if runs.is_empty() {
                "—".to_owned()
            } else {
                let ok = runs.iter().filter(|r| r.orientation_ok).count();
                format!("{ok} sur {}", runs.len())
            }
        };
        let skew_error = if tilted.is_empty() {
            "—".to_owned()
        } else {
            format!(
                "{:.2}°",
                tilted.iter().map(|r| (r.skew - 4.0).abs()).sum::<f64>() / tilted.len() as f64
            )
        };
        let _ = writeln!(
            out,
            "| {engine} | {} | {} | {} | {skew_error} |",
            variant.name(),
            righted(&upside),
            righted(&sideways),
        );
    }

    // Vrais scans.
    if !scans.is_empty() {
        let _ = writeln!(
            out,
            "\n## Vrais scans (sans texte de référence)\n\n\
             | Document | Page | Moteur | Prétraitement | Mots | Confiance moyenne | Quarts de tour | Temps |\n\
             |---|---:|---|---|---:|---:|---:|---:|"
        );
        for run in scans {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {:.2} | {} | {:.0} ms |",
                run.document,
                run.page,
                run.engine,
                run.variant.name(),
                run.words,
                run.confidence,
                run.quarter_turns,
                run.millis
            );
        }
    }
    out
}

/// Détail des lectures des scans simulés, une ligne par lecture.
pub fn csv(runs: &[SimulatedRun]) -> String {
    let mut out = String::from(
        "document;page;profil;moteur;pretraitement;caracteres;erreurs;mots;erreurs_mots;ms;orientation_ok;inclinaison\n",
    );
    for run in runs {
        let _ = writeln!(
            out,
            "{};{};{};{};{};{};{};{};{};{:.0};{};{:.2}",
            run.document,
            run.page,
            run.profile.name(),
            run.engine,
            run.variant.name(),
            run.score.chars,
            run.score.char_errors,
            run.score.words,
            run.score.word_errors,
            run.millis,
            run.orientation_ok,
            run.skew
        );
    }
    out
}
