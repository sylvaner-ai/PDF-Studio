//! Préparation de l'image d'une page avant sa lecture : fond égalisé et
//! contraste, bruit, binarisation adaptative, inclinaison et orientation.
//!
//! Chaque étape travaille sur une [`GrayImage`] et dépend de la résolution de
//! l'image (`dpi`), qui fixe la taille des caractères attendus.

use crate::image::GrayImage;

/// Égalise le fond (éclairage inégal, papier jauni) puis étire le contraste :
/// le papier devient blanc et l'encre la plus sombre noire.
///
/// Le fond de chaque carré d'un quart de pouce est le niveau que dépassent
/// ses 10 % de pixels les plus clairs (le bruit n'y compte pas, à la
/// différence du pixel le plus clair) ; un carré presque couvert d'encre
/// prend la médiane de ses voisins. Le fond est interpolé entre les carrés et
/// chaque pixel divisé par ce fond.
pub fn flatten_background(image: &GrayImage, dpi: f32) -> GrayImage {
    let block = ((dpi / 4.0).round() as u32).max(16);
    let (gw, gh) = (image.width.div_ceil(block), image.height.div_ceil(block));
    let mut histograms = vec![[0u32; 256]; (gw * gh) as usize];
    for y in 0..image.height {
        let row = &image.pixels[(y * image.width) as usize..][..image.width as usize];
        let gy = y / block;
        for (x, &value) in row.iter().enumerate() {
            histograms[(gy * gw + x as u32 / block) as usize][value as usize] += 1;
        }
    }
    let levels: Vec<u8> = histograms
        .iter()
        .map(|histogram| {
            let total: u32 = histogram.iter().sum();
            let mut brighter = 0;
            for level in (0..256).rev() {
                brighter += histogram[level];
                if brighter * 10 >= total {
                    return level as u8;
                }
            }
            0
        })
        .collect();
    let mut background = levels.clone();
    for gy in 0..gh {
        for gx in 0..gw {
            let mut around: Vec<u8> = Vec::with_capacity(9);
            for ny in gy.saturating_sub(1)..=(gy + 1).min(gh - 1) {
                for nx in gx.saturating_sub(1)..=(gx + 1).min(gw - 1) {
                    around.push(levels[(ny * gw + nx) as usize]);
                }
            }
            around.sort_unstable();
            let median = around[around.len() / 2];
            let own = levels[(gy * gw + gx) as usize];
            background[(gy * gw + gx) as usize] = own.max(median).max(1);
        }
    }
    let grid = Grid {
        values: background.iter().map(|&v| f32::from(v)).collect(),
        width: gw,
        height: gh,
        cell: block as f32,
    };
    let mut out = GrayImage::new(image.width, image.height, 255);
    for y in 0..image.height {
        for x in 0..image.width {
            let index = (y * image.width + x) as usize;
            let bg = grid.at(x, y);
            let value = f32::from(image.pixels[index]) * 255.0 / bg;
            out.pixels[index] = value.round().clamp(0.0, 255.0) as u8;
        }
    }
    stretch_contrast(&mut out);
    out
}

/// Étire les niveaux : l'encre la plus sombre (premier centile des pixels
/// sombres) devient noire, et le papier blanc. Le blanc est pris au quart le
/// plus sombre des pixels (une page est surtout du papier) : le grain du
/// papier et le bruit du scanner disparaissent, alors qu'ils occupent toute la
/// page et égarent les moteurs de lecture. Une page presque blanche est
/// laissée telle quelle.
fn stretch_contrast(image: &mut GrayImage) {
    let mut histogram = [0u64; 256];
    for &value in &image.pixels {
        histogram[value as usize] += 1;
    }
    let dark: u64 = histogram[..200].iter().sum();
    if dark < 100 {
        return;
    }
    let level_at = |count: u64, histogram: &[u64; 256]| {
        let mut seen = 0u64;
        for (value, &n) in histogram.iter().enumerate() {
            seen += n;
            if seen > count {
                return value;
            }
        }
        255
    };
    let low = level_at(dark / 100, &histogram);
    if low == 0 || low > 160 {
        return;
    }
    let total = image.pixels.len() as u64;
    let high = level_at(total / 4, &histogram).max(low + 80).min(255);
    let (low, high) = (low as f32, high as f32);
    for value in &mut image.pixels {
        let stretched = (f32::from(*value) - low) * 255.0 / (high - low);
        *value = stretched.round().clamp(0.0, 255.0) as u8;
    }
}

/// Filtre médian 3 × 3 : retire le bruit ponctuel sans flouter les contours.
pub fn median3(image: &GrayImage) -> GrayImage {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut out = image.clone();
    if w < 3 || h < 3 {
        return out;
    }
    let mut window = [0u8; 9];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut k = 0;
            for dy in 0..3 {
                let row = &image.pixels[(y + dy - 1) * w + x - 1..][..3];
                window[k..k + 3].copy_from_slice(row);
                k += 3;
            }
            window.sort_unstable();
            out.pixels[y * w + x] = window[4];
        }
    }
    out
}

/// Binarisation adaptative de Sauvola : le seuil de chaque pixel dépend de la
/// moyenne et de l'écart type de son voisinage (un sixième de pouce), ce qui
/// résiste aux fonds inégaux et aux encres pâles. Les taches isolées de
/// quelques pixels sont ensuite retirées.
pub fn binarize(image: &GrayImage, dpi: f32) -> GrayImage {
    const K: f32 = 0.25;
    let window = ((dpi / 6.0).round() as u32).max(12);
    let cell = (window / 2).max(4);
    let (gw, gh) = (image.width.div_ceil(cell), image.height.div_ceil(cell));
    let mut sums = vec![0f64; (gw * gh) as usize];
    let mut squares = vec![0f64; (gw * gh) as usize];
    let mut counts = vec![0u32; (gw * gh) as usize];
    for y in 0..image.height {
        let row = &image.pixels[(y * image.width) as usize..][..image.width as usize];
        let gy = y / cell;
        for (x, &value) in row.iter().enumerate() {
            let index = (gy * gw + x as u32 / cell) as usize;
            let v = f64::from(value);
            sums[index] += v;
            squares[index] += v * v;
            counts[index] += 1;
        }
    }
    let mut thresholds = vec![0f32; (gw * gh) as usize];
    for gy in 0..gh {
        for gx in 0..gw {
            let (mut sum, mut square, mut count) = (0f64, 0f64, 0u32);
            for ny in gy.saturating_sub(1)..=(gy + 1).min(gh - 1) {
                for nx in gx.saturating_sub(1)..=(gx + 1).min(gw - 1) {
                    let index = (ny * gw + nx) as usize;
                    sum += sums[index];
                    square += squares[index];
                    count += counts[index];
                }
            }
            let n = f64::from(count.max(1));
            let mean = sum / n;
            let deviation = (square / n - mean * mean).max(0.0).sqrt();
            thresholds[(gy * gw + gx) as usize] =
                (mean * (1.0 + f64::from(K) * (deviation / 128.0 - 1.0))) as f32;
        }
    }
    let grid = Grid {
        values: thresholds,
        width: gw,
        height: gh,
        cell: cell as f32,
    };
    let mut out = GrayImage::new(image.width, image.height, 255);
    for y in 0..image.height {
        for x in 0..image.width {
            let index = (y * image.width + x) as usize;
            if f32::from(image.pixels[index]) < grid.at(x, y) {
                out.pixels[index] = 0;
            }
        }
    }
    let speck = ((dpi / 300.0).powi(2) * 3.0).round().max(1.0) as usize;
    remove_specks(&mut out, speck);
    out
}

/// Retire les taches noires de `max_area` pixels au plus (8-connexité).
fn remove_specks(image: &mut GrayImage, max_area: usize) {
    const VISITED: u8 = 1;
    const KEPT: u8 = 2;
    let (w, h) = (image.width as usize, image.height as usize);
    let mut stack: Vec<usize> = Vec::new();
    let mut component: Vec<usize> = Vec::new();
    for start in 0..image.pixels.len() {
        if image.pixels[start] != 0 {
            continue;
        }
        component.clear();
        stack.clear();
        stack.push(start);
        image.pixels[start] = VISITED;
        while let Some(index) = stack.pop() {
            component.push(index);
            let (x, y) = (index % w, index / w);
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let next = ny * w + nx;
                    if image.pixels[next] == 0 {
                        image.pixels[next] = VISITED;
                        stack.push(next);
                    }
                }
            }
        }
        // Une composante gardée est marquée comme traitée jusqu'à la fin, pour
        // ne pas être parcourue de nouveau.
        let value = if component.len() <= max_area {
            255
        } else {
            KEPT
        };
        for &index in &component {
            image.pixels[index] = value;
        }
    }
    for value in &mut image.pixels {
        if *value == KEPT {
            *value = 0;
        }
    }
}

/// Valeurs définies au centre de chaque case d'une grille, interpolées
/// (bilinéaire) en chaque pixel.
struct Grid {
    values: Vec<f32>,
    width: u32,
    height: u32,
    cell: f32,
}

impl Grid {
    fn at(&self, x: u32, y: u32) -> f32 {
        let fx = ((x as f32 + 0.5) / self.cell - 0.5).clamp(0.0, (self.width - 1) as f32);
        let fy = ((y as f32 + 0.5) / self.cell - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let v = |gx: u32, gy: u32| self.values[(gy * self.width + gx) as usize];
        let top = v(x0, y0) * (1.0 - tx) + v(x1, y0) * tx;
        let bottom = v(x0, y1) * (1.0 - tx) + v(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

// MARK: - Inclinaison et orientation

/// Pixels d'encre d'une image réduite à environ 100 dpi, en coordonnées de
/// cette image réduite.
struct Ink {
    points: Vec<(f32, f32)>,
}

/// Plus grand nombre de pixels d'encre examinés.
const MAX_INK_POINTS: usize = 200_000;

/// Efface de `ink` (image de `width` × `height` pixels à `dpi`) les taches
/// (composantes connexes) qui ne peuvent pas être du texte : trop petites
/// (bruit), plus longues qu'un pouce et demi (filets, bords, lignes de texte
/// fondues par le flou, dans un sens comme dans l'autre) ou plus épaisses
/// qu'un demi-pouce (images).
fn keep_text_shapes(ink: &mut [bool], width: u32, height: u32, dpi: f32) {
    let (w, h) = (width as usize, height as usize);
    let longest = (1.5 * dpi) as usize;
    let thickest = (0.5 * dpi) as usize;
    let mut seen = vec![false; ink.len()];
    let mut stack = Vec::new();
    let mut members = Vec::new();
    for start in 0..ink.len() {
        if !ink[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        members.clear();
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        while let Some(index) = stack.pop() {
            members.push(index);
            let (x, y) = (index % w, index / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            let mut visit = |next: usize| {
                if ink[next] && !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            };
            if x > 0 {
                visit(index - 1);
            }
            if x + 1 < w {
                visit(index + 1);
            }
            if y > 0 {
                visit(index - w);
            }
            if y + 1 < h {
                visit(index + w);
            }
        }
        let (long, short) = {
            let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
            (bw.max(bh), bw.min(bh))
        };
        if long > longest || short > thickest || long < 2 {
            for &index in &members {
                ink[index] = false;
            }
        }
    }
}

impl Ink {
    /// Pixels d'encre (seuil d'Otsu) de l'image réduite. Avec `text_only`,
    /// seuls comptent ceux des taches de la taille d'un caractère ou d'un mot :
    /// ni les filets d'un tableau, ni les bords sombres d'un scan, ni les
    /// grandes images, qui tromperaient la détection d'une page couchée.
    fn sample(image: &GrayImage, dpi: f32, text_only: bool) -> Self {
        Self::sample_at(image, dpi, text_only, 100.0, false)
    }

    /// Comme [`Ink::sample`], l'image réduite à environ `target` ppp. Avec
    /// `jitter`, chaque pixel est placé en un point pris au hasard (de façon
    /// reproductible) dans son carré, et non en son coin : alignés sur la
    /// grille des pixels, les points donneraient aux angles de pente simple
    /// (45°, 26,6°…) des profils artificiellement nets.
    fn sample_at(image: &GrayImage, dpi: f32, text_only: bool, target: f32, jitter: bool) -> Self {
        let factor = (dpi / target).round().max(1.0) as u32;
        let small = image.downscale(factor);
        let threshold = otsu(&small);
        let mut ink: Vec<bool> = small.pixels.iter().map(|&v| v < threshold).collect();
        if text_only {
            let scale = dpi / factor as f32;
            keep_text_shapes(&mut ink, small.width, small.height, scale);
        }
        let total = ink.iter().filter(|&&v| v).count();
        let step = total.div_ceil(MAX_INK_POINTS).max(1);
        let mut points = Vec::with_capacity(total.min(MAX_INK_POINTS));
        let mut seen = 0usize;
        for (index, &is_ink) in ink.iter().enumerate() {
            if is_ink {
                if seen % step == 0 {
                    let x = (index as u32 % small.width) as f32;
                    let y = (index as u32 / small.width) as f32;
                    if jitter {
                        let (dx, dy) = scatter(index as u64);
                        points.push((x + dx, y + dy));
                    } else {
                        points.push((x, y));
                    }
                }
                seen += 1;
            }
        }
        Self { points }
    }

    /// Netteté du profil de projection des pixels selon des lignes inclinées
    /// de `degrees` (ordonnées vers le bas : une ligne qui descend vers la
    /// droite a un angle positif). `transposed` projette sur les colonnes.
    fn score(&self, degrees: f64, transposed: bool) -> f64 {
        if self.points.is_empty() {
            return 0.0;
        }
        let (sin, cos) = degrees.to_radians().sin_cos();
        let project = |&(x, y): &(f32, f32)| -> f64 {
            let (x, y) = if transposed { (y, x) } else { (x, y) };
            f64::from(y) * cos - f64::from(x) * sin
        };
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for point in &self.points {
            let p = project(point);
            low = low.min(p);
            high = high.max(p);
        }
        let bins = (high - low).ceil() as usize + 1;
        let mut profile = vec![0u32; bins];
        for point in &self.points {
            profile[(project(point) - low) as usize] += 1;
        }
        let total = self.points.len() as f64;
        profile
            .windows(2)
            .map(|pair| {
                let delta = f64::from(pair[1]) - f64::from(pair[0]);
                delta * delta
            })
            .sum::<f64>()
            / (total * total)
    }

    /// Angle (en degrés, de −`range` à `range`) qui rend le profil le plus
    /// net, sa netteté et la netteté médiane des angles essayés au pas d'un
    /// demi-degré.
    fn best_angle(&self, range: f64, transposed: bool) -> (f64, f64, f64) {
        let mut best = (0.0, self.score(0.0, transposed));
        let mut coarse = Vec::new();
        let mut search = |from: f64, to: f64, step: f64, best: &mut (f64, f64)| {
            let mut angle = from;
            while angle <= to + 1e-9 {
                let score = self.score(angle, transposed);
                if step >= 0.5 {
                    coarse.push(score);
                }
                if score > best.1 {
                    *best = (angle, score);
                }
                angle += step;
            }
        };
        search(-range, range, 0.5, &mut best);
        let center = best.0;
        search(center - 0.5, center + 0.5, 0.1, &mut best);
        let center = best.0;
        search(center - 0.1, center + 0.1, 0.02, &mut best);
        coarse.sort_unstable_by(f64::total_cmp);
        let median = coarse.get(coarse.len() / 2).copied().unwrap_or(0.0);
        (best.0, best.1, median)
    }
}

/// Décalage reproductible de 0 à 1 pixel sur chaque axe, tiré du rang `seed`
/// du pixel (mélange de SplitMix64).
fn scatter(seed: u64) -> (f32, f32) {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let unit = |bits: u64| (bits & 0xFF_FFFF) as f32 / 16_777_216.0;
    (unit(z), unit(z >> 32))
}

/// Seuil d'Otsu : sépare au mieux l'encre du papier.
fn otsu(image: &GrayImage) -> u8 {
    let mut histogram = [0u64; 256];
    for &value in &image.pixels {
        histogram[value as usize] += 1;
    }
    let total = image.pixels.len() as f64;
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(v, &c)| v as f64 * c as f64)
        .sum();
    let (mut weight_b, mut sum_b) = (0f64, 0f64);
    let (mut best, mut threshold) = (0f64, 128u8);
    for (value, &count) in histogram.iter().enumerate() {
        weight_b += count as f64;
        if weight_b == 0.0 {
            continue;
        }
        let weight_f = total - weight_b;
        if weight_f == 0.0 {
            break;
        }
        sum_b += value as f64 * count as f64;
        let mean_b = sum_b / weight_b;
        let mean_f = (sum - sum_b) / weight_f;
        let between = weight_b * weight_f * (mean_b - mean_f).powi(2);
        if between > best {
            best = between;
            threshold = value as u8 + 1;
        }
    }
    threshold
}

/// Inclinaison des lignes de texte, en degrés (ordonnées vers le bas : des
/// lignes qui descendent vers la droite ont un angle positif), cherchée entre
/// −10° et 10° ; 0 pour une page sans texte.
pub fn estimate_skew(image: &GrayImage, dpi: f32) -> f64 {
    Ink::sample(image, dpi, false).best_angle(10.0, false).0
}

/// Mesure de l'inclinaison d'une page (cf. [`measure_skew`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skew {
    /// Inclinaison des lignes, en degrés (positive : elles descendent vers la
    /// droite).
    pub degrees: f64,
    /// Netteté du profil à cet angle rapportée à la netteté médiane des
    /// angles essayés : proche de 1 quand aucune direction ne se distingue
    /// (photographie, page vide), de plusieurs unités pour des lignes de texte.
    pub prominence: f64,
    /// Nombre de pixels d'encre examinés (réduits à environ 100 ppp).
    pub ink: usize,
}

/// Comme [`estimate_skew`], entre −`range` et `range` degrés, avec la
/// netteté relative de l'angle trouvé, qui dit si la mesure est fiable.
pub fn measure_skew(image: &GrayImage, dpi: f32, range: f64) -> Skew {
    let ink = Ink::sample_at(image, dpi, true, 100.0, true);
    let (degrees, best, median) = ink.best_angle(range, false);
    Skew {
        degrees,
        prominence: if median > 0.0 { best / median } else { 0.0 },
        ink: ink.points.len(),
    }
}

/// Vrai si les lignes de texte sont verticales : la page est couchée et doit
/// être tournée d'un quart de tour pour être lue.
///
/// Projetées sur l'axe vertical, des lignes horizontales alternent bandes
/// pleines et interlignes vides : leur profil varie fortement. Projetées sur
/// l'axe horizontal, elles donnent un profil presque uniforme. On compare la
/// variation relative des deux profils, à l'échelle de quelques points pour ne
/// pas voir le détail des lettres.
pub fn is_sideways(image: &GrayImage, dpi: f32) -> bool {
    let ink = Ink::sample(image, dpi, true);
    if ink.points.len() < 500 {
        return false;
    }
    let best = |transposed: bool| {
        (-5..=5)
            .map(|degrees| ink.contrast(f64::from(degrees), transposed))
            .fold(0.0, f64::max)
    };
    best(true) > best(false) * 1.5
}

/// Rapport de l'encre des hampes à celle des jambages, sur les lignes de
/// texte d'une page inclinée de `skew` degrés : au-dessus de la bande des
/// lettres basses (b, d, h, l, t, majuscules, chiffres) et au-dessous de la
/// ligne de base (g, p, q, y). Dans les écritures latines, les hampes sont
/// bien plus nombreuses : le rapport dépasse nettement 1 sur une page à
/// l'endroit et passe sous 1 sur une page à l'envers. `None` si la page n'a
/// pas assez de lignes de texte pour en juger.
///
/// Les lignes sont repérées sur le profil de projection de l'encre, à
/// environ 200 ppp : une ligne est une suite de rangées d'encre entre deux
/// interlignes vides, et sa bande des lettres basses, les rangées autour du
/// maximum qui en atteignent la moitié.
pub fn ascender_ratio(image: &GrayImage, dpi: f32, skew: f64) -> Option<f64> {
    /// Nombre minimal de lignes examinées.
    const MIN_LINES: usize = 3;
    let ink = Ink::sample_at(image, dpi, true, 200.0, false);
    if ink.points.len() < 2000 {
        return None;
    }
    let (sin, cos) = skew.to_radians().sin_cos();
    let projected: Vec<f64> = ink
        .points
        .iter()
        .map(|&(x, y)| f64::from(y) * cos - f64::from(x) * sin)
        .collect();
    let low = projected.iter().copied().fold(f64::INFINITY, f64::min);
    let high = projected.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let bins = (high - low).ceil() as usize + 1;
    let mut profile = vec![0f64; bins];
    for p in projected {
        profile[(p - low) as usize] += 1.0;
    }
    let mut filled: Vec<f64> = profile.iter().copied().filter(|&v| v > 0.0).collect();
    filled.sort_unstable_by(f64::total_cmp);
    let reference = filled[filled.len() * 9 / 10];
    let gap = (0.04 * reference).max(0.5);

    // Suites de rangées d'encre ; deux suites séparées d'une ou deux rangées
    // presque vides forment une seule ligne.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut y = 0;
    while y < bins {
        if profile[y] <= gap {
            y += 1;
            continue;
        }
        let start = y;
        let mut end = y;
        while y < bins {
            if profile[y] > gap {
                end = y;
                y += 1;
            } else if y + 2 < bins && (profile[y + 1] > gap || profile[y + 2] > gap) {
                y += 1;
            } else {
                break;
            }
        }
        regions.push((start, end));
    }

    let (mut above, mut below, mut lines) = (0.0, 0.0, 0usize);
    for (start, end) in regions {
        let (peak, top) = (start..=end).fold((start, 0.0), |best, y| {
            if profile[y] > best.1 {
                (y, profile[y])
            } else {
                best
            }
        });
        let half = top / 2.0;
        let mut core_start = peak;
        while core_start > start && profile[core_start - 1] >= half {
            core_start -= 1;
        }
        let mut core_end = peak;
        while core_end < end && profile[core_end + 1] >= half {
            core_end += 1;
        }
        let core = core_end - core_start + 1;
        let height = end - start + 1;
        // Trop mince pour être une ligne (bruit, filet), ou plusieurs lignes
        // jointives : ignorée.
        if core < 3 || height > 4 * core {
            continue;
        }
        above += profile[start..core_start].iter().sum::<f64>();
        below += profile[core_end + 1..=end].iter().sum::<f64>();
        lines += 1;
    }
    if lines < MIN_LINES || above + below < 50.0 {
        return None;
    }
    Some((above + 1.0) / (below + 1.0))
}

impl Ink {
    /// Variation relative (variance sur carré de la moyenne) du profil de
    /// projection, par tranches de 3 pixels, entre les 2e et 98e centiles.
    fn contrast(&self, degrees: f64, transposed: bool) -> f64 {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let mut projected: Vec<f64> = self
            .points
            .iter()
            .map(|&(x, y)| {
                let (x, y) = if transposed { (y, x) } else { (x, y) };
                f64::from(y) * cos - f64::from(x) * sin
            })
            .collect();
        projected.sort_unstable_by(f64::total_cmp);
        let n = projected.len();
        let (low, high) = (projected[n / 50], projected[n - 1 - n / 50]);
        let bins = ((high - low) / 3.0).ceil() as usize + 1;
        if bins < 4 {
            return 0.0;
        }
        let mut profile = vec![0f64; bins];
        for &p in &projected {
            if (low..=high).contains(&p) {
                profile[((p - low) / 3.0) as usize] += 1.0;
            }
        }
        let mean = profile.iter().sum::<f64>() / bins as f64;
        let variance = profile.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / bins as f64;
        variance / (mean * mean).max(1e-9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Page de 600 × 400 pixels à 100 dpi : lignes de « mots » noirs.
    fn lines_page(gray_background: bool) -> GrayImage {
        let mut image = GrayImage::new(600, 400, if gray_background { 200 } else { 250 });
        for line in 0..8 {
            let y0 = 40 + line * 40;
            let mut x = 40;
            while x < 540 {
                let word = 20 + (x * 7 + line * 13) % 40;
                for dx in 0..word {
                    // Lettres : traits de hauteur et d'espacement irréguliers.
                    let seed = (x + dx) * 31 + line * 17;
                    if seed % 7 < 3 {
                        let top = if seed % 5 == 0 { y0 - 5 } else { y0 };
                        let bottom = if seed % 11 == 0 { y0 + 19 } else { y0 + 14 };
                        for y in top..bottom {
                            image.pixels[(y * 600 + x + dx) as usize] = 30;
                        }
                    }
                }
                x += word + 15;
            }
        }
        image
    }

    #[test]
    fn mesure_l_inclinaison() {
        let page = lines_page(false);
        assert!(estimate_skew(&page, 100.0).abs() < 0.2);
        // Contenu tourné de 3° dans le sens horaire : les lignes descendent
        // vers la droite.
        let (tilted, _) = page.rotate(3.0, 250);
        let skew = estimate_skew(&tilted, 100.0);
        assert!((skew - 3.0).abs() < 0.2, "{skew}");
        let (tilted, _) = page.rotate(-1.5, 250);
        let skew = estimate_skew(&tilted, 100.0);
        assert!((skew + 1.5).abs() < 0.2, "{skew}");
    }

    #[test]
    fn reconnait_une_page_couchee() {
        let page = lines_page(false);
        assert!(!is_sideways(&page, 100.0));
        assert!(is_sideways(&page.rotate_quarters(1).0, 100.0));
        assert!(!is_sideways(&page.rotate_quarters(2).0, 100.0));
    }

    #[test]
    fn egalise_le_fond_et_binarise() {
        let page = lines_page(true);
        let ink = page.pixels.iter().position(|&v| v == 30).unwrap() as u32;
        let (x, y) = (ink % 600, ink / 600);
        let flat = flatten_background(&page, 100.0);
        // Le papier gris devient blanc, l'encre noire.
        assert_eq!(flat.get(5, 5), 255);
        assert!(flat.get(x, y) < 40);
        let binary = binarize(&page, 100.0);
        assert!(binary.pixels.iter().all(|&v| v == 0 || v == 255));
        assert_eq!(binary.get(5, 5), 255);
        assert_eq!(binary.get(x, y), 0);
    }

    #[test]
    fn retire_les_taches_isolees() {
        let mut image = GrayImage::new(50, 50, 255);
        image.pixels[10 * 50 + 10] = 0;
        for y in 20..30 {
            for x in 20..22 {
                image.pixels[y * 50 + x] = 0;
            }
        }
        remove_specks(&mut image, 3);
        assert_eq!(image.get(10, 10), 255);
        assert_eq!(image.get(20, 25), 0);
        assert_eq!(median3(&image).get(21, 25), 0);
    }
}
