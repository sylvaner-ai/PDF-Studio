//! Génère les liaisons Swift (sources, en-tête C, modulemap) à partir de la
//! bibliothèque compilée. Appelé par `make bindings`.

fn main() {
    uniffi::uniffi_bindgen_swift()
}
