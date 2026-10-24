//! Encodage de l'image améliorée.
//!
//! Le scan d'origine est presque toujours un JPEG : l'image améliorée l'est
//! aussi, de qualité 90 par défaut (cf. D63), en niveaux de gris si tous ses
//! pixels sont gris. Le fichier garde ainsi une taille voisine de celle du
//! scan ; une image sans perte serait dix fois plus lourde.

use jpeg_encoder::{ColorType, Encoder};
use scan::RgbImage;

/// Image encodée, prête à être posée dans une page PDF (`/DCTDecode`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    /// Octets du fichier JPEG.
    pub data: Vec<u8>,
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Un seul canal (niveaux de gris).
    pub gray: bool,
}

/// L'image n'a pas pu être encodée.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// Plus de 65 535 pixels de côté : hors des limites du format.
    #[error("image trop grande pour le format JPEG ({0} × {1} pixels)")]
    TooLarge(usize, usize),
    /// Erreur de l'encodeur.
    #[error("encodage JPEG impossible : {0}")]
    Encoder(String),
}

/// Encode `image` en JPEG de qualité `quality` (1 à 100).
pub fn encode(image: &RgbImage, quality: u8) -> Result<Encoded, EncodeError> {
    let (w, h) = (image.width, image.height);
    let (Ok(w16), Ok(h16)) = (u16::try_from(w), u16::try_from(h)) else {
        return Err(EncodeError::TooLarge(w, h));
    };
    let gray = image
        .data
        .chunks_exact(3)
        .all(|p| p[0] == p[1] && p[1] == p[2]);
    let mut data = Vec::new();
    let encoder = Encoder::new(&mut data, quality.clamp(1, 100));
    let result = if gray {
        let luma: Vec<u8> = image.data.chunks_exact(3).map(|p| p[0]).collect();
        encoder.encode(&luma, w16, h16, ColorType::Luma)
    } else {
        encoder.encode(&image.data, w16, h16, ColorType::Rgb)
    };
    result.map_err(|err| EncodeError::Encoder(err.to_string()))?;
    Ok(Encoded {
        data,
        width: w as u32,
        height: h as u32,
        gray,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn couleur_ou_gris() {
        let mut image = RgbImage::new(64, 48, [240, 230, 200]);
        image.set(3, 3, [10, 20, 30]);
        let color = encode(&image, 90).unwrap();
        assert!(!color.gray);
        assert_eq!((color.width, color.height), (64, 48));
        assert_eq!(&color.data[..2], &[0xFF, 0xD8]);
        let gray = encode(&RgbImage::new(64, 48, [128; 3]), 90).unwrap();
        assert!(gray.gray);
        assert!(gray.data.len() < color.data.len());
    }
}
