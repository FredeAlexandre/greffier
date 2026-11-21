//! Supprimer une réunion : son dossier entier part à la corbeille du Mac (adaptateur
//! [`Corbeille`](crate::adaptateurs::Corbeille)), pour qu'on puisse s'en rattraper.

use crate::{Greffier, IdReunion, Refus};

impl Greffier {
    /// Arrête le traitement de la réunion (l'adaptateur écrit dans son dossier), met son dossier
    /// à la corbeille et l'oublie : elle quitte la liste, l'index de recherche et les empreintes
    /// (la surveillance ne la voit pas disparaître). Un dossier déjà parti (supprimé dans le
    /// Finder avant que l'app le voie) n'a rien à jeter.
    ///
    /// Si la corbeille refuse, la réunion reste, avec son dossier ; son traitement, lui, est
    /// arrêté (elle redevient à traiter).
    pub(crate) fn supprimer(&mut self, id: IdReunion) -> Result<(), Refus> {
        let nom = self.reunion(id).ok_or(Refus::ReunionInconnue(id))?.nom_du_dossier.clone();
        if self.reunion(id).is_some_and(|r| r.statut.dans_la_file()) {
            self.sortir_de_la_file(id);
            self.reunion_mut(id)?.statut = crate::Statut::ATraiter;
        }
        let dossier = self.dossier.dossier(&nom);
        if dossier.exists() {
            self.adaptateurs.corbeille.jeter(&dossier).map_err(|e| Refus::Impossible(format!("« {nom} » n'a pas pu aller à la corbeille : {e}")))?;
        }
        self.reunions.retain(|r| r.id != id);
        self.empreintes.remove(&nom);
        Ok(())
    }
}
