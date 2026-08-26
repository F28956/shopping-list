//! Telling other people's screens that a list is no longer what they last read.
//!
//! This lives in the service layer rather than in a transport because the service
//! layer is where every mutation already funnels. A transport that announced its own
//! changes would only announce the ones made through it, and the interesting case is
//! exactly the other one: a list edited in the browser while a phone is looking at
//! it.

use tokio::sync::broadcast;

use crate::models::{list, user};

/// A nudge: this list is not what you last read.
///
/// Deliberately not the change itself. Sending the new rows would make every watcher
/// a second source of truth for order and content, and the two would drift the first
/// time an event was dropped. A watcher that is told "something moved" and re-reads
/// cannot drift, because it never had its own opinion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    pub list_id: list::Id,
}

/// A nudge for one person: the set of lists they can see is not what they last read.
///
/// Separate from [`Changed`] because it is a different question. A list's watchers are
/// whoever has it open; the watchers of *which lists exist* are people, and a list
/// that has just been made has no watchers at all — which is exactly why making one
/// went unnoticed everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListsChanged {
    pub user_id: user::Id,
}

/// Who to tell when a list changes.
///
/// Cloning shares the channel, which is what makes one of these in `main` reach both
/// transports. Two separately constructed `Changes` are two separate worlds, and a
/// browser edit would never reach a phone.
#[derive(Debug, Clone)]
pub struct Changes {
    lists: broadcast::Sender<Changed>,
    people: broadcast::Sender<ListsChanged>,
}

impl Changes {
    pub fn new() -> Self {
        // Room for a burst while a watcher is between polls. A watcher that falls
        // further behind than this is told it lagged rather than fed stale nudges,
        // and since a nudge only says "re-read", missing some of them costs nothing.
        Self {
            lists: broadcast::channel(64).0,
            people: broadcast::channel(64).0,
        }
    }

    /// Says a list changed. Silent when nobody is watching, which is the normal case.
    pub fn announce(&self, list_id: list::Id) {
        // `send` fails only when there are no receivers, which is not a problem: an
        // unwatched list still changed, there is simply no one to tell.
        let _ = self.lists.send(Changed { list_id });
    }

    /// Says the set of lists this person can see has changed — one made, renamed,
    /// deleted, joined or left.
    pub fn announce_lists_of(&self, user_id: user::Id) {
        let _ = self.people.send(ListsChanged { user_id });
    }

    /// Starts watching one list. Only nudges sent after this call arrive.
    pub fn watch(&self) -> broadcast::Receiver<Changed> {
        self.lists.subscribe()
    }

    /// Starts watching what lists a person can see.
    pub fn watch_lists(&self) -> broadcast::Receiver<ListsChanged> {
        self.people.subscribe()
    }
}

impl Default for Changes {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(id: i64) -> list::Id {
        list::Id(id)
    }

    #[tokio::test]
    async fn a_watcher_hears_a_change() {
        let changes = Changes::new();
        let mut watching = changes.watch();

        changes.announce(list(7));

        assert_eq!(watching.recv().await.unwrap(), Changed { list_id: list(7) });
    }

    /// The reason `Changes` is cloned rather than constructed twice: the API and the
    /// browser transport each hold one, and an edit in either has to reach the other.
    #[tokio::test]
    async fn a_clone_is_the_same_channel() {
        let changes = Changes::new();
        let mut watching = changes.watch();

        changes.clone().announce(list(7));

        assert_eq!(watching.recv().await.unwrap().list_id, list(7));
    }

    #[tokio::test]
    async fn every_watcher_hears_it() {
        let changes = Changes::new();
        let mut one = changes.watch();
        let mut two = changes.watch();

        changes.announce(list(3));

        assert_eq!(one.recv().await.unwrap().list_id, list(3));
        assert_eq!(two.recv().await.unwrap().list_id, list(3));
    }

    /// Announcing into an empty room is not an error. Most lists are unwatched most
    /// of the time, and a service call must not fail because of that.
    #[tokio::test]
    async fn announcing_to_nobody_is_fine() {
        Changes::new().announce(list(1));
    }

    /// A watcher only hears what happened after it started watching -- which is why
    /// a client re-reads on connect rather than trusting the stream for history.
    #[tokio::test]
    async fn watching_is_not_retrospective() {
        let changes = Changes::new();
        changes.announce(list(1));
        let mut watching = changes.watch();
        changes.announce(list(2));

        assert_eq!(watching.recv().await.unwrap().list_id, list(2));
    }
}
