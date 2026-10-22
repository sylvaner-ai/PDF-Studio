//! Couche de texte d'une page scannée : les mots lus par la reconnaissance de
//! texte, avec leur emprise et leur confiance.
//!
//! La couche est invisible : elle rend la page cherchable et permet d'en
//! sélectionner le texte, sans changer son aspect. Comme les autres
//! modifications, elle n'est écrite dans le fichier qu'à l'enregistrement.

/// Un mot de la couche de texte.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerWord {
    /// Texte du mot, sans espace.
    pub text: String,
    /// Emprise du mot dans l'espace de la page PDF (ordonnées vers le haut,
    /// sans rotation) : coins inférieur gauche, inférieur droit, supérieur
    /// droit et supérieur gauche, dans le sens de lecture du mot (`x`, `y`
    /// pour chacun).
    pub quad: [f32; 8],
    /// Confiance de la reconnaissance, de 0 à 1.
    pub confidence: f32,
    /// Numéro de ligne : les mots d'une ligne le partagent et se suivent.
    pub line: u32,
}

impl LayerWord {
    /// Coin inférieur gauche (début de la ligne de pied).
    pub fn bottom_left(&self) -> (f32, f32) {
        (self.quad[0], self.quad[1])
    }

    /// Coin inférieur droit.
    pub fn bottom_right(&self) -> (f32, f32) {
        (self.quad[2], self.quad[3])
    }

    /// Coin supérieur droit.
    pub fn top_right(&self) -> (f32, f32) {
        (self.quad[4], self.quad[5])
    }

    /// Coin supérieur gauche.
    pub fn top_left(&self) -> (f32, f32) {
        (self.quad[6], self.quad[7])
    }
}

/// Mots reconnus sur une page, dans l'ordre de lecture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayer {
    /// Mots, ligne après ligne.
    pub words: Vec<LayerWord>,
    /// Nom du moteur de reconnaissance (« Vision »).
    pub engine: String,
}

impl TextLayer {
    /// Vrai si aucun mot n'a été reconnu.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}
