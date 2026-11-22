//! A meeting's title names its folder and its files: when it changes (the agent's summary, or
//! by hand in the app), everything is renamed, internal links included.

use crate::{Event, Greffier, MeetingId};

impl Greffier {
    /// Renames the meeting's folder after its title, then rewrites its files. A failure leaves
    /// the folder as is (and reports it): the title is still kept in the sheet.
    pub(crate) fn rename_from_title(&mut self, id: MeetingId, ev: &mut Vec<Event>) {
        let Some(r) = self.meeting(id) else { return };
        let old = r.folder_name.clone();
        match self.folder.rename(&old, r.start, &r.title) {
            Ok(new) if new != old => {
                // The fingerprints follow the folder: otherwise it would look gone, and the new
                // one like a meeting that appeared outside the app.
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
