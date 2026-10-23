//! Texte retouché d'une page scannée : nouveau texte d'une ligne de l'image
//! et réglages qui le font ressembler au scan.
//!
//! Le texte est dessiné dans les pixels du scan (pas en texte PDF) : la
//! police, le corps et l'interlettrage retrouvés sur la ligne, plus la
//! graisse, le flou, le grain et la couleur de l'encre. Les réglages
//! mesurés sont gardés à côté des réglages actuels, pour y revenir.

/// Réglages du rendu d'un texte dans un scan. Les longueurs sont en points :
/// elles ne dépendent pas de la résolution de l'image.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanStyle {
    /// Nom PostScript de la police.
    pub font: String,
    /// Famille de la police.
    pub family: String,
    /// Corps (taille du cadratin), en points.
    pub size: f32,
    /// Interlettrage, en points.
    pub spacing: f32,
    /// Graisse ajoutée, de -1 (traits amincis) à 1 (épaissis).
    pub weight: f32,
    /// Flou (écart type), en points.
    pub blur: f32,
    /// Grain de l'encre (écart type, sur 255).
    pub noise: f32,
    /// Couleur de l'encre (rouge, vert, bleu, de 0 à 1).
    pub color: [f32; 3],
    /// Crénage de la police appliqué.
    pub kerning: bool,
}

/// Police candidate retrouvée sur la ligne, avec ses réglages.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanCandidate {
    /// Nom PostScript.
    pub font: String,
    /// Famille.
    pub family: String,
    /// Similarité du rendu avec le scan, de 0 à 1.
    pub score: f32,
    /// Corps, en points.
    pub size: f32,
    /// Interlettrage, en points.
    pub spacing: f32,
    /// Graisse ajoutée.
    pub weight: f32,
    /// Flou, en points.
    pub blur: f32,
    /// Crénage appliqué.
    pub kerning: bool,
}

impl ScanCandidate {
    /// Réglages de cette police, avec la couleur et le grain de `base`.
    pub fn style(&self, base: &ScanStyle) -> ScanStyle {
        ScanStyle {
            font: self.font.clone(),
            family: self.family.clone(),
            size: self.size,
            spacing: self.spacing,
            weight: self.weight,
            blur: self.blur,
            kerning: self.kerning,
            ..base.clone()
        }
    }
}

/// Nouveau texte d'une ligne scannée.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanText {
    /// Texte de la ligne.
    pub text: String,
    /// Réglages actuels.
    pub style: ScanStyle,
    /// Réglages mesurés sur le scan (meilleure police).
    pub measured: ScanStyle,
    /// Polices retrouvées, de la plus proche à la moins proche.
    pub candidates: Vec<ScanCandidate>,
}

impl ScanText {
    /// Forme texte, d'une ligne par valeur, pour la copie de récupération.
    pub fn to_record(&self) -> String {
        let mut out = String::new();
        push(&mut out, &["text", &escape(&self.text)]);
        for (key, style) in [("style", &self.style), ("measured", &self.measured)] {
            let [r, g, b] = style.color;
            push(
                &mut out,
                &[
                    key,
                    &escape(&style.font),
                    &escape(&style.family),
                    &style.size.to_string(),
                    &style.spacing.to_string(),
                    &style.weight.to_string(),
                    &style.blur.to_string(),
                    &style.noise.to_string(),
                    &r.to_string(),
                    &g.to_string(),
                    &b.to_string(),
                    if style.kerning { "1" } else { "0" },
                ],
            );
        }
        for c in &self.candidates {
            push(
                &mut out,
                &[
                    "candidate",
                    &escape(&c.font),
                    &escape(&c.family),
                    &c.score.to_string(),
                    &c.size.to_string(),
                    &c.spacing.to_string(),
                    &c.weight.to_string(),
                    &c.blur.to_string(),
                    if c.kerning { "1" } else { "0" },
                ],
            );
        }
        out
    }

    /// Relit la forme texte de [`ScanText::to_record`] ; `None` si elle est
    /// illisible.
    pub fn from_record(record: &str) -> Option<ScanText> {
        let (mut text, mut style, mut measured) = (None, None, None);
        let mut candidates = Vec::new();
        for line in record.lines() {
            let fields: Vec<String> = line.split('\t').map(unescape).collect();
            let number = |i: usize| fields.get(i)?.parse::<f32>().ok();
            match fields.first().map(String::as_str) {
                Some("text") => text = Some(fields.get(1)?.clone()),
                Some(key @ ("style" | "measured")) => {
                    let parsed = ScanStyle {
                        font: fields.get(1)?.clone(),
                        family: fields.get(2)?.clone(),
                        size: number(3)?,
                        spacing: number(4)?,
                        weight: number(5)?,
                        blur: number(6)?,
                        noise: number(7)?,
                        color: [number(8)?, number(9)?, number(10)?],
                        kerning: fields.get(11)? == "1",
                    };
                    if key == "style" {
                        style = Some(parsed);
                    } else {
                        measured = Some(parsed);
                    }
                }
                Some("candidate") => candidates.push(ScanCandidate {
                    font: fields.get(1)?.clone(),
                    family: fields.get(2)?.clone(),
                    score: number(3)?,
                    size: number(4)?,
                    spacing: number(5)?,
                    weight: number(6)?,
                    blur: number(7)?,
                    kerning: fields.get(8)? == "1",
                }),
                _ => return None,
            }
        }
        let style = style?;
        Some(ScanText {
            text: text?,
            measured: measured.unwrap_or_else(|| style.clone()),
            style,
            candidates,
        })
    }
}

fn push(out: &mut String, fields: &[&str]) {
    out.push_str(&fields.join("\t"));
    out.push('\n');
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forme_de_recuperation() {
        let style = ScanStyle {
            font: "Caladea-Regular".into(),
            family: "Caladea".into(),
            size: 11.5,
            spacing: -0.1,
            weight: 0.25,
            blur: 0.3,
            noise: 6.0,
            color: [0.1, 0.12, 0.2],
            kerning: true,
        };
        let text = ScanText {
            text: "Total\tTTC\nligne \\ 2".into(),
            measured: style.clone(),
            style: ScanStyle {
                size: 12.0,
                ..style.clone()
            },
            candidates: vec![ScanCandidate {
                font: "Carlito-Regular".into(),
                family: "Carlito".into(),
                score: 0.81,
                size: 11.0,
                spacing: 0.0,
                weight: 0.0,
                blur: 0.4,
                kerning: false,
            }],
        };
        let record = text.to_record();
        assert_eq!(ScanText::from_record(&record), Some(text));
        assert_eq!(ScanText::from_record("inconnu\t1"), None);
    }
}
