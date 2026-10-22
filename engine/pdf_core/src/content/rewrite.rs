//! Réécriture du flux de contenu d'une page après modification de ses objets.
//!
//! La réécriture est chirurgicale : seules les opérations des objets modifiés
//! changent, le reste du flux est recopié octet pour octet.
//!
//! - Un glyphe effacé devient, dans son tableau `TJ`, un décalage égal à son
//!   avance : le texte qui le suit garde sa place.
//! - Un texte déplacé est effacé, puis redessiné juste après son objet texte
//!   (`ET`), dans un groupe `q … Q` qui rétablit son état graphique, avec sa
//!   matrice de texte décalée.
//! - Un texte remplacé est effacé, puis le nouveau texte, mis en page par
//!   [`super::layout`], est dessiné au même endroit, de la même façon.
//! - Un objet autre que du texte est supprimé en retirant ses plages
//!   d'opérations, déplacé en les entourant de `q M cm … Q`. Un tracé qui
//!   détoure ce qui suit garde son détourage (`W n`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use super::font::code_bytes;
use super::geometry::Matrix;
use super::interp::{GraphicsState, Interpretation, ShowItem};
use super::layout::{LaidItem, Layout};
use super::lexer::Operation;
use super::objects::PageObject;
use super::writer::{write_hex, write_name, write_number, write_op};

/// Nouveau texte d'un bloc.
#[derive(Clone, Debug)]
pub struct NewText {
    /// Texte mis en page.
    pub layout: Layout,
    /// Nom de ressource de la police qui le dessine.
    pub font: Vec<u8>,
}

/// Modification d'un objet.
#[derive(Clone, Debug, Default)]
pub struct Change {
    /// L'objet est supprimé.
    pub deleted: bool,
    /// Déplacement, dans l'espace de la page PDF.
    pub offset: (f64, f64),
    /// Nouveau texte (bloc de texte modifiable seulement).
    pub text: Option<NewText>,
}

impl Change {
    fn moved(&self) -> bool {
        self.offset != (0.0, 0.0)
    }
}

/// Contenu à écrire à la place des opérations d'origine.
#[derive(Default)]
struct Edits {
    /// Opérations remplacées une à une.
    ops: HashMap<usize, Vec<u8>>,
    /// Plages d'opérations remplacées, par première opération.
    ranges: BTreeMap<usize, (usize, Vec<u8>)>,
    /// Contenu ajouté après une opération (`ops.len()` : en fin de flux).
    after: BTreeMap<usize, Vec<u8>>,
    /// Un objet texte reste ouvert en fin de flux et a déjà été fermé.
    closed_at_end: bool,
}

/// Réécrit le flux `source` (lu en `ops` et interprété en `run`) en appliquant
/// `changes` aux objets `objects`, désignés par leur numéro.
pub fn rewrite(
    source: &[u8],
    ops: &[Operation],
    run: &Interpretation,
    objects: &[PageObject],
    changes: &BTreeMap<u32, Change>,
) -> Vec<u8> {
    let mut edits = Edits::default();
    let mut removed: HashSet<usize> = HashSet::new();
    for (&id, change) in changes {
        let Some(object) = objects.get(id as usize).filter(|object| object.id == id) else {
            continue;
        };
        if !object.movable || (!change.deleted && !change.moved() && change.text.is_none()) {
            continue;
        }
        match &object.text {
            Some(block) => {
                let new_text = change.text.as_ref().filter(|_| object.editable);
                if !change.deleted && !change.moved() && new_text.is_none() {
                    continue;
                }
                let glyphs: Vec<usize> = block
                    .lines
                    .iter()
                    .flat_map(|l| l.glyphs.iter().copied())
                    .collect();
                removed.extend(glyphs.iter().copied());
                if change.deleted {
                    continue;
                }
                match new_text {
                    Some(new_text) => {
                        let first = &run.glyphs[block.lines[0].glyphs[0]];
                        let snippet = replaced_text(run, new_text, change.offset, |state| {
                            insertion_ctm(run, first.text_object, state)
                        });
                        insert_after_text_object(&mut edits, run, ops, first.text_object, snippet);
                    }
                    None => moved_text(&mut edits, run, ops, &glyphs, change.offset),
                }
            }
            None => {
                for segment in &object.segments {
                    let replacement =
                        segment_replacement(source, ops, run, object, segment, change);
                    edits
                        .ranges
                        .insert(segment.start, (segment.end, replacement));
                }
            }
        }
    }

    let mut touched: Vec<usize> = removed.iter().map(|&g| run.glyphs[g].op).collect();
    touched.sort_unstable();
    touched.dedup();
    for op in touched {
        if let Some(items) = run.shows.get(&op) {
            edits
                .ops
                .insert(op, show_without(&ops[op], items, run, &removed));
        }
    }
    apply(source, ops, edits)
}

/// Recopie le flux avec les modifications.
fn apply(source: &[u8], ops: &[Operation], mut edits: Edits) -> Vec<u8> {
    let mut out = Vec::with_capacity(source.len() + 256);
    let mut cursor = 0;
    let mut index = 0;
    while index < ops.len() {
        let op = &ops[index];
        if let Some((end, replacement)) = edits.ranges.remove(&index) {
            let end = end.clamp(index + 1, ops.len());
            out.extend_from_slice(&source[cursor..op.span.start]);
            out.push(b'\n');
            out.extend_from_slice(&replacement);
            cursor = ops[end - 1].span.end;
            // Contenu ajouté après une opération de la plage : après la plage.
            for inner in index..end {
                if let Some(snippet) = edits.after.remove(&inner) {
                    out.push(b'\n');
                    out.extend_from_slice(&snippet);
                }
            }
            index = end;
            continue;
        }
        if let Some(replacement) = edits.ops.remove(&index) {
            out.extend_from_slice(&source[cursor..op.span.start]);
            out.push(b'\n');
            out.extend_from_slice(&replacement);
            cursor = op.span.end;
        }
        if let Some(snippet) = edits.after.remove(&index) {
            out.extend_from_slice(&source[cursor..op.span.end.max(cursor)]);
            cursor = op.span.end.max(cursor);
            out.push(b'\n');
            out.extend_from_slice(&snippet);
        }
        index += 1;
    }
    out.extend_from_slice(&source[cursor.min(source.len())..]);
    if let Some(snippet) = edits.after.remove(&ops.len()) {
        out.push(b'\n');
        out.extend_from_slice(&snippet);
    }
    out
}

/// Opération d'affichage de texte sans les glyphes `removed`, remplacés par
/// leur avance.
fn show_without(
    op: &Operation,
    items: &[ShowItem],
    run: &Interpretation,
    removed: &HashSet<usize>,
) -> Vec<u8> {
    let mut out = Vec::new();
    match op.operator.as_slice() {
        b"'" => write_op(&mut out, &[], "T*"),
        b"\"" => {
            write_op(&mut out, &[op.number(0)], "Tw");
            write_op(&mut out, &[op.number(1)], "Tc");
            write_op(&mut out, &[], "T*");
        }
        _ => {}
    }
    let mut array = TjArray::default();
    for item in items {
        match *item {
            ShowItem::Kern(kern) => array.kern(kern),
            ShowItem::Glyph(g) => {
                let glyph = &run.glyphs[g];
                if removed.contains(&g) {
                    array.kern(advance_kern(run, g));
                } else {
                    array.codes(&code_bytes(glyph.code, glyph.code_len));
                }
            }
        }
    }
    array.write(&mut out);
    out
}

/// Décalage `TJ` équivalent à l'avance d'un glyphe.
fn advance_kern(run: &Interpretation, g: usize) -> f64 {
    let glyph = &run.glyphs[g];
    let state = &run.states[glyph.state];
    let word = if glyph.code_len == 1 && glyph.code == 32 {
        state.word_spacing
    } else {
        0.0
    };
    if state.font_size == 0.0 {
        return 0.0;
    }
    -(glyph.width + (state.char_spacing + word) * 1000.0 / state.font_size)
}

/// Tableau d'une opération `TJ` en construction.
#[derive(Default)]
struct TjArray {
    items: Vec<LaidItem>,
}

impl TjArray {
    fn codes(&mut self, bytes: &[u8]) {
        if let Some(LaidItem::Codes(codes)) = self.items.last_mut() {
            codes.extend_from_slice(bytes);
        } else {
            self.items.push(LaidItem::Codes(bytes.to_vec()));
        }
    }

    fn kern(&mut self, kern: f64) {
        if let Some(LaidItem::Kern(previous)) = self.items.last_mut() {
            *previous += kern;
        } else {
            self.items.push(LaidItem::Kern(kern));
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_tj(out, &self.items);
    }
}

fn write_tj(out: &mut Vec<u8>, items: &[LaidItem]) {
    out.push(b'[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        match item {
            LaidItem::Codes(codes) => write_hex(out, codes),
            LaidItem::Kern(kern) => write_number(out, *kern),
        }
    }
    out.extend_from_slice(b"] TJ\n");
}

/// Matrice qui décale de `offset` (espace de la page) ce qui est dessiné sous
/// la matrice `ctm` : `ctm × T(offset) × ctm⁻¹`.
fn offset_matrix(ctm: &Matrix, offset: (f64, f64)) -> Option<Matrix> {
    Some(
        ctm.then(&Matrix::translate(offset.0, offset.1))
            .then(&ctm.inverse()?),
    )
}

/// Matrice en vigueur là où le texte de l'objet texte `object` est redessiné.
fn insertion_ctm(run: &Interpretation, object: usize, state: &GraphicsState) -> Matrix {
    match run.text_objects[object].end {
        Some(end) => run.ctm_before.get(end).copied().unwrap_or(state.ctm),
        None => state.ctm,
    }
}

/// Ajoute `snippet` après l'objet texte `object` (ou en fin de flux, en le
/// fermant, s'il n'est pas fermé).
fn insert_after_text_object(
    edits: &mut Edits,
    run: &Interpretation,
    ops: &[Operation],
    object: usize,
    snippet: Vec<u8>,
) {
    let (key, close) = match run.text_objects[object].end {
        Some(end) => (end, false),
        None => (ops.len(), true),
    };
    let entry = edits.after.entry(key).or_default();
    if close && !edits.closed_at_end {
        edits.closed_at_end = true;
        write_op(entry, &[], "ET");
    }
    entry.extend_from_slice(&snippet);
}

/// Ouvre un groupe `q` qui rétablit l'état graphique `state` là où la matrice
/// en vigueur est `ctm_here`, puis un objet texte avec l'état du texte.
fn open_text(
    out: &mut Vec<u8>,
    state: &GraphicsState,
    ctm_here: &Matrix,
    font: &[u8],
    word_spacing: f64,
) {
    write_op(out, &[], "q");
    if !nearly_equal(&state.ctm, ctm_here) {
        if let Some(inverse) = ctm_here.inverse() {
            let m = state.ctm.then(&inverse);
            write_op(out, &[m.a, m.b, m.c, m.d, m.e, m.f], "cm");
        }
    }
    for gs in &state.ext_gstates {
        out.extend_from_slice(gs);
        out.push(b'\n');
    }
    if state.strokes_text() {
        for (_, line_op) in &state.line_ops {
            out.extend_from_slice(line_op);
            out.push(b'\n');
        }
    }
    for (color, default) in [(&state.stroke, "0 G"), (&state.fill, "0 g")] {
        if color.space.is_none() && color.color.is_none() {
            out.extend_from_slice(default.as_bytes());
            out.push(b'\n');
        }
        for bytes in [&color.space, &color.color].into_iter().flatten() {
            out.extend_from_slice(bytes);
            out.push(b'\n');
        }
    }
    write_op(out, &[], "BT");
    write_name(out, font);
    out.push(b' ');
    write_op(out, &[state.font_size], "Tf");
    write_op(out, &[state.char_spacing], "Tc");
    write_op(out, &[word_spacing], "Tw");
    write_op(out, &[state.h_scale * 100.0], "Tz");
    write_op(out, &[state.rise], "Ts");
    write_op(out, &[f64::from(state.render_mode)], "Tr");
}

fn close_text(out: &mut Vec<u8>) {
    write_op(out, &[], "ET");
    write_op(out, &[], "Q");
}

fn write_tm(out: &mut Vec<u8>, m: &Matrix) {
    write_op(out, &[m.a, m.b, m.c, m.d, m.e, m.f], "Tm");
}

fn nearly_equal(a: &Matrix, b: &Matrix) -> bool {
    [
        a.a - b.a,
        a.b - b.b,
        a.c - b.c,
        a.d - b.d,
        a.e - b.e,
        a.f - b.f,
    ]
    .iter()
    .all(|d| d.abs() < 1e-6)
}

/// Redessine les glyphes `glyphs`, décalés, après leurs objets texte.
fn moved_text(
    edits: &mut Edits,
    run: &Interpretation,
    ops: &[Operation],
    glyphs: &[usize],
    offset: (f64, f64),
) {
    let members: HashSet<usize> = glyphs.iter().copied().collect();
    let mut show_ops: Vec<usize> = glyphs.iter().map(|&g| run.glyphs[g].op).collect();
    show_ops.sort_unstable();
    show_ops.dedup();

    // Suites de glyphes consécutifs du bloc, par opération : (glyphes et
    // décalages entre eux).
    let mut runs: Vec<Vec<ShowItem>> = Vec::new();
    for op in show_ops {
        let Some(items) = run.shows.get(&op) else {
            continue;
        };
        let mut current: Vec<ShowItem> = Vec::new();
        for item in items {
            match *item {
                ShowItem::Glyph(g) if members.contains(&g) => current.push(*item),
                ShowItem::Glyph(_) => {
                    if !current.is_empty() {
                        runs.push(std::mem::take(&mut current));
                    }
                }
                ShowItem::Kern(_) => {
                    if !current.is_empty() {
                        current.push(*item);
                    }
                }
            }
        }
        if !current.is_empty() {
            runs.push(current);
        }
    }

    // Un fragment par objet texte, un groupe par état graphique.
    let mut by_object: BTreeMap<usize, Vec<Vec<ShowItem>>> = BTreeMap::new();
    for items in runs {
        let Some(ShowItem::Glyph(first)) = items.first() else {
            continue;
        };
        by_object
            .entry(run.glyphs[*first].text_object)
            .or_default()
            .push(items);
    }
    for (object, runs) in by_object {
        let mut out = Vec::new();
        let mut open: Option<usize> = None;
        for items in runs {
            let Some(&ShowItem::Glyph(first)) = items.first() else {
                continue;
            };
            let glyph = &run.glyphs[first];
            let state = &run.states[glyph.state];
            let Some(font) = state.font_name.as_deref() else {
                continue;
            };
            let Some(shift) = offset_matrix(&state.ctm, offset) else {
                continue;
            };
            if open != Some(glyph.state) {
                if open.is_some() {
                    close_text(&mut out);
                }
                open_text(
                    &mut out,
                    state,
                    &insertion_ctm(run, object, state),
                    font,
                    state.word_spacing,
                );
                open = Some(glyph.state);
            }
            write_tm(&mut out, &glyph.tm.then(&shift));
            let mut array = TjArray::default();
            let end = items
                .iter()
                .rposition(|item| matches!(item, ShowItem::Glyph(_)))
                .unwrap_or(0);
            for item in &items[..=end] {
                match *item {
                    ShowItem::Glyph(g) => {
                        let glyph = &run.glyphs[g];
                        array.codes(&code_bytes(glyph.code, glyph.code_len));
                    }
                    ShowItem::Kern(kern) => array.kern(kern),
                }
            }
            if !array.is_empty() {
                array.write(&mut out);
            }
        }
        if open.is_some() {
            close_text(&mut out);
            insert_after_text_object(edits, run, ops, object, out);
        }
    }
}

/// Dessine le nouveau texte d'un bloc, décalé de `offset`.
fn replaced_text(
    run: &Interpretation,
    new_text: &NewText,
    offset: (f64, f64),
    ctm_here: impl Fn(&GraphicsState) -> Matrix,
) -> Vec<u8> {
    let layout = &new_text.layout;
    let state = &run.states[layout.state];
    let shift = offset_matrix(&state.ctm, offset).unwrap_or(Matrix::IDENTITY);
    let frame = layout.frame.then(&shift);
    let mut out = Vec::new();
    // Les espaces sont dessinées avec leur chasse mesurée : pas
    // d'espacement des mots en plus.
    open_text(&mut out, state, &ctm_here(state), &new_text.font, 0.0);
    for line in &layout.lines {
        if line.items.is_empty() {
            continue;
        }
        write_tm(&mut out, &Matrix::translate(line.x, line.y).then(&frame));
        write_tj(&mut out, &line.items);
    }
    close_text(&mut out);
    out
}

/// Contenu qui remplace une plage d'opérations d'un objet supprimé ou déplacé.
fn segment_replacement(
    source: &[u8],
    ops: &[Operation],
    run: &Interpretation,
    object: &PageObject,
    segment: &Range<usize>,
    change: &Change,
) -> Vec<u8> {
    let bytes = &source[ops[segment.start].span.start..ops[segment.end - 1].span.end];
    let mut out = Vec::new();
    // Un tracé hors groupe qui détoure la suite : son détourage reste.
    let clip = object
        .units
        .iter()
        .map(|&u| &run.units[u])
        .any(|unit| unit.clips && unit.ops == *segment);
    if clip && segment.len() >= 2 {
        let construction = &source[ops[segment.start].span.start..ops[segment.end - 2].span.end];
        out.extend_from_slice(construction);
        out.push(b'\n');
        write_op(&mut out, &[], "n");
    }
    if change.deleted {
        return out;
    }
    let ctm = run
        .ctm_before
        .get(segment.start)
        .copied()
        .unwrap_or(Matrix::IDENTITY);
    match offset_matrix(&ctm, change.offset) {
        Some(m) => {
            write_op(&mut out, &[], "q");
            write_op(&mut out, &[m.a, m.b, m.c, m.d, m.e, m.f], "cm");
            out.extend_from_slice(bytes);
            out.push(b'\n');
            write_op(&mut out, &[], "Q");
        }
        None => {
            if !clip {
                out.extend_from_slice(bytes);
                out.push(b'\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::geometry::{Point, Rect};
    use crate::content::interp::{interpret, tests::test_font, tests::TestResources};
    use crate::content::layout::{layout, Shaper};
    use crate::content::lexer::parse;
    use crate::content::objects::{build, lines};

    struct Page {
        source: Vec<u8>,
        ops: Vec<Operation>,
        run: Interpretation,
        objects: Vec<PageObject>,
    }

    fn page(source: &[u8]) -> Page {
        let ops = parse(source);
        let mut resources = TestResources { font: test_font() };
        let page_box = Rect::new(0.0, 0.0, 600.0, 800.0);
        let run = interpret(&ops, &mut resources, page_box);
        let objects = build(&ops, &run, page_box);
        Page {
            source: source.to_vec(),
            ops,
            run,
            objects,
        }
    }

    impl Page {
        fn edit(&self, changes: &[(u32, Change)]) -> Page {
            let changes: BTreeMap<u32, Change> = changes.iter().cloned().collect();
            page(&rewrite(
                &self.source,
                &self.ops,
                &self.run,
                &self.objects,
                &changes,
            ))
        }

        /// Texte et origine de chaque ligne visible.
        fn lines(&self) -> Vec<(String, Point)> {
            lines(&self.run)
                .into_iter()
                .map(|line| (line.text.clone(), self.run.glyphs[line.glyphs[0]].origin))
                .collect()
        }
    }

    fn deleted() -> Change {
        Change {
            deleted: true,
            ..Change::default()
        }
    }

    fn moved(dx: f64, dy: f64) -> Change {
        Change {
            offset: (dx, dy),
            ..Change::default()
        }
    }

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
    }

    #[test]
    fn efface_un_texte_sans_deplacer_la_suite() {
        // Deux blocs dans le même tableau TJ, séparés par un grand décalage.
        let before = page(b"BT /F1 10 Tf 100 700 Td [(gauche) -20000 (droite)] TJ ET");
        assert_eq!(before.objects.len(), 2);
        let after = before.edit(&[(0, deleted())]);
        let lines = after.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, "droite");
        assert!(close(lines[0].1, before.lines()[1].1));
    }

    #[test]
    fn efface_avec_apostrophe_et_guillemet() {
        let before =
            page(b"BT /F1 10 Tf 100 TL 100 700 Td (un) Tj (deux) ' 2 1 (trois) \" (quatre) ' ET");
        assert_eq!(before.objects.len(), 4);
        let after = before.edit(&[(1, deleted()), (2, deleted())]);
        let lines = after.lines();
        let texts: Vec<&str> = lines.iter().map(|l| l.0.as_str()).collect();
        assert_eq!(texts, ["un", "quatre"]);
        assert!(close(lines[1].1, before.lines()[3].1));
        // `"` a fixé l'espacement des caractères pour la suite.
        let last = after.run.glyphs.last().unwrap();
        assert_eq!(after.run.states[last.state].char_spacing, 1.0);
    }

    #[test]
    fn deplace_un_texte_sous_une_matrice() {
        let before = page(
            b"q 2 0 0 2 0 0 cm BT 1 0 0 rg /F1 5 Tf 50 350 Td (bouge) Tj ET Q \
              BT /F1 10 Tf 100 100 Td (reste) Tj ET",
        );
        let after = before.edit(&[(0, moved(10.0, -20.0))]);
        let old = before.lines();
        let new = after.lines();
        assert_eq!(new.len(), 2);
        let bouge = new.iter().find(|l| l.0 == "bouge").unwrap();
        assert!(close(
            bouge.1,
            Point::new(old[0].1.x + 10.0, old[0].1.y - 20.0)
        ));
        let reste = new.iter().find(|l| l.0 == "reste").unwrap();
        assert!(close(reste.1, old[1].1));
        // La couleur et le corps sont rétablis.
        let glyph = after.run.glyphs.iter().find(|g| g.text == "b").unwrap();
        let state = &after.run.states[glyph.state];
        assert_eq!(state.fill.color.as_deref(), Some(&b"1 0 0 rg"[..]));
        assert_eq!(state.font_size, 5.0);
        assert!((glyph.trm.vertical_scale() - 10.0).abs() < 1e-9);
    }

    struct Ascii;

    impl Shaper for Ascii {
        fn glyph(&self, c: char) -> Option<(Vec<u8>, f64)> {
            c.is_ascii().then(|| (vec![c as u8], 500.0))
        }
        fn ascent(&self) -> f64 {
            800.0
        }
        fn descent(&self) -> f64 {
            -200.0
        }
    }

    #[test]
    fn remplace_un_texte() {
        let before = page(
            b"BT /F1 10 Tf 12 TL 100 700 Td (aaaa bbbb cc) Tj T* (dddd eeee ff) Tj ET \
              BT /F1 10 Tf 100 600 Td (fin) Tj ET",
        );
        let block = before.objects[0].text.as_ref().unwrap();
        let laid = layout(&before.run, block, "xx yy zzzz", &Ascii);
        let change = Change {
            text: Some(NewText {
                layout: laid,
                font: b"F1".to_vec(),
            }),
            ..Change::default()
        };
        let after = before.edit(&[(0, change)]);
        let lines = after.lines();
        let texts: Vec<&str> = lines.iter().map(|l| l.0.as_str()).collect();
        // Le nouveau texte suit son objet texte d'origine, avant « fin ».
        assert_eq!(texts, ["xx yy zzzz", "fin"]);
        assert!(close(lines[0].1, Point::new(100.0, 700.0)));
    }

    #[test]
    fn deplace_et_supprime_des_traces_et_images() {
        let before = page(
            b"q 1 0 0 RG 10 10 m 50 50 l S Q q 100 0 0 50 200 300 cm /Im1 Do Q \
              BT /F1 10 Tf 100 700 Td (texte) Tj ET",
        );
        assert_eq!(before.objects.len(), 3);
        let after = before.edit(&[(0, moved(5.0, 5.0)), (1, deleted())]);
        assert_eq!(after.run.units.len(), 1);
        let path = &after.run.units[0];
        let expected = before.run.units[0].bbox.offset(5.0, 5.0);
        assert!(
            (path.bbox.x0 - expected.x0).abs() < 1e-6 && (path.bbox.y1 - expected.y1).abs() < 1e-6
        );
        assert_eq!(after.lines()[0].0, "texte");
    }

    #[test]
    fn un_trace_qui_detoure_garde_son_detourage() {
        let before = page(b"0 0 100 100 re W f q 300 0 0 300 0 0 cm /Im1 Do Q");
        let image_bbox = before.run.units[1].bbox;
        let after = before.edit(&[(0, deleted())]);
        assert_eq!(after.run.units.len(), 1);
        assert_eq!(after.run.units[0].bbox, image_bbox);
        assert!(String::from_utf8_lossy(&after.source).contains("re W\nn"));
    }
}
