//! Mode Amélioration : transformer une page scannée en une page proche d'une
//! page native (cf. `docs/AMELIORATION.md`).
//!
//! Ce crate travaille sur les pixels du scan, sans MuPDF ni format PDF :
//!
//! - [`Settings`] : réglages de chaque étape, sérialisables (JSON) ;
//! - [`straighten`] : orientation (quarts de tour) et inclinaison d'une page,
//!   avec les garde-fous du redressement ;
//! - [`rotate`] : rotation de qualité (bicubique) de l'image, bords remplis de
//!   la couleur du papier, estimée par blocs ([`Paper`]) ;
//! - [`encode`] : image améliorée encodée en JPEG, pour être posée dans la
//!   page à la place du scan.
//!
//! Les étapes suivantes (couleurs, enchaînement avec l'OCR) s'y ajouteront.
#![forbid(unsafe_code)]

pub mod encode;
mod paper;
pub mod rotate;
mod settings;
pub mod straighten;

pub use encode::{encode, EncodeError, Encoded};
pub use paper::{paper_color, Paper};
pub use rotate::rotate;
pub use settings::{Settings, SettingsError, StraightenSettings};
pub use straighten::{analyze, Orientation, Straightening, Verdict};
