//! Les fichiers font foi : le cœur relit ce qui a changé dans le dossier de réunions hors de
//! l'app (un bilan retouché dans un éditeur, une transcription corrigée par un agent, un dossier
//! supprimé ou ajouté dans le Finder).
//!
//! Le cœur retient l'empreinte de chaque dossier de réunion ([`Empreintes`]) et la met à jour à
//! chacune de ses propres écritures ([`Greffier::noter`]) : relire le dossier juste après une
//! écriture de Greffier ne recharge rien, il n'y a pas de boucle.

use crate::folder::watch::Fingerprints;
use crate::{Event, Greffier, Role, Status};

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
    /// - Un dossier connu qui n'est la réunion de personne (illisible au dernier passage) est
    ///   laissé de côté.
    pub fn rescan_folder(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        let names = self.folder.names();
        let gone: Vec<String> = self.fingerprints.keys().filter(|name| !names.contains(name)).cloned().collect();
        for name in gone {
            self.fingerprints.remove(&name);
            if let Some(id) = self.id_for_folder(&name) {
                self.remove_from_queue(id);
                self.meetings.retain(|r| r.id != id);
                ev.push(Event::MeetingDisappeared(id));
            }
        }
        let recorded = self.recording.as_ref().map(|e| e.folder_name.clone());
        let processed = self.processing.as_ref().map(|t| t.meeting);
        for name in names {
            if recorded.as_ref() == Some(&name) {
                continue;
            }
            let fingerprints = self.folder.fingerprints(&name);
            let known = match self.fingerprints.get(&name) {
                Some(e) if *e == fingerprints => continue,
                Some(_) => true,
                None => false,
            };
            let id = self.id_for_folder(&name);
            if id.is_some() && id == processed {
                continue;
            }
            match id {
                Some(id) => {
                    if let Some(reread) = self.folder.read(&name, id) {
                        self.replace(reread);
                        ev.push(Event::MeetingReviewed(id));
                    }
                }
                // Une réunion supprimée dans l'app est partie à la corbeille et sortie des
                // empreintes : un dossier connu sans réunion est un dossier illisible.
                None if known => {}
                None => {
                    if let Some(r) = self.folder.read(&name, self.next_id) {
                        self.next_id += 1;
                        ev.push(Event::MeetingAppeared(r.id));
                        self.add(r);
                    }
                }
            }
            self.fingerprints.insert(name, fingerprints);
        }
        self.indexer();
        ev
    }

    /// Note l'empreinte des fichiers que Greffier vient d'écrire dans le dossier `nom` (tout le
    /// dossier s'il ne le connaissait pas encore, comme celui d'un enregistrement qui s'arrête).
    pub(crate) fn note_end(&mut self, name: &str, roles: &[Role]) {
        let current: Fingerprints = self.folder.fingerprints(name);
        let known = self.fingerprints.entry(name.to_string()).or_insert_with(|| current.clone());
        for role in roles {
            match current.get(role) {
                Some(e) => _ = known.insert(*role, *e),
                None => _ = known.remove(role),
            }
        }
    }

    fn id_for_folder(&self, name: &str) -> Option<crate::MeetingId> {
        self.meetings.iter().find(|r| r.folder_name == name).map(|r| r.id)
    }

    /// Remplace une réunion par sa version relue. Dans la file, elle garde son statut.
    fn replace(&mut self, mut reread: crate::Meeting) {
        let Some(i) = self.meetings.iter().position(|r| r.id == reread.id) else { return };
        if self.meetings[i].status.in_queue() {
            reread.status = self.meetings[i].status.clone();
            self.meetings[i] = reread;
        } else {
            let id = reread.id;
            self.meetings[i] = reread;
            self.enqueue_if_pending(id);
        }
    }

    /// Ajoute une réunion apparue, à sa place (les plus récentes d'abord).
    fn add(&mut self, r: crate::Meeting) {
        let i = self.meetings.iter().position(|x| x.start < r.start).unwrap_or(self.meetings.len());
        let id = r.id;
        self.meetings.insert(i, r);
        self.enqueue_if_pending(id);
    }

    /// Une fiche en attente ou en cours (écrite hors de l'app) remet la réunion dans la file,
    /// comme au lancement.
    fn enqueue_if_pending(&mut self, id: crate::MeetingId) {
        if let Some(Status::Pending(stage) | Status::Running { stage, .. }) = self.meeting(id).map(|r| r.status.clone()) {
            self.file.retain(|x| *x != id);
            self.enqueue(id, stage);
        }
    }
}
