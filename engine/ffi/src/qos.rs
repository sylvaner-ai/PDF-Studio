//! Qualité de service des fils sur macOS.
//!
//! Le fil de chaque document adopte la qualité de service du fil qui attend sa
//! réponse (cf. `pdf_core::QosHooks`), ce qui évite les inversions de priorité.
//! Les deux appels système nécessaires sont le seul code `unsafe` écrit à la main
//! dans le moteur.

/// Fournit au moteur les fonctions de qualité de service de la plateforme.
pub(crate) fn install() {
    #[cfg(target_vendor = "apple")]
    pdf_core::install_qos_hooks(pdf_core::QosHooks {
        current: apple::current,
        apply: apple::apply,
    });
}

#[cfg(target_vendor = "apple")]
mod apple {
    // Déclarées dans <pthread/qos.h> ; `qos_class_t` y est un `unsigned int`.
    extern "C" {
        fn qos_class_self() -> u32;
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }

    pub(super) fn current() -> u32 {
        // SAFETY : fonction de libSystem sans argument ni effet de bord.
        unsafe { qos_class_self() }
    }

    pub(super) fn apply(qos_class: u32) -> bool {
        // SAFETY : n'agit que sur le fil appelant ; une classe invalide est
        // refusée par le système (code d'erreur), sans comportement indéfini.
        unsafe { pthread_set_qos_class_self_np(qos_class, 0) == 0 }
    }
}
