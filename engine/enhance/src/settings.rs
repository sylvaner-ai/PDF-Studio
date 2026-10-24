//! Réglages du mode Amélioration, sérialisables en JSON : l'application les
//! retient d'une session à l'autre, le banc d'essai les consigne avec ses
//! mesures. Une clé absente prend sa valeur par défaut, ce qui laisse lire
//! des réglages écrits avant l'ajout d'une étape.

use serde::{Deserialize, Serialize};

/// Réglages de toutes les étapes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Étape 1 : redressement.
    pub straighten: StraightenSettings,
    /// Qualité de l'image améliorée enregistrée (JPEG, de 1 à 100).
    pub jpeg_quality: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            straighten: StraightenSettings::default(),
            jpeg_quality: 90,
        }
    }
}

/// Réglages du redressement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StraightenSettings {
    /// Corriger l'inclinaison.
    pub deskew: bool,
    /// Remettre à l'endroit une page couchée ou à l'envers.
    pub orientation: bool,
    /// Inclinaison sous laquelle la page est laissée telle quelle, en degrés.
    pub min_degrees: f64,
    /// Plafond de la correction, en degrés : au-delà, la page n'est pas
    /// redressée et elle est signalée « à vérifier ».
    pub max_degrees: f64,
}

impl Default for StraightenSettings {
    fn default() -> Self {
        Self {
            deskew: true,
            orientation: true,
            min_degrees: 0.1,
            max_degrees: 15.0,
        }
    }
}

/// Réglages illisibles.
#[derive(Debug, thiserror::Error)]
#[error("réglages illisibles : {0}")]
pub struct SettingsError(String);

impl Settings {
    /// Réglages au format JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Réglages lus au format JSON ; les valeurs hors limites sont ramenées
    /// dans leurs bornes.
    pub fn from_json(text: &str) -> Result<Self, SettingsError> {
        let settings: Settings =
            serde_json::from_str(text).map_err(|err| SettingsError(err.to_string()))?;
        Ok(settings.bounded())
    }

    /// Les mêmes réglages, ramenés dans leurs bornes : seuil de 0 à 5°,
    /// plafond de ce seuil à 45°, qualité de 50 à 100.
    pub fn bounded(mut self) -> Self {
        let s = &mut self.straighten;
        if !s.min_degrees.is_finite() {
            s.min_degrees = StraightenSettings::default().min_degrees;
        }
        if !s.max_degrees.is_finite() {
            s.max_degrees = StraightenSettings::default().max_degrees;
        }
        s.min_degrees = s.min_degrees.clamp(0.0, 5.0);
        s.max_degrees = s.max_degrees.clamp(s.min_degrees, 45.0);
        self.jpeg_quality = self.jpeg_quality.clamp(50, 100);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_en_json() {
        let mut settings = Settings::default();
        settings.straighten.orientation = false;
        settings.straighten.max_degrees = 10.0;
        let text = settings.to_json();
        assert_eq!(Settings::from_json(&text).unwrap(), settings);
    }

    #[test]
    fn cles_absentes_et_valeurs_hors_limites() {
        let settings = Settings::from_json(r#"{"straighten":{"max_degrees":90}}"#).unwrap();
        assert_eq!(settings.straighten.max_degrees, 45.0);
        assert!(settings.straighten.deskew);
        assert_eq!(settings.jpeg_quality, 90);
        assert!(Settings::from_json("pas du JSON").is_err());
    }
}
