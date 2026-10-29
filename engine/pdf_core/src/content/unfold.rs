//! Formulaires dépliés : le contenu d'un formulaire PDF (objet externe
//! `/Subtype /Form`) qui contient du texte est recopié à la place de
//! l'opération `Do` qui le dessine. Ses blocs de texte, images et tracés
//! deviennent ainsi des objets de la page comme les autres.
//!
//! macOS (Quartz) loge tout le contenu de chaque page dans un formulaire
//! quand il exporte ou imprime un document en PDF : sans dépliage, une telle
//! page n'offrait en Édition qu'un seul objet, déplacé d'un bloc, sans texte.
//!
//! Le formulaire est remplacé par `q <Matrix> cm <BBox> re W n … Q`, ce que
//! la norme prescrit pour son affichage. Ses ressources rejoignent celles de
//! la page sous leur nom, ou sous un nom nouveau quand la page emploie déjà
//! ce nom pour un autre objet ; les opérations qui les citent sont alors
//! réécrites. Un formulaire reste entier quand son affichage pourrait
//! changer : groupe de transparence à élimination (knockout), ou combiné à un
//! mode de fusion, une opacité ou un masque ; pile `q`/`Q` qui descend sous
//! son niveau de départ ; image en ligne dont l'espace de couleur serait
//! renommé.

use std::collections::{HashMap, HashSet};

use mupdf::pdf::PdfObject;

use super::geometry::Matrix;
use super::lexer::{self, Operand, Operation};
use super::writer::{write_number, write_operand};

/// Catégories de ressources qu'un flux de contenu cite par leur nom.
const CATEGORIES: [&str; 7] = [
    "Font",
    "XObject",
    "ExtGState",
    "ColorSpace",
    "Pattern",
    "Shading",
    "Properties",
];

/// Profondeur maximale des formulaires imbriqués dépliés.
const MAX_DEPTH: usize = 8;

/// Taille maximale du contenu déplié, en octets : au-delà (formulaire
/// répété des milliers de fois), la page garde ses formulaires entiers.
const MAX_SIZE: usize = 32 * 1024 * 1024;

/// Ressource d'un formulaire déplié, à ajouter aux ressources de la page.
#[derive(Clone, Debug, PartialEq)]
pub struct Alias {
    /// Catégorie (`Font`, `XObject`…).
    pub category: &'static str,
    /// Nom dans les ressources de la page.
    pub name: Vec<u8>,
    /// Noms des formulaires qui mènent, depuis les ressources de la page, au
    /// dictionnaire de ressources qui la porte.
    pub path: Vec<Vec<u8>>,
    /// Nom dans ce dictionnaire.
    pub source: Vec<u8>,
}

/// Contenu d'une page, formulaires à texte dépliés.
#[derive(Debug, Default)]
pub struct Unfolded {
    /// Flux de contenu.
    pub source: Vec<u8>,
    /// Ressources des formulaires dépliés ; vide si rien n'a été déplié.
    pub aliases: Vec<Alias>,
}

/// Déplie les formulaires à texte du flux `source`, dont les ressources sont
/// `resources`. Le flux est rendu tel quel s'il n'en dessine aucun.
pub fn unfold(source: Vec<u8>, resources: Option<&PdfObject>) -> Unfolded {
    // Sans opérateur `Do` ni formulaire dans les ressources, rien à déplier :
    // la page n'est pas lue une seconde fois.
    let has_form = entries(resources, "XObject")
        .iter()
        .any(|(_, object)| is_form(object));
    if !has_form || !source.windows(2).any(|pair| pair == b"Do") {
        return Unfolded {
            source,
            aliases: Vec::new(),
        };
    }
    let resources = resources.and_then(|r| r.try_clone().ok());
    let mut unfolder = Unfolder::default();
    for category in CATEGORIES {
        for (name, object) in entries(resources.as_ref(), category) {
            if let Some(number) = indirect_number(&object) {
                unfolder.known.insert((category, name.clone()), number);
            }
            unfolder.taken.entry(category).or_default().insert(name);
        }
    }
    let root = Scope {
        blends: blending(resources.as_ref()).any(),
        resources,
        path: Vec::new(),
        renames: HashMap::new(),
    };
    match unfolder.expand(&source, &root, 0) {
        Ok(expanded) if !unfolder.unfolded.is_empty() => Unfolded {
            source: expanded,
            aliases: unfolder.aliases,
        },
        _ => Unfolded {
            source,
            aliases: Vec::new(),
        },
    }
}

/// Objet que désigne `alias`, dans les ressources de la page `resources`.
pub fn resolve(resources: Option<&PdfObject>, alias: &Alias) -> Option<PdfObject> {
    let mut dict = resources?.try_clone().ok()?;
    for name in &alias.path {
        let form = lookup(Some(&dict), "XObject", name)?;
        dict = form.get_dict("Resources").ok()??;
    }
    dict.get_dict(alias.category)
        .ok()??
        .get_dict(String::from_utf8_lossy(&alias.source).into_owned())
        .ok()?
}

/// Le contenu déplié dépasserait [`MAX_SIZE`].
struct TooLarge;

#[derive(Default)]
struct Unfolder {
    /// Noms pris dans les ressources de la page, par catégorie.
    taken: HashMap<&'static str, HashSet<Vec<u8>>>,
    /// Objet indirect de chaque ressource de la page, par catégorie et nom.
    known: HashMap<(&'static str, Vec<u8>), i32>,
    aliases: Vec<Alias>,
    /// Formulaires dépliés, par numéro d'objet (0 pour un objet direct).
    unfolded: Vec<i32>,
    /// Formulaires examinés : contiennent-ils du texte ?
    texts: HashMap<i32, bool>,
    size: usize,
}

/// Ressources en vigueur dans un flux.
struct Scope {
    resources: Option<PdfObject>,
    /// Chemin du dictionnaire de ressources (cf. [`Alias::path`]).
    path: Vec<Vec<u8>>,
    /// Noms des ressources dans la page, par catégorie et nom d'origine.
    renames: HashMap<(&'static str, Vec<u8>), Vec<u8>>,
    /// Le flux peut afficher son contenu avec une opacité, un masque ou un
    /// mode de fusion : un groupe de transparence n'y est pas déplié.
    blends: bool,
}

impl Unfolder {
    /// Recopie `source`, formulaires à texte dépliés et ressources renommées.
    fn expand(&mut self, source: &[u8], scope: &Scope, depth: usize) -> Result<Vec<u8>, TooLarge> {
        let ops = lexer::parse(source);
        let mut out = Vec::with_capacity(source.len());
        let mut pos = 0;
        for op in &ops {
            let replacement = match self.inline(op, scope, depth)? {
                Some(bytes) => Some(bytes),
                None => renamed(op, &scope.renames),
            };
            if let Some(bytes) = replacement {
                if op.span.start < pos {
                    continue;
                }
                out.extend_from_slice(&source[pos..op.span.start]);
                out.extend_from_slice(&bytes);
                pos = op.span.end;
            }
        }
        out.extend_from_slice(&source[pos.min(source.len())..]);
        self.size += out.len();
        if self.size > MAX_SIZE {
            return Err(TooLarge);
        }
        Ok(out)
    }

    /// Contenu déplié du formulaire que dessine `op`, ou `None` s'il reste
    /// entier (ce n'est pas un formulaire à texte, ou il ne peut l'être).
    fn inline(
        &mut self,
        op: &Operation,
        scope: &Scope,
        depth: usize,
    ) -> Result<Option<Vec<u8>>, TooLarge> {
        if !op.is("Do") || depth >= MAX_DEPTH {
            return Ok(None);
        }
        let Some(name) = op.operands.last().and_then(Operand::name) else {
            return Ok(None);
        };
        let Some(form) = lookup(scope.resources.as_ref(), "XObject", name) else {
            return Ok(None);
        };
        if !is_form(&form) {
            return Ok(None);
        }
        let Ok(content) = form.read_stream() else {
            return Ok(None);
        };
        let ops = lexer::parse(&content);
        let own = form.get_dict("Resources").ok().flatten().filter(is_dict);
        let resources = own
            .as_ref()
            .and_then(|r| r.try_clone().ok())
            .or_else(|| scope.resources.as_ref().and_then(|r| r.try_clone().ok()));
        if !self.has_text(&form, &ops, resources.as_ref(), depth) {
            return Ok(None);
        }
        let blending = blending(own.as_ref());
        if is_transparency_group(&form) && (scope.blends || blending.mode || is_knockout(&form)) {
            return Ok(None);
        }
        let Some(balance) = balance(&ops) else {
            return Ok(None);
        };

        // Ressources du formulaire, sous leur nom dans la page.
        let saved = (self.taken.clone(), self.known.clone(), self.aliases.len());
        let child = match own {
            Some(own) => {
                let mut path = scope.path.clone();
                path.push(name.to_vec());
                let mut renames = HashMap::new();
                for category in CATEGORIES {
                    for (key, object) in entries(Some(&own), category) {
                        let new = self.allocate(category, &key, &object, &path);
                        renames.insert((category, key), new);
                    }
                }
                Scope {
                    resources: Some(own),
                    path,
                    renames,
                    blends: scope.blends || blending.any(),
                }
            }
            None => Scope {
                resources,
                path: scope.path.clone(),
                renames: scope.renames.clone(),
                blends: scope.blends,
            },
        };
        if renames_inline_image(&ops, &child.renames) {
            (self.taken, self.known) = (saved.0, saved.1);
            self.aliases.truncate(saved.2);
            return Ok(None);
        }

        let body = self.expand(&content, &child, depth + 1)?;
        let mut out = b"\nq\n".to_vec();
        let matrix = numbers(&form, "Matrix")
            .filter(|v| v.len() == 6)
            .map(|v| Matrix::from_slice(&v))
            .unwrap_or(Matrix::IDENTITY);
        if matrix != Matrix::IDENTITY {
            for value in [matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f] {
                write_number(&mut out, value);
                out.push(b' ');
            }
            out.extend_from_slice(b"cm\n");
        }
        if let Some(b) = numbers(&form, "BBox").filter(|v| v.len() == 4) {
            let (x0, x1) = (b[0].min(b[2]), b[0].max(b[2]));
            let (y0, y1) = (b[1].min(b[3]), b[1].max(b[3]));
            for value in [x0, y0, x1 - x0, y1 - y0] {
                write_number(&mut out, value);
                out.push(b' ');
            }
            out.extend_from_slice(b"re W n\n");
        }
        out.extend_from_slice(&body);
        out.push(b'\n');
        if balance.open_text {
            out.extend_from_slice(b"ET\n");
        }
        for _ in 0..balance.open_saves {
            out.extend_from_slice(b"Q\n");
        }
        out.extend_from_slice(b"Q\n");
        self.unfolded.push(indirect_number(&form).unwrap_or(0));
        Ok(Some(out))
    }

    /// Vrai si le formulaire `form` (opérations `ops`, ressources
    /// `resources`) affiche du texte, lui-même ou par un formulaire imbriqué.
    fn has_text(
        &mut self,
        form: &PdfObject,
        ops: &[Operation],
        resources: Option<&PdfObject>,
        depth: usize,
    ) -> bool {
        let number = indirect_number(form);
        if let Some(&known) = number.and_then(|n| self.texts.get(&n)) {
            return known;
        }
        // Un formulaire qui se dessine lui-même ne contient pas de texte de
        // plus : la valeur provisoire coupe le cycle.
        if let Some(number) = number {
            self.texts.insert(number, false);
        }
        let mut found = ops.iter().any(|op| op.is("BT"));
        if !found && depth + 1 < MAX_DEPTH {
            for op in ops.iter().filter(|op| op.is("Do")) {
                let Some(name) = op.operands.last().and_then(Operand::name) else {
                    continue;
                };
                let Some(inner) = lookup(resources, "XObject", name).filter(is_form) else {
                    continue;
                };
                let Ok(content) = inner.read_stream() else {
                    continue;
                };
                let inner_ops = lexer::parse(&content);
                let inner_resources = inner
                    .get_dict("Resources")
                    .ok()
                    .flatten()
                    .filter(is_dict)
                    .or_else(|| resources.and_then(|r| r.try_clone().ok()));
                if self.has_text(&inner, &inner_ops, inner_resources.as_ref(), depth + 1) {
                    found = true;
                    break;
                }
            }
        }
        if let Some(number) = number {
            self.texts.insert(number, found);
        }
        found
    }

    /// Nom dans la page de la ressource `key` d'un formulaire : le même si
    /// la page n'emploie pas ce nom ou y désigne le même objet, sinon un nom
    /// libre.
    fn allocate(
        &mut self,
        category: &'static str,
        key: &[u8],
        object: &PdfObject,
        path: &[Vec<u8>],
    ) -> Vec<u8> {
        let number = indirect_number(object);
        if number.is_some() && self.known.get(&(category, key.to_vec())) == number.as_ref() {
            return key.to_vec();
        }
        let taken = self.taken.entry(category).or_default();
        let mut name = key.to_vec();
        let mut suffix = 1;
        while taken.contains(&name) {
            name = key.to_vec();
            name.extend_from_slice(format!("_{suffix}").as_bytes());
            suffix += 1;
        }
        taken.insert(name.clone());
        if let Some(number) = number {
            self.known.insert((category, name.clone()), number);
        }
        self.aliases.push(Alias {
            category,
            name: name.clone(),
            path: path.to_vec(),
            source: key.to_vec(),
        });
        name
    }
}

/// Opération réécrite avec le nouveau nom de la ressource qu'elle cite, ou
/// `None` si ce nom ne change pas.
fn renamed(op: &Operation, renames: &HashMap<(&'static str, Vec<u8>), Vec<u8>>) -> Option<Vec<u8>> {
    if renames.is_empty() {
        return None;
    }
    let last = op.operands.len().checked_sub(1)?;
    let (category, index) = match op.operator.as_slice() {
        b"Tf" => ("Font", 0),
        b"Do" => ("XObject", last),
        b"gs" => ("ExtGState", 0),
        b"cs" | b"CS" => ("ColorSpace", 0),
        b"scn" | b"SCN" => ("Pattern", last),
        b"sh" => ("Shading", 0),
        b"BDC" | b"DP" => ("Properties", 1),
        _ => return None,
    };
    let name = op.operands.get(index)?.name()?;
    let new = renames.get(&(category, name.to_vec()))?;
    if new.as_slice() == name {
        return None;
    }
    let mut out = b" ".to_vec();
    for (position, operand) in op.operands.iter().enumerate() {
        if position == index {
            write_operand(&mut out, &Operand::Name(new.clone()));
        } else {
            write_operand(&mut out, operand);
        }
        out.push(b' ');
    }
    out.extend_from_slice(&op.operator);
    out.push(b'\n');
    Some(out)
}

/// Vrai si une image en ligne de `ops` cite un espace de couleur renommé :
/// son dictionnaire ne serait pas réécrit.
fn renames_inline_image(
    ops: &[Operation],
    renames: &HashMap<(&'static str, Vec<u8>), Vec<u8>>,
) -> bool {
    ops.iter().filter(|op| op.is("BI")).any(|op| {
        let Some(Operand::Dict(entries)) = op.operands.first() else {
            return false;
        };
        entries.iter().any(|(key, value)| {
            matches!(key.as_slice(), b"CS" | b"ColorSpace")
                && value.name().is_some_and(|name| {
                    renames
                        .get(&("ColorSpace", name.to_vec()))
                        .is_some_and(|new| new.as_slice() != name)
                })
        })
    })
}

/// Ce que laisse ouvert le contenu d'un formulaire.
struct Balance {
    /// `q` sans `Q`.
    open_saves: usize,
    /// Objet texte (`BT`) non refermé.
    open_text: bool,
}

/// Équilibre des `q`/`Q` et des objets texte de `ops` ; `None` si un `Q`
/// referme un `q` d'avant le formulaire.
fn balance(ops: &[Operation]) -> Option<Balance> {
    let mut depth = 0usize;
    let mut text = false;
    for op in ops {
        match op.operator.as_slice() {
            b"q" => depth += 1,
            b"Q" => depth = depth.checked_sub(1)?,
            b"BT" => text = true,
            b"ET" => text = false,
            _ => {}
        }
    }
    Some(Balance {
        open_saves: depth,
        open_text: text,
    })
}

/// Effets de transparence que permettent les états graphiques (`ExtGState`)
/// d'un dictionnaire de ressources.
struct Blending {
    /// Mode de fusion autre que `Normal`.
    mode: bool,
    /// Opacité inférieure à 1 ou masque.
    alpha: bool,
}

impl Blending {
    fn any(&self) -> bool {
        self.mode || self.alpha
    }
}

fn blending(resources: Option<&PdfObject>) -> Blending {
    let mut result = Blending {
        mode: false,
        alpha: false,
    };
    for (_, state) in entries(resources, "ExtGState") {
        if let Ok(Some(mode)) = state.get_dict("BM") {
            let normal = |object: &PdfObject| {
                object
                    .as_name()
                    .is_ok_and(|name| name == b"Normal" || name == b"Compatible")
            };
            let is_normal = if mode.is_array().unwrap_or(false) {
                mode.get_array(0).ok().flatten().is_some_and(|m| normal(&m))
            } else {
                normal(&mode)
            };
            result.mode |= !is_normal;
        }
        for key in ["CA", "ca"] {
            if let Ok(Some(value)) = state.get_dict(key) {
                result.alpha |= value.as_float().is_ok_and(|v| v < 1.0);
            }
        }
        if let Ok(Some(mask)) = state.get_dict("SMask") {
            result.alpha |= !mask.as_name().is_ok_and(|name| name == b"None");
        }
    }
    result
}

fn is_form(object: &PdfObject) -> bool {
    name(object, "Subtype").as_deref() == Some(b"Form")
}

fn is_transparency_group(form: &PdfObject) -> bool {
    form.get_dict("Group")
        .ok()
        .flatten()
        .is_some_and(|group| name(&group, "S").as_deref() == Some(b"Transparency"))
}

fn is_knockout(form: &PdfObject) -> bool {
    form.get_dict("Group")
        .ok()
        .flatten()
        .and_then(|group| group.get_dict("K").ok().flatten())
        .is_some_and(|k| k.as_bool().unwrap_or(false))
}

fn is_dict(object: &PdfObject) -> bool {
    object.is_dict().unwrap_or(false)
}

/// Entrées d'une catégorie de ressources : nom et valeur (référence
/// indirecte non résolue).
fn entries(resources: Option<&PdfObject>, category: &str) -> Vec<(Vec<u8>, PdfObject)> {
    let Some(dict) = resources
        .and_then(|r| r.get_dict(category).ok().flatten())
        .filter(is_dict)
    else {
        return Vec::new();
    };
    let Ok(iter) = dict.dict_iter() else {
        return Vec::new();
    };
    iter.filter_map(|entry| {
        let (key, value) = entry.ok()?;
        Some((key.as_name().ok()?, value))
    })
    .collect()
}

fn lookup(resources: Option<&PdfObject>, category: &str, name: &[u8]) -> Option<PdfObject> {
    resources?
        .get_dict(category)
        .ok()??
        .get_dict(String::from_utf8_lossy(name).into_owned())
        .ok()?
}

fn indirect_number(object: &PdfObject) -> Option<i32> {
    if object.is_indirect().ok()? {
        object.as_indirect().ok()
    } else {
        None
    }
}

fn name(object: &PdfObject, key: &str) -> Option<Vec<u8>> {
    let value = object.get_dict(key).ok()??;
    value.as_name().ok()
}

fn numbers(object: &PdfObject, key: &str) -> Option<Vec<f64>> {
    let array = object.get_dict(key).ok()??;
    if !array.is_array().ok()? {
        return None;
    }
    let mut values = Vec::new();
    for index in 0..array.len().ok()? {
        let item = array.get_array(index as i32).ok()??;
        values.push(f64::from(item.as_float().ok()?));
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(source: &str) -> Operation {
        lexer::parse(source.as_bytes()).remove(0)
    }

    fn renames(
        entries: &[(&'static str, &str, &str)],
    ) -> HashMap<(&'static str, Vec<u8>), Vec<u8>> {
        entries
            .iter()
            .map(|(category, old, new)| {
                (
                    (*category, old.as_bytes().to_vec()),
                    new.as_bytes().to_vec(),
                )
            })
            .collect()
    }

    #[test]
    fn renomme_les_ressources_citees() {
        let map = renames(&[
            ("Font", "G1", "G1_1"),
            ("XObject", "Im1", "Im1_1"),
            ("Properties", "MC0", "MC0_1"),
            ("Pattern", "P0", "P0_1"),
        ]);
        let text = |s: &str| renamed(&op(s), &map).map(|b| String::from_utf8(b).unwrap());
        assert_eq!(text("/G1 12 Tf").as_deref(), Some(" /G1_1 12 Tf\n"));
        assert_eq!(text("/Im1 Do").as_deref(), Some(" /Im1_1 Do\n"));
        assert_eq!(text("/OC /MC0 BDC").as_deref(), Some(" /OC /MC0_1 BDC\n"));
        assert_eq!(
            text("0.5 0.2 0.1 /P0 scn").as_deref(),
            Some(" 0.5 0.2 0.1 /P0_1 scn\n")
        );
        // Nom inchangé, autre catégorie, opérateur sans ressource.
        assert_eq!(text("/G2 12 Tf"), None);
        assert_eq!(text("/G1 gs"), None);
        assert_eq!(text("1 0 0 1 0 0 cm"), None);
    }

    #[test]
    fn equilibre_des_sauvegardes() {
        let ops = lexer::parse(b"q q BT (a) Tj Q");
        let open = balance(&ops).unwrap();
        assert_eq!(open.open_saves, 1);
        assert!(open.open_text);
        assert!(balance(&lexer::parse(b"q Q Q")).is_none());
        let closed = balance(&lexer::parse(b"q BT ET Q")).unwrap();
        assert_eq!(closed.open_saves, 0);
        assert!(!closed.open_text);
    }

    #[test]
    fn image_en_ligne_dont_l_espace_serait_renomme() {
        let ops = lexer::parse(b"BI /W 1 /H 1 /CS /CS0 /BPC 8 ID \x00\x00\x00 EI");
        assert!(renames_inline_image(
            &ops,
            &renames(&[("ColorSpace", "CS0", "CS0_1")])
        ));
        assert!(!renames_inline_image(
            &ops,
            &renames(&[("ColorSpace", "CS0", "CS0")])
        ));
        assert!(!renames_inline_image(
            &ops,
            &renames(&[("Font", "CS0", "CS0_1")])
        ));
    }
}
