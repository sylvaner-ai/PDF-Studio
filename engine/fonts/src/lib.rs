//! Polices : catalogue des polices disponibles, mise en forme et rendu d'un
//! texte, appariement d'une police au texte d'une page scannée.
//!
//! Le catalogue réunit les polices fournies avec l'application (familles
//! libres sous licence OFL) et une liste de familles courantes des documents,
//! cherchées parmi les polices installées sur le Mac. Pour retrouver la
//! police d'une ligne scannée, chaque candidate est rendue à la taille et à
//! l'interlettrage mesurés sur le scan, puis comparée pixel à pixel à l'encre
//! de la ligne ([`matching`]). Le rendu ([`text`]) sert aussi à dessiner le
//! nouveau texte d'une retouche, à la résolution du scan.
//!
//! Ce crate ne dépend ni de MuPDF ni du format PDF.
#![forbid(unsafe_code)]

pub mod catalog;
pub mod matching;
pub mod text;

pub use catalog::{Catalog, FaceInfo, FontFile};
pub use matching::{match_line, Candidate, LineMatch, Observation, ObservedWord};
pub use text::{rasterize, shape, Coverage, Pen, Shaped};
