//! The files are authoritative: the core reads back what changed in the meetings folder outside
//! the app (a summary edited in an editor, a transcription corrected by an agent, a folder
//! deleted or added in the Finder).
//!
//! The core remembers the fingerprint of each meeting folder ([`Fingerprints`]) and updates it on
//! each of its own writes ([`Greffier::note_end`]): reading the folder right after a write by
//! Greffier reloads nothing, there is no loop.

use crate::folder::watch::Fingerprints;
use crate::{Event, Greffier, Role, Status};

impl Greffier {
    /// Compares the meetings folder with what the core knows of it and reads back what changed
    /// outside the app. The interface calls it when the system reports a change in the folder.
    ///
    /// - A file modified, added or deleted in a meeting's folder: the meeting is read back (its
    ///   status is kept if it is in the queue).
    /// - A new meeting folder: the meeting appears; a vanished folder: the meeting disappears
    ///   (and leaves the queue).
    /// - The folder of the recording in progress and that of the meeting being processed are
    ///   left aside: they will be read back later.
    /// - A known folder that is nobody's meeting (unreadable at the last pass) is left aside.
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
                // A meeting deleted in the app went to the trash and left the fingerprints: a
                // known folder without a meeting is an unreadable folder.
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

    /// Records the fingerprint of the files Greffier has just written in folder `name` (the whole
    /// folder if it did not know it yet, like that of a recording that stops).
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

    /// Replaces a meeting with its read-back version. In the queue, it keeps its status.
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

    /// Adds a meeting that appeared, in its place (most recent first).
    fn add(&mut self, r: crate::Meeting) {
        let i = self.meetings.iter().position(|x| x.start < r.start).unwrap_or(self.meetings.len());
        let id = r.id;
        self.meetings.insert(i, r);
        self.enqueue_if_pending(id);
    }

    /// A pending or running sheet (written outside the app) puts the meeting back in the queue,
    /// as at launch.
    fn enqueue_if_pending(&mut self, id: crate::MeetingId) {
        if let Some(Status::Pending(stage) | Status::Running { stage, .. }) = self.meeting(id).map(|r| r.status.clone()) {
            self.file.retain(|x| *x != id);
            self.enqueue(id, stage);
        }
    }
}
