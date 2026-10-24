//! Rotation d'une image par interpolation bicubique.
//!
//! L'image tourne autour de son centre et garde sa taille : la page du scan
//! garde son format et sa place. Les pixels qui viennent d'en dehors de
//! l'image (coins découverts) prennent la couleur du papier au bord le plus
//! proche ([`Paper`]), qui entre aussi dans l'interpolation au bord : le
//! passage est sans liseré. Les rangées sont calculées en parallèle, sur
//! tous les cœurs.

use scan::RgbImage;

use crate::paper::Paper;

/// Image `image` tournée de `degrees` degrés dans le sens horaire (à
/// l'écran, ordonnées vers le bas) autour de son centre, de même taille, les
/// pixels venus d'en dehors remplis de la couleur de `paper`. Même
/// convention que `ocr::GrayImage::rotate` : redresser des lignes inclinées
/// de `θ` demande une rotation de `−θ`.
pub fn rotate(image: &RgbImage, degrees: f64, paper: &Paper) -> RgbImage {
    let (w, h) = (image.width, image.height);
    if degrees.abs() < 1e-6 || w == 0 || h == 0 {
        return image.clone();
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let mut data = vec![0u8; w * h * 3];
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, 16);
    let rows_per_chunk = h.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (chunk, rows) in data.chunks_mut(rows_per_chunk * w * 3).enumerate() {
            scope.spawn(move || {
                let first = chunk * rows_per_chunk;
                for (dy, row) in rows.chunks_mut(w * 3).enumerate() {
                    let y = (first + dy) as f64 + 0.5 - cy;
                    for x in 0..w {
                        // Point de l'image d'origine (rotation inverse autour
                        // du centre), puis repère des centres des pixels.
                        let xr = x as f64 + 0.5 - cx;
                        let sx = cos * xr + sin * y + cx - 0.5;
                        let sy = -sin * xr + cos * y + cy - 0.5;
                        let color = sample(image, sx, sy, paper);
                        row[x * 3..x * 3 + 3]
                            .copy_from_slice(&color.map(|c| c.round().clamp(0.0, 255.0) as u8));
                    }
                }
            });
        }
    });
    RgbImage {
        width: w,
        height: h,
        data,
    }
}

/// Noyau cubique de Keys (a = −0,5) : interpolation sans flou excessif ni
/// rebond marqué.
fn keys(t: f64) -> f64 {
    const A: f64 = -0.5;
    let t = t.abs();
    if t <= 1.0 {
        ((A + 2.0) * t - (A + 3.0)) * t * t + 1.0
    } else if t < 2.0 {
        ((A * t - 5.0 * A) * t + 8.0 * A) * t - 4.0 * A
    } else {
        0.0
    }
}

/// Couleur interpolée au point `(x, y)`, en coordonnées des centres des
/// pixels ; celle du papier hors de l'image.
fn sample(image: &RgbImage, x: f64, y: f64, paper: &Paper) -> [f32; 3] {
    let (w, h) = (image.width as i64, image.height as i64);
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    if x0 < -2 || y0 < -2 || x0 > w + 1 || y0 > h + 1 {
        return paper.at(x, y);
    }
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let wx = [keys(fx + 1.0), keys(fx), keys(1.0 - fx), keys(2.0 - fx)];
    let wy = [keys(fy + 1.0), keys(fy), keys(1.0 - fy), keys(2.0 - fy)];
    let inside = x0 >= 1 && y0 >= 1 && x0 + 2 < w && y0 + 2 < h;
    let fill = if inside { [0.0; 3] } else { paper.at(x, y) };
    let mut out = [0f64; 3];
    for (j, &wyj) in wy.iter().enumerate() {
        let py = y0 - 1 + j as i64;
        let mut line = [0f64; 3];
        for (i, &wxi) in wx.iter().enumerate() {
            let px = x0 - 1 + i as i64;
            let c = if inside || (px >= 0 && py >= 0 && px < w && py < h) {
                let k = (py as usize * image.width + px as usize) * 3;
                [
                    f32::from(image.data[k]),
                    f32::from(image.data[k + 1]),
                    f32::from(image.data[k + 2]),
                ]
            } else {
                fill
            };
            for channel in 0..3 {
                line[channel] += wxi * f64::from(c[channel]);
            }
        }
        for channel in 0..3 {
            out[channel] += wyj * line[channel];
        }
    }
    out.map(|v| v as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Damier de 8 pixels sur un fond gris.
    fn checker(w: usize, h: usize) -> RgbImage {
        let mut image = RgbImage::new(w, h, [128, 128, 128]);
        for y in 0..h {
            for x in 0..w {
                if (x / 8 + y / 8) % 2 == 0 {
                    image.set(x as i32, y as i32, [250, 40, 10]);
                }
            }
        }
        image
    }

    #[test]
    fn sans_angle_l_image_est_intacte() {
        let image = checker(40, 30);
        assert_eq!(rotate(&image, 0.0, &Paper::uniform([0; 3])), image);
    }

    #[test]
    fn aller_et_retour() {
        let image = checker(120, 90);
        let gray = Paper::uniform([128; 3]);
        let back = rotate(&rotate(&image, 3.0, &gray), -3.0, &gray);
        // Loin des bords, l'image revient à peu près à l'identique.
        let mut error = 0f64;
        let mut count = 0;
        for y in 20..70 {
            for x in 20..100 {
                let (a, b) = (image.get(x, y), back.get(x, y));
                for c in 0..3 {
                    error += (f64::from(a[c]) - f64::from(b[c])).abs();
                    count += 1;
                }
            }
        }
        assert!(
            error / f64::from(count) < 12.0,
            "{}",
            error / f64::from(count)
        );
    }

    #[test]
    fn sens_horaire_et_coins_remplis() {
        // Un trait horizontal au centre, tourné de 10° dans le sens
        // horaire : sa partie droite descend (ordonnées vers le bas).
        let mut image = RgbImage::new(200, 100, [255; 3]);
        for x in 40..160 {
            for y in 49..51 {
                image.set(x, y, [0; 3]);
            }
        }
        let rotated = rotate(&image, 10.0, &Paper::uniform([200, 0, 0]));
        let darkest = |x: i32| (0..100).min_by_key(|&y| rotated.get(x, y)[0]).unwrap();
        assert!(darkest(150) > darkest(50) + 10);
        // Les coins viennent d'en dehors de l'image.
        assert_eq!(rotated.get(0, 0), [200, 0, 0]);
        assert_eq!(rotated.get(199, 99), [200, 0, 0]);
    }
}
