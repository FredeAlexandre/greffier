//! Supprimer une réunion : son dossier entier part à la corbeille du Mac (adaptateur
//! [`Corbeille`](crate::adaptateurs::Corbeille)), pour qu'on puisse s'en rattraper.

use crate::{Greffier, MeetingId, Rejection};

impl Greffier {
    /// Arrête le traitement de la réunion (l'adaptateur écrit dans son dossier), met son dossier
    /// à la corbeille et l'oublie : elle quitte la liste, l'index de recherche et les empreintes
    /// (la surveillance ne la voit pas disparaître). Un dossier déjà parti (supprimé dans le
    /// Finder avant que l'app le voie) n'a rien à jeter.
    ///
    /// Si la corbeille refuse, la réunion reste, avec son dossier ; son traitement, lui, est
    /// arrêté (elle redevient à traiter).
    pub(crate) fn delete(&mut self, id: MeetingId) -> Result<(), Rejection> {
        let name = self.meeting(id).ok_or(Rejection::UnknownMeeting(id))?.folder_name.clone();
        if self.meeting(id).is_some_and(|r| r.status.in_queue()) {
            self.remove_from_queue(id);
            self.meeting_mut(id)?.status = crate::Status::ToProcess;
        }
        let folder = self.folder.folder(&name);
        if folder.exists() {
            self.adapters.trash.trash(&folder).map_err(|e| Rejection::Impossible(format!("« {name} » n'a pas pu aller à la corbeille : {e}")))?;
        }
        self.meetings.retain(|r| r.id != id);
        self.fingerprints.remove(&name);
        Ok(())
    }
}
