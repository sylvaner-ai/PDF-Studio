import AppKit
import SwiftUI

/// Règles visuelles communes de l'interface (cahier des charges, §5 ; D43).
///
/// - Espacements : multiples de 4 pt.
/// - Coins : arrondis continus, un rayon pour les panneaux, un pour les petits
///   cadres ; les bandeaux sont des capsules.
/// - Texte : SF Pro en trois tailles seulement, `.title3` pour les écrans vides,
///   `.body` par défaut et `.caption` pour les précisions.
/// - Couleur : celle d'accentuation du système, réservée à ce qui est actif ou
///   sélectionné ; le reste en gris neutres.
/// - Animations : 150 à 250 ms, ressorts sans rebond, un simple fondu si
///   « Réduire les animations » est activé.
enum Design {
    /// Grille d'espacement.
    enum Spacing {
        static let tight: CGFloat = 4
        static let small: CGFloat = 8
        static let medium: CGFloat = 12
        static let large: CGFloat = 16
        static let wide: CGFloat = 32
    }

    /// Rayons des coins.
    enum Radius {
        /// Petits cadres : sélection d'une vignette.
        static let small: CGFloat = 4
        /// Panneaux posés sur les pages : recherche, retouche, bandeau d'édition.
        static let panel: CGFloat = 12
    }

    /// Animations de l'interface.
    enum Motion {
        /// Apparition d'un panneau ou d'un bandeau, changement de présentation.
        static let spring = Animation.spring(duration: 0.22, bounce: 0)
        /// Remplace les mouvements si « Réduire les animations » est activé.
        static let fade = Animation.easeInOut(duration: 0.15)
        /// Durée des animations AppKit (défilement, fondu d'une page rendue).
        static let duration: TimeInterval = 0.2

        static func animation(reduceMotion: Bool) -> Animation {
            reduceMotion ? fade : spring
        }

        /// Vrai si les mouvements doivent être évités (réglage d'accessibilité).
        @MainActor
        static var reducesMotion: Bool {
            NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        }
    }

    /// Ombre des pages, plus marquée sur le fond sombre du mode sombre, où une
    /// ombre légère disparaît.
    static func pageShadowOpacity(dark: Bool) -> Float {
        dark ? 0.5 : 0.14
    }
}

extension NSAppearance {
    /// Vrai pour les apparences sombres (mode sombre, contraste élevé compris).
    var isDark: Bool {
        bestMatch(from: [.aqua, .darkAqua, .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua])
            .map { $0 == .darkAqua || $0 == .accessibilityHighContrastDarkAqua } ?? false
    }
}

// MARK: - Surfaces posées sur les pages

/// Fond translucide d'un panneau ou d'un bandeau : matériau, filet fin et
/// ombre douce ; le filet s'épaissit si « Augmenter le contraste » est activé.
struct FloatingSurface<S: InsettableShape>: ViewModifier {
    let shape: S
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        let increased = contrast == .increased
        content
            .background(.regularMaterial, in: shape)
            .overlay(shape.strokeBorder(.separator, lineWidth: increased ? 1 : 0.5))
            .shadow(color: .black.opacity(0.12), radius: 8, y: 2)
    }
}

/// Apparition et disparition d'un panneau : fondu accompagné d'un léger
/// glissement depuis son bord, ou fondu seul si les animations sont réduites.
struct FloatingTransition: ViewModifier {
    let edge: VerticalEdge
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        let offset = edge == .bottom ? Design.Spacing.small : -Design.Spacing.small
        content.transition(reduceMotion ? .opacity : .opacity.combined(with: .offset(y: offset)))
    }
}

/// Anime les changements de `value` avec l'animation de l'interface.
struct InterfaceAnimation<Value: Equatable>: ViewModifier {
    let value: Value
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.animation(Design.Motion.animation(reduceMotion: reduceMotion), value: value)
    }
}

extension View {
    /// Panneau posé sur les pages, aux coins arrondis.
    func floatingPanel() -> some View {
        modifier(FloatingSurface(shape: RoundedRectangle(cornerRadius: Design.Radius.panel, style: .continuous)))
    }

    /// Bandeau en capsule, au bas de la fenêtre.
    func floatingBadge() -> some View {
        modifier(FloatingSurface(shape: Capsule(style: .continuous)))
    }

    /// Transition d'un panneau qui apparaît depuis le bord `edge`.
    func floatingTransition(from edge: VerticalEdge) -> some View {
        modifier(FloatingTransition(edge: edge))
    }

    /// Anime ce qui apparaît ou disparaît quand `value` change.
    func interfaceAnimation(value: some Equatable) -> some View {
        modifier(InterfaceAnimation(value: value))
    }
}
