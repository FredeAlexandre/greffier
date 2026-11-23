//! Deleting a meeting: its whole folder goes to the Mac's trash (adapter
//! [`Trash`](crate::adapters::Trash)), so the deletion can be undone.

use crate::{Greffier, MeetingId, Reason, Rejection};

impl Greffier {
    /// Stops the meeting's processing (the adapter writes into its folder), moves its folder to
    /// the trash and forgets it: it leaves the list, the search index and the fingerprints (the
    /// watch does not see it disappear). A folder already gone (deleted in the Finder before the
    /// app saw it) has nothing to throw away.
    ///
    /// If the trash refuses, the meeting stays, with its folder; its processing, though, is
    /// stopped (it goes back to to-process).
    pub(crate) fn delete(&mut self, id: MeetingId) -> Result<(), Rejection> {
        let name = self.meeting(id).ok_or(Rejection::UnknownMeeting(id))?.folder_name.clone();
        if self.meeting(id).is_some_and(|r| r.status.in_queue()) {
            self.remove_from_queue(id);
            self.meeting_mut(id)?.status = crate::Status::ToProcess;
        }
        let folder = self.folder.folder(&name);
        if folder.exists() {
            self.adapters.trash.trash(&folder).map_err(|detail| Reason::Trash { folder_name: name.clone(), detail })?;
        }
        self.meetings.retain(|r| r.id != id);
        self.fingerprints.remove(&name);
        Ok(())
    }
}
