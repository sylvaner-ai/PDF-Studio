//! Qualité de service des fils.
//!
//! Le fil d'un document répond à des fils de priorités diverses : interface,
//! rendu, recherche. Un fil prioritaire qui attend un fil moins prioritaire subit
//! une inversion de priorité, que macOS signale. Le fil du document adopte donc,
//! pour chaque requête, la qualité de service du fil qui attend la réponse.
//!
//! Les appels système nécessaires relèvent du code `unsafe` ; ils sont fournis par
//! la couche FFI au moyen de [`install_qos_hooks`]. Sans eux, ce module ne fait
//! rien.

use std::sync::OnceLock;

/// Fonctions qui lisent et règlent la qualité de service du fil courant.
#[derive(Clone, Copy, Debug)]
pub struct QosHooks {
    /// Classe de qualité de service du fil courant (`qos_class_t` sur macOS) ;
    /// 0 signifie « non précisée ».
    pub current: fn() -> u32,
    /// Applique une classe au fil courant ; renvoie `false` si le système refuse.
    pub apply: fn(u32) -> bool,
}

static HOOKS: OnceLock<QosHooks> = OnceLock::new();

/// Installe les fonctions de qualité de service. Seul le premier appel compte.
pub fn install_qos_hooks(hooks: QosHooks) {
    let _ = HOOKS.set(hooks);
}

/// Qualité de service d'un fil ; 0 signifie « non précisée ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Qos(u32);

impl Qos {
    /// Qualité de service du fil courant.
    pub(crate) fn current() -> Self {
        Self(HOOKS.get().map_or(0, |hooks| (hooks.current)()))
    }

    /// Applique cette qualité de service au fil courant ; renvoie `false` si elle
    /// n'est pas précisée ou n'a pas pu être appliquée.
    pub(crate) fn apply(self) -> bool {
        self.0 != 0 && HOOKS.get().is_some_and(|hooks| (hooks.apply)(self.0))
    }
}
