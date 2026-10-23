//! Catalogue des polices candidates : polices fournies avec l'application et
//! familles courantes des documents, installées sur le Mac.
//!
//! Le catalogue ne garde que la description de chaque police ; ses fichiers
//! sont lus à la demande et gardés dans un cache borné, partagé par les
//! polices d'une même collection (`.ttc`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rustybuzz::ttf_parser::{name_id, Tag};
use rustybuzz::Face;

/// Familles cherchées parmi les polices installées : celles des documents
/// bureautiques, administratifs et commerciaux. Les polices fantaisie et
/// d'interface ne sont pas candidates.
pub const DOCUMENT_FAMILIES: &[&str] = &[
    // Linéales.
    "Helvetica",
    "Helvetica Neue",
    "Arial",
    "Arial Narrow",
    "Verdana",
    "Tahoma",
    "Trebuchet MS",
    "Gill Sans",
    "Futura",
    "Avenir",
    "Avenir Next",
    "Optima",
    "Lucida Grande",
    "Geneva",
    "PT Sans",
    "DIN Alternate",
    "Calibri",
    "Segoe UI",
    "Century Gothic",
    "Franklin Gothic Book",
    "Liberation Sans",
    "DejaVu Sans",
    // À empattements.
    "Times New Roman",
    "Times",
    "Georgia",
    "Palatino",
    "Baskerville",
    "Didot",
    "Hoefler Text",
    "Charter",
    "Iowan Old Style",
    "PT Serif",
    "Cambria",
    "Garamond",
    "Book Antiqua",
    "Bookman Old Style",
    "Century Schoolbook",
    "Rockwell",
    "American Typewriter",
    "Liberation Serif",
    "DejaVu Serif",
    // À chasse fixe.
    "Courier",
    "Courier New",
    "Menlo",
    "Monaco",
    "Andale Mono",
    "PT Mono",
    "Consolas",
    "Liberation Mono",
];

/// Taille cumulée des fichiers de police gardés en mémoire.
const FILE_CACHE_BYTES: usize = 128 * 1024 * 1024;

/// Taille maximale d'un fichier de police candidat (les très grandes
/// collections sont des polices asiatiques, sans intérêt ici).
const MAX_FILE_BYTES: u64 = 40 * 1024 * 1024;

/// Description d'une police du catalogue.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceInfo {
    /// Famille (« Helvetica Neue »).
    pub family: String,
    /// Style (« Bold Italic »).
    pub style: String,
    /// Nom PostScript, qui l'identifie (« HelveticaNeue-BoldItalic »).
    pub postscript_name: String,
    /// Fichier.
    pub path: PathBuf,
    /// Indice de la police dans le fichier (collections).
    pub index: u32,
    /// Graisse (400 : normale, 700 : grasse).
    pub weight: u16,
    /// Italique ou oblique.
    pub italic: bool,
    /// Chasse fixe.
    pub monospaced: bool,
    /// À empattements, si la police le déclare.
    pub serif: Option<bool>,
    /// Fournie avec l'application.
    pub bundled: bool,
}

impl FaceInfo {
    /// Graisse grasse ou plus.
    pub fn is_bold(&self) -> bool {
        self.weight >= 600
    }
}

/// Fichier d'une police chargé en mémoire.
pub struct FontFile {
    data: Arc<Vec<u8>>,
    index: u32,
}

impl FontFile {
    /// Police lue dans `data` (indice `index` d'une collection).
    pub fn new(data: Arc<Vec<u8>>, index: u32) -> Option<Self> {
        Face::from_slice(&data, index)?;
        Some(Self { data, index })
    }

    /// Police prête à mettre en forme et à rendre du texte.
    pub fn face(&self) -> Face<'_> {
        // Vérifié à la création.
        Face::from_slice(&self.data, self.index).expect("police vérifiée")
    }
}

/// Polices candidates et cache de leurs fichiers.
#[derive(Default)]
pub struct Catalog {
    faces: Vec<FaceInfo>,
    files: Mutex<FileCache>,
}

#[derive(Default)]
struct FileCache {
    files: HashMap<PathBuf, (Arc<Vec<u8>>, u64)>,
    clock: u64,
    bytes: usize,
}

impl Catalog {
    /// Catalogue vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Catalogue de l'application : polices des dossiers `directories`
    /// (fournies avec l'application), puis familles [`DOCUMENT_FAMILIES`]
    /// installées.
    pub fn standard(directories: &[PathBuf]) -> Self {
        let mut catalog = Self::new();
        for directory in directories {
            catalog.add_directory(directory, true);
        }
        catalog.add_installed_families(DOCUMENT_FAMILIES);
        catalog
    }

    /// Ajoute les polices d'un dossier (`.ttf`, `.otf`, `.ttc`).
    pub fn add_directory(&mut self, directory: &Path, bundled: bool) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc")
                    })
            })
            .collect();
        paths.sort();
        for path in paths {
            self.add_file(&path, None, bundled);
        }
    }

    /// Ajoute les polices installées des familles données. Sur le Mac, les
    /// fichiers sont retrouvés par Core Text (font-kit n'y donne que leurs
    /// données, qu'il faudrait garder en mémoire).
    #[cfg(target_os = "macos")]
    pub fn add_installed_families(&mut self, families: &[&str]) {
        use core_text::font_collection;

        for family in families {
            let Some(descriptors) =
                font_collection::create_for_family(family).and_then(|c| c.get_descriptors())
            else {
                continue;
            };
            for descriptor in descriptors.iter() {
                if let Some(path) = descriptor.font_path() {
                    let name = descriptor.font_name();
                    self.add_file(&path, Some(&Only::Name(name)), false);
                }
            }
        }
    }

    /// Ajoute les polices installées des familles données.
    #[cfg(not(target_os = "macos"))]
    pub fn add_installed_families(&mut self, families: &[&str]) {
        use font_kit::handle::Handle;
        use font_kit::source::SystemSource;

        let source = SystemSource::new();
        for family in families {
            let Ok(handle) = source.select_family_by_name(family) else {
                continue;
            };
            for font in handle.fonts() {
                if let Handle::Path { path, font_index } = font {
                    self.add_file(path, Some(&Only::Index(*font_index)), false);
                }
            }
        }
    }

    /// Ajoute les polices d'un fichier : toutes, ou seulement celle que
    /// désigne `only`.
    fn add_file(&mut self, path: &Path, only: Option<&Only>, bundled: bool) {
        if std::fs::metadata(path).map_or(true, |meta| meta.len() > MAX_FILE_BYTES) {
            return;
        }
        let Some(data) = self.read(path) else {
            return;
        };
        let count = rustybuzz::ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for index in 0..count {
            if matches!(only, Some(Only::Index(only)) if *only != index) {
                continue;
            }
            let Some(info) = describe(&data, index, path, bundled) else {
                continue;
            };
            if matches!(only, Some(Only::Name(name)) if *name != info.postscript_name) {
                continue;
            }
            if self
                .faces
                .iter()
                .any(|face| face.postscript_name == info.postscript_name)
            {
                continue;
            }
            self.faces.push(info);
        }
    }

    /// Polices du catalogue.
    pub fn faces(&self) -> &[FaceInfo] {
        &self.faces
    }

    /// Indice de la police de nom PostScript donné.
    pub fn find(&self, postscript_name: &str) -> Option<usize> {
        self.faces
            .iter()
            .position(|face| face.postscript_name == postscript_name)
    }

    /// Fichier de la police d'indice `face`, lu à la demande.
    pub fn load(&self, face: usize) -> Option<FontFile> {
        let info = self.faces.get(face)?;
        let data = self.read(&info.path)?;
        FontFile::new(data, info.index)
    }

    fn read(&self, path: &Path) -> Option<Arc<Vec<u8>>> {
        let mut cache = self.files.lock().unwrap_or_else(|p| p.into_inner());
        cache.clock += 1;
        let clock = cache.clock;
        if let Some((data, used)) = cache.files.get_mut(path) {
            *used = clock;
            return Some(Arc::clone(data));
        }
        let data = Arc::new(std::fs::read(path).ok()?);
        cache.bytes += data.len();
        cache
            .files
            .insert(path.to_owned(), (Arc::clone(&data), clock));
        // Les fichiers les plus anciens sortent du cache ; ceux qui servent
        // encore restent valables (ils sont partagés).
        while cache.bytes > FILE_CACHE_BYTES && cache.files.len() > 1 {
            let Some(oldest) = cache
                .files
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            if let Some((removed, _)) = cache.files.remove(&oldest) {
                cache.bytes -= removed.len();
            }
        }
        Some(data)
    }
}

/// Police à retenir dans un fichier (collection `.ttc`).
enum Only {
    /// D'indice donné.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Index(u32),
    /// De nom PostScript donné.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Name(String),
}

/// Catalogue partagé par l'application, établi au premier usage.
static SHARED: OnceLock<Arc<Catalog>> = OnceLock::new();
/// Dossiers des polices fournies, donnés par l'application.
static DIRECTORIES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Désigne les dossiers des polices fournies avec l'application ; à appeler
/// avant le premier usage du catalogue partagé.
pub fn set_bundled_directories(directories: Vec<PathBuf>) {
    *DIRECTORIES.lock().unwrap_or_else(|p| p.into_inner()) = directories;
}

/// Catalogue partagé : polices fournies et familles installées.
pub fn shared() -> Arc<Catalog> {
    Arc::clone(SHARED.get_or_init(|| {
        let directories = DIRECTORIES
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        Arc::new(Catalog::standard(&directories))
    }))
}

/// Description de la police `index` du fichier `data`.
fn describe(data: &[u8], index: u32, path: &Path, bundled: bool) -> Option<FaceInfo> {
    let face = Face::from_slice(data, index)?;
    let name = |ids: &[u16]| -> Option<String> {
        ids.iter().find_map(|&id| {
            face.names()
                .into_iter()
                .filter(|name| name.name_id == id)
                .find_map(|name| name.to_string())
                .filter(|text| !text.trim().is_empty())
        })
    };
    let family = name(&[name_id::TYPOGRAPHIC_FAMILY, name_id::FAMILY])?;
    let style = name(&[name_id::TYPOGRAPHIC_SUBFAMILY, name_id::SUBFAMILY])
        .unwrap_or_else(|| "Regular".to_owned());
    let postscript_name =
        name(&[name_id::POST_SCRIPT_NAME]).unwrap_or_else(|| format!("{family}-{style}"));
    // Polices sans contours dessinables (bitmaps, symboles).
    face.glyph_index('a')?;
    Some(FaceInfo {
        family,
        style,
        postscript_name,
        path: path.to_owned(),
        index,
        weight: face.weight().to_number(),
        italic: face.is_italic() || face.is_oblique() || face.italic_angle() < -2.0,
        monospaced: face.is_monospaced(),
        serif: serif_class(&face),
        bundled,
    })
}

/// À empattements ou linéale, d'après la classification PANOSE de la table
/// `OS/2`, sinon sa classe de famille.
fn serif_class(face: &Face) -> Option<bool> {
    let os2 = face.raw_face().table(Tag::from_bytes(b"OS/2"))?;
    // PANOSE : type de famille (2 : texte latin), puis style d'empattement.
    if let (Some(&2), Some(&serif)) = (os2.get(32), os2.get(33)) {
        match serif {
            2..=10 | 14..=15 => return Some(true),
            11..=13 => return Some(false),
            _ => {}
        }
    }
    let class = *os2.get(30)?;
    match class {
        1..=5 | 7 => Some(true),
        8 => Some(false),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Dossier des polices fournies avec l'application.
    pub(crate) fn bundled_directory() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/Resources/Fonts")
    }

    pub(crate) fn bundled() -> Catalog {
        let mut catalog = Catalog::new();
        catalog.add_directory(&bundled_directory(), true);
        catalog
    }

    #[test]
    fn decrit_les_polices_fournies() {
        let catalog = bundled();
        assert_eq!(catalog.faces().len(), 8);
        let bold = &catalog.faces()[catalog.find("Carlito-Bold").unwrap()];
        assert_eq!(bold.family, "Carlito");
        assert!(bold.is_bold() && !bold.italic && bold.bundled);
        let caladea = &catalog.faces()[catalog.find("Caladea-Italic").unwrap()];
        assert!(caladea.italic && !caladea.is_bold());
        assert_eq!(caladea.serif, Some(true));
        assert!(catalog.load(0).is_some());
        assert!(catalog.find("Inconnue").is_none());
    }
}
