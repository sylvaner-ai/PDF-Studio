//! Historique d'annulation et de rétablissement, selon le patron *Command*.
//!
//! Chaque modification est un objet qui sait s'appliquer et se défaire.
//! L'historique est illimité sur la durée de la session ; il retient aussi l'état
//! du dernier enregistrement pour savoir si le document a changé depuis.

/// Modification réversible d'une cible `T`.
pub trait Command<T>: Send {
    /// Applique la modification, la première fois comme lors d'un rétablissement.
    fn apply(&mut self, target: &mut T);
    /// Défait la modification ; `target` est dans l'état laissé par [`Command::apply`].
    fn revert(&mut self, target: &mut T);
    /// Nom de l'action pour les menus, avec son article (« la rotation »).
    fn name(&self) -> &str;
}

/// Historique des modifications d'une cible `T`.
pub struct History<T> {
    done: Vec<Box<dyn Command<T>>>,
    undone: Vec<Box<dyn Command<T>>>,
    /// Nombre de modifications appliquées lors du dernier enregistrement ;
    /// `None` quand cet état ne peut plus être retrouvé.
    saved: Option<usize>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            saved: Some(0),
        }
    }
}

impl<T> History<T> {
    /// Historique vide ; l'état initial est considéré comme enregistré.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applique une modification et la retient. Les actions annulées ne peuvent
    /// plus être rétablies ensuite.
    pub fn execute(&mut self, target: &mut T, mut command: Box<dyn Command<T>>) {
        if self.saved.is_some_and(|saved| saved > self.done.len()) {
            // L'état enregistré se trouvait parmi les actions annulées, qui disparaissent.
            self.saved = None;
        }
        command.apply(target);
        self.done.push(command);
        self.undone.clear();
    }

    /// Défait la dernière modification ; renvoie `false` s'il n'y en a pas.
    pub fn undo(&mut self, target: &mut T) -> bool {
        let Some(mut command) = self.done.pop() else {
            return false;
        };
        command.revert(target);
        self.undone.push(command);
        true
    }

    /// Rétablit la dernière modification annulée ; renvoie `false` s'il n'y en a pas.
    pub fn redo(&mut self, target: &mut T) -> bool {
        let Some(mut command) = self.undone.pop() else {
            return false;
        };
        command.apply(target);
        self.done.push(command);
        true
    }

    /// Nom de l'action que [`History::undo`] déferait.
    pub fn undo_name(&self) -> Option<&str> {
        self.done.last().map(|command| command.name())
    }

    /// Nom de l'action que [`History::redo`] rétablirait.
    pub fn redo_name(&self) -> Option<&str> {
        self.undone.last().map(|command| command.name())
    }

    /// Vrai si l'état courant diffère du dernier état enregistré.
    pub fn is_modified(&self) -> bool {
        self.saved != Some(self.done.len())
    }

    /// Retient l'état courant comme enregistré.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.done.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Add(i32);

    impl Command<i32> for Add {
        fn apply(&mut self, target: &mut i32) {
            *target += self.0;
        }
        fn revert(&mut self, target: &mut i32) {
            *target -= self.0;
        }
        fn name(&self) -> &str {
            "l'ajout"
        }
    }

    #[test]
    fn annule_et_retablit() {
        let (mut value, mut history) = (0, History::new());
        history.execute(&mut value, Box::new(Add(2)));
        history.execute(&mut value, Box::new(Add(3)));
        assert_eq!(value, 5);
        assert!(history.undo(&mut value));
        assert_eq!(value, 2);
        assert_eq!(history.redo_name(), Some("l'ajout"));
        assert!(history.redo(&mut value));
        assert_eq!(value, 5);
        assert!(!history.redo(&mut value));
        assert!(history.undo(&mut value) && history.undo(&mut value));
        assert!(!history.undo(&mut value));
        assert_eq!(value, 0);
    }

    #[test]
    fn une_nouvelle_action_efface_les_actions_annulees() {
        let (mut value, mut history) = (0, History::new());
        history.execute(&mut value, Box::new(Add(2)));
        history.undo(&mut value);
        history.execute(&mut value, Box::new(Add(7)));
        assert!(!history.redo(&mut value));
        assert_eq!(value, 7);
    }

    #[test]
    fn suit_l_etat_enregistre() {
        let (mut value, mut history) = (0, History::new());
        assert!(!history.is_modified());
        history.execute(&mut value, Box::new(Add(1)));
        assert!(history.is_modified());
        history.mark_saved();
        assert!(!history.is_modified());
        history.undo(&mut value);
        assert!(history.is_modified());
        history.redo(&mut value);
        assert!(!history.is_modified());

        // Annuler puis modifier autrement rend l'état enregistré inaccessible.
        history.undo(&mut value);
        history.execute(&mut value, Box::new(Add(5)));
        history.undo(&mut value);
        assert!(history.is_modified());
    }
}
