//! Structure masquée du texte d'un PDF, pour diagnostiquer le découpage en
//! blocs sur un document privé sans en dévoiler le contenu.
//!
//! Usage : `cargo run --release -p pdf_core --example structure -- fichier.pdf [page]`

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage : structure fichier.pdf [page, à partir de 0]");
        std::process::exit(2);
    };
    let page: u32 = args.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let bytes = std::fs::read(&path).expect("lecture du fichier");
    let mut document = pdf_core::Document::from_bytes(&bytes, None).expect("ouverture du PDF");
    print!(
        "{}",
        document.text_structure(page).expect("analyse de la page")
    );
}
