//! Le titre d'une réunion nomme son dossier et ses fichiers : quand il change (bilan de l'agent,
//! ou à la main dans l'app), tout est renommé, liens internes compris.

use crate::{Evenement, Greffier, IdReunion};

impl Greffier {
    /// Renomme le dossier de la réunion d'après son titre, puis réécrit ses fichiers. Un échec
    /// laisse le dossier tel quel (et l'annonce) : le titre est quand même gardé dans la fiche.
    pub(crate) fn renommer_d_apres_le_titre(&mut self, id: IdReunion, ev: &mut Vec<Evenement>) {
        let Some(r) = self.reunion(id) else { return };
        let ancien = r.nom_du_dossier.clone();
        match self.dossier.renommer(&ancien, r.debut, &r.titre) {
            Ok(nouveau) if nouveau != ancien => {
                // Les empreintes suivent le dossier : sinon il passerait pour disparu, et le
                // nouveau pour une réunion apparue hors de l'app.
                self.empreintes.remove(&ancien);
                let bilan = self.dossier.lire_le_bilan(&nouveau).ok();
                if let Ok(r) = self.reunion_mut(id) {
                    r.nom_du_dossier = nouveau;
                    if let Some(md) = bilan.filter(|md| *md != r.bilan) {
                        r.poser_bilan(md);
                    }
                }
            }
            Ok(_) => {}
            Err(message) => ev.push(Evenement::EcritureImpossible { reunion: id, message: format!("Renommage impossible : {message}") }),
        }
        self.persister(id, ev);
    }
}
