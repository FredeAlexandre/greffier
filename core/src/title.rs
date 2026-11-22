//! Le titre d'une réunion nomme son dossier et ses fichiers : quand il change (bilan de l'agent,
//! ou à la main dans l'app), tout est renommé, liens internes compris.

use crate::{Event, Greffier, MeetingId};

impl Greffier {
    /// Renomme le dossier de la réunion d'après son titre, puis réécrit ses fichiers. Un échec
    /// laisse le dossier tel quel (et l'annonce) : le titre est quand même gardé dans la fiche.
    pub(crate) fn rename_from_title(&mut self, id: MeetingId, ev: &mut Vec<Event>) {
        let Some(r) = self.meeting(id) else { return };
        let old = r.folder_name.clone();
        match self.folder.rename(&old, r.start, &r.title) {
            Ok(new) if new != old => {
                // Les empreintes suivent le dossier : sinon il passerait pour disparu, et le
                // nouveau pour une réunion apparue hors de l'app.
                self.fingerprints.remove(&old);
                let summary = self.folder.read_summary(&new).ok();
                if let Ok(r) = self.meeting_mut(id) {
                    r.folder_name = new;
                    if let Some(md) = summary.filter(|md| *md != r.summary) {
                        r.set_summary(md);
                    }
                }
            }
            Ok(_) => {}
            Err(message) => ev.push(Event::WriteFailed { meeting: id, message: format!("Renommage impossible : {message}") }),
        }
        self.persist(id, ev);
    }
}
