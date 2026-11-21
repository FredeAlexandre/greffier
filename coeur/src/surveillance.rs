//! Les fichiers font foi : le cœur relit ce qui a changé dans le dossier de réunions hors de
//! l'app (un bilan retouché dans un éditeur, une transcription corrigée par un agent, un dossier
//! supprimé ou ajouté dans le Finder).
//!
//! Le cœur retient l'empreinte de chaque dossier de réunion ([`Empreintes`]) et la met à jour à
//! chacune de ses propres écritures ([`Greffier::noter`]) : relire le dossier juste après une
//! écriture de Greffier ne recharge rien, il n'y a pas de boucle.

use crate::dossier::surveillance::Empreintes;
use crate::{Evenement, Greffier, Role, Statut};

impl Greffier {
    /// Compare le dossier de réunions à ce que le cœur en connaît et relit ce qui a changé hors
    /// de l'app. L'interface l'appelle quand le système signale un changement dans le dossier.
    ///
    /// - Un fichier modifié, ajouté ou supprimé dans le dossier d'une réunion : elle est relue
    ///   (son statut est gardé si elle est dans la file).
    /// - Un dossier de réunion nouveau : la réunion apparaît ; un dossier disparu : elle
    ///   disparaît (et sort de la file).
    /// - Le dossier de l'enregistrement en cours et celui de la réunion en cours de traitement
    ///   sont laissés de côté : ils seront relus après.
    /// - Une réunion supprimée dans l'app dont le dossier existe encore ne revient pas.
    pub fn relire_le_dossier(&mut self) -> Vec<Evenement> {
        let mut ev = Vec::new();
        let noms = self.dossier.noms();
        let disparus: Vec<String> = self.empreintes.keys().filter(|nom| !noms.contains(nom)).cloned().collect();
        for nom in disparus {
            self.empreintes.remove(&nom);
            if let Some(id) = self.id_du_dossier(&nom) {
                self.sortir_de_la_file(id);
                self.reunions.retain(|r| r.id != id);
                ev.push(Evenement::ReunionDisparue(id));
            }
        }
        let enregistre = self.enregistrement.as_ref().map(|e| e.nom_du_dossier.clone());
        let traitee = self.traitement.as_ref().map(|t| t.reunion);
        for nom in noms {
            if enregistre.as_ref() == Some(&nom) {
                continue;
            }
            let empreintes = self.dossier.empreintes(&nom);
            let connu = match self.empreintes.get(&nom) {
                Some(e) if *e == empreintes => continue,
                Some(_) => true,
                None => false,
            };
            let id = self.id_du_dossier(&nom);
            if id.is_some() && id == traitee {
                continue;
            }
            match id {
                Some(id) => {
                    if let Some(relue) = self.dossier.lire(&nom, id) {
                        self.remplacer(relue);
                        ev.push(Evenement::ReunionRelue(id));
                    }
                }
                // Supprimée dans l'app : son dossier reste jusqu'à la corbeille (#15).
                None if connu => {}
                None => {
                    if let Some(r) = self.dossier.lire(&nom, self.prochain_id) {
                        self.prochain_id += 1;
                        ev.push(Evenement::ReunionApparue(r.id));
                        self.ajouter(r);
                    }
                }
            }
            self.empreintes.insert(nom, empreintes);
        }
        ev
    }

    /// Note l'empreinte des fichiers que Greffier vient d'écrire dans le dossier `nom` (tout le
    /// dossier s'il ne le connaissait pas encore, comme celui d'un enregistrement qui s'arrête).
    pub(crate) fn noter(&mut self, nom: &str, roles: &[Role]) {
        let actuelles: Empreintes = self.dossier.empreintes(nom);
        let connues = self.empreintes.entry(nom.to_string()).or_insert_with(|| actuelles.clone());
        for role in roles {
            match actuelles.get(role) {
                Some(e) => _ = connues.insert(*role, *e),
                None => _ = connues.remove(role),
            }
        }
    }

    fn id_du_dossier(&self, nom: &str) -> Option<crate::IdReunion> {
        self.reunions.iter().find(|r| r.nom_du_dossier == nom).map(|r| r.id)
    }

    /// Remplace une réunion par sa version relue. Dans la file, elle garde son statut.
    fn remplacer(&mut self, mut relue: crate::Reunion) {
        let Some(i) = self.reunions.iter().position(|r| r.id == relue.id) else { return };
        if self.reunions[i].statut.dans_la_file() {
            relue.statut = self.reunions[i].statut.clone();
            self.reunions[i] = relue;
        } else {
            let id = relue.id;
            self.reunions[i] = relue;
            self.mettre_en_file_si_en_attente(id);
        }
    }

    /// Ajoute une réunion apparue, à sa place (les plus récentes d'abord).
    fn ajouter(&mut self, r: crate::Reunion) {
        let i = self.reunions.iter().position(|x| x.debut < r.debut).unwrap_or(self.reunions.len());
        let id = r.id;
        self.reunions.insert(i, r);
        self.mettre_en_file_si_en_attente(id);
    }

    /// Une fiche en attente ou en cours (écrite hors de l'app) remet la réunion dans la file,
    /// comme au lancement.
    fn mettre_en_file_si_en_attente(&mut self, id: crate::IdReunion) {
        if let Some(Statut::EnAttente(etape) | Statut::EnCours { etape, .. }) = self.reunion(id).map(|r| r.statut.clone()) {
            self.file.retain(|x| *x != id);
            self.mettre_en_file(id, etape);
        }
    }
}
