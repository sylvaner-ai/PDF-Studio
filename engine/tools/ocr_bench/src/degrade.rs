//! Scans simulés : une page rastérisée est dégradée comme par un scanner
//! (niveaux, éclairage, flou, inclinaison, bruit, compression JPEG).
//! Toutes les étapes sont déterministes.

use ocr::{Affine, GrayImage};

/// Profil de numérisation simulée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Scanner de bureau, 300 dpi, document propre.
    Clean,
    /// Photocopie fatiguée : encre pâle, fond gris, éclairage inégal, flou,
    /// bruit, forte compression, légère inclinaison.
    Degraded,
    /// Page posée de travers (4°).
    Tilted,
    /// Page numérisée à l'envers.
    UpsideDown,
    /// Page à l'italienne numérisée couchée (un quart de tour, sens
    /// antihoraire).
    Sideways,
    /// Basse résolution (150 dpi), comme une télécopie ou un scan rapide.
    LowResolution,
}

impl Profile {
    /// Tous les profils, dans l'ordre du rapport.
    pub const ALL: [Profile; 6] = [
        Profile::Clean,
        Profile::Degraded,
        Profile::Tilted,
        Profile::UpsideDown,
        Profile::Sideways,
        Profile::LowResolution,
    ];

    /// Nom court, pour la ligne de commande et le rapport.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Clean => "propre",
            Profile::Degraded => "dégradé",
            Profile::Tilted => "incliné",
            Profile::UpsideDown => "retourné",
            Profile::Sideways => "couché",
            Profile::LowResolution => "150 dpi",
        }
    }

    /// Profil désigné par son nom.
    pub fn parse(name: &str) -> Option<Profile> {
        Profile::ALL
            .into_iter()
            .find(|p| p.name() == name || (name == "degrade" && *p == Profile::Degraded))
            .or(match name {
                "incline" => Some(Profile::Tilted),
                "retourne" => Some(Profile::UpsideDown),
                "couche" => Some(Profile::Sideways),
                "150" => Some(Profile::LowResolution),
                _ => None,
            })
    }

    /// Quarts de tour (sens horaire) qui remettent la page simulée à l'endroit.
    pub fn expected_turns(self) -> u8 {
        (4 - self.spec().turns) % 4
    }

    /// Résolution de rastérisation.
    pub fn dpi(self) -> f32 {
        match self {
            Profile::LowResolution => 150.0,
            _ => 300.0,
        }
    }

    fn spec(self) -> Spec {
        let clean = Spec {
            ink: 20.0,
            paper: 245.0,
            gradient: 0.0,
            blur: 0.5,
            skew: 0.0,
            turns: 0,
            noise: 3.0,
            jpeg: 92,
        };
        match self {
            Profile::Clean => clean,
            Profile::Degraded => Spec {
                ink: 75.0,
                paper: 215.0,
                gradient: 0.22,
                blur: 1.1,
                skew: 1.2,
                noise: 12.0,
                jpeg: 45,
                ..clean
            },
            Profile::Tilted => Spec {
                skew: 4.0,
                blur: 0.6,
                noise: 4.0,
                jpeg: 85,
                ..clean
            },
            Profile::UpsideDown => Spec { turns: 2, ..clean },
            Profile::Sideways => Spec { turns: 3, ..clean },
            Profile::LowResolution => Spec {
                blur: 0.4,
                noise: 5.0,
                jpeg: 75,
                ..clean
            },
        }
    }
}

/// Réglages d'un profil.
struct Spec {
    /// Niveau de l'encre noire.
    ink: f32,
    /// Niveau du papier blanc.
    paper: f32,
    /// Assombrissement du bord droit (éclairage inégal), de 0 à 1.
    gradient: f32,
    /// Écart type du flou gaussien, en pixels.
    blur: f32,
    /// Inclinaison, en degrés (sens horaire).
    skew: f64,
    /// Quarts de tour (sens horaire).
    turns: u8,
    /// Écart type du bruit gaussien, en niveaux de gris.
    noise: f32,
    /// Qualité de la compression JPEG.
    jpeg: u8,
}

/// Générateur pseudo-aléatoire xorshift64*, déterministe.
pub struct Rng(u64);

impl Rng {
    /// Générateur initialisé par `seed`.
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn uniform(&mut self) -> f32 {
        ((self.next() >> 40) as f32 + 0.5) / (1u64 << 24) as f32
    }

    /// Tirage gaussien centré réduit (Box-Muller).
    fn gaussian(&mut self) -> f32 {
        let (u, v) = (self.uniform(), self.uniform());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

/// Applique `profile` à la page `page` ; renvoie l'image dégradée et la
/// transformation qui ramène un de ses points dans la page d'origine.
pub fn degrade(page: &GrayImage, profile: Profile, seed: u64) -> (GrayImage, Affine) {
    let spec = profile.spec();
    let mut rng = Rng::new(seed);
    let mut image = page.clone();
    let width = image.width as f32;
    for (index, value) in image.pixels.iter_mut().enumerate() {
        let x = (index % image.width as usize) as f32;
        let level = spec.ink + (spec.paper - spec.ink) * f32::from(*value) / 255.0;
        let light = 1.0 - spec.gradient * x / width;
        *value = (level * light).round().clamp(0.0, 255.0) as u8;
    }
    image = gaussian_blur(&image, spec.blur);

    let (turned, back_turn) = image.rotate_quarters(spec.turns);
    let (tilted, back_tilt) = turned.rotate(spec.skew, spec.paper as u8);
    image = tilted;
    let to_upright = back_tilt.then(&back_turn);

    for value in image.pixels.iter_mut() {
        let noisy = f32::from(*value) + rng.gaussian() * spec.noise;
        *value = noisy.round().clamp(0.0, 255.0) as u8;
    }
    (jpeg_round_trip(&image, spec.jpeg), to_upright)
}

/// Flou gaussien séparable.
pub fn gaussian_blur(image: &GrayImage, sigma: f32) -> GrayImage {
    if sigma <= 0.0 {
        return image.clone();
    }
    let radius = (sigma * 3.0).ceil() as i32;
    let kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|k| k / total).collect();
    let (w, h) = (image.width as i32, image.height as i32);
    let pass = |source: &[u8], horizontal: bool| -> Vec<u8> {
        let mut out = vec![0u8; source.len()];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let offset = k as i32 - radius;
                    let (sx, sy) = if horizontal {
                        ((x + offset).clamp(0, w - 1), y)
                    } else {
                        (x, (y + offset).clamp(0, h - 1))
                    };
                    sum += weight * f32::from(source[(sy * w + sx) as usize]);
                }
                out[(y * w + x) as usize] = sum.round().clamp(0.0, 255.0) as u8;
            }
        }
        out
    };
    let horizontal = pass(&image.pixels, true);
    GrayImage {
        width: image.width,
        height: image.height,
        pixels: pass(&horizontal, false),
    }
}

/// Compression puis décompression JPEG.
fn jpeg_round_trip(image: &GrayImage, quality: u8) -> GrayImage {
    let mut encoded = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, quality);
    let buffer = image::GrayImage::from_raw(image.width, image.height, image.pixels.clone())
        .expect("dimensions cohérentes");
    if buffer.write_with_encoder(encoder).is_err() {
        return image.clone();
    }
    match image::load_from_memory(&encoded) {
        Ok(decoded) => {
            let gray = decoded.into_luma8();
            GrayImage {
                width: gray.width(),
                height: gray.height(),
                pixels: gray.into_raw(),
            }
        }
        Err(_) => image.clone(),
    }
}
