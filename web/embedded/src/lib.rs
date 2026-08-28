//! The server, running on the device.
//!
//! A phone kept to itself is not an app with the server parts taken out. It is the
//! same app talking to a server that happens to be in the same process. That is the
//! whole idea, and it is worth saying why it is worth the trouble.
//!
//! ## What this replaces
//!
//! Standalone used to be *a server that fails every request*: `API.reachable` was
//! false, every call threw a transport error, and every screen went down an error path
//! and then had to be told the error was not real. `onDeviceOnly` reached fifty-seven
//! places across eighteen files. The queue filled for a reader that did not exist. The
//! empty state had to be taught that "I could not find out" and "there is nothing" are
//! different unless there is no server, in which case they are the same again.
//!
//! Every one of those is the cost of two modes pretending to be one. This makes them
//! actually one: the client asks the same questions and gets real answers, because
//! there is a real server -- it is simply not on the network.
//!
//! ## Why `domain` itself, rather than something like it
//!
//! Because "something like it" is a second implementation of the rules, and this
//! project has already paid for one of those twice: `pint milk` resolved differently on
//! the phone than on the server, and then the `bare` flag did. The lesson both times
//! was that sharing the *rules* is not enough if the two ends can drift -- so this
//! links the same crate, over the same schema, run by the same migrator.
//!
//! It compiles for `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-apple-darwin`
//! and both Android targets without a line of change to `domain`.
//!
//! ## Multi-user, with one user
//!
//! The device's database has `users`, `list_members` and roles, exactly as a server
//! does, and exactly one row in `users`. That is deliberate and it is not waste:
//!
//! * Every query in `domain` is written in terms of an actor and a role. A schema
//!   without those would need a second set of queries -- the thing this exists to
//!   avoid.
//! * Adopting a server stops being a migration and becomes a merge, because both sides
//!   already have the same shape.
//! * Sharing a list device-to-device, with no server anywhere, is then a question about
//!   transport rather than about data model. The second user already fits.

use std::path::Path;

use std::sync::Arc;

pub mod ffi;

use domain::models::item::{self, Item};
use domain::models::list::{self, List, Name};
use domain::models::user;
use domain::models::user::{Email, Name as UserName, Sub};
use domain::models::{Direction, OrderBy, Paging};
use domain::service::{Actor, Ctx, identity, lists};

/// How the one local person is identified.
///
/// A provider like any other, so `users` needs no column to say "this one is local".
/// The subject is fixed because there is exactly one of them: two installs are two
/// databases, and neither has ever heard of the other.
const LOCAL_PROVIDER: &str = "local";
const LOCAL_SUBJECT: &str = "this-device";

/// A shopping list database on this device, and the one person using it.
pub struct Local {
    ctx: Ctx,
    me: Actor,
    /// One current-thread runtime, because `domain` is async and something has to drive
    /// it. Held here rather than made per call: a runtime per call would be a thread
    /// pool per tap, and the connection pool inside `ctx` would be rebuilt around it.
    runtime: tokio::runtime::Runtime,
}

impl Local {
    /// Opens the database at `path`, migrating it, and finds or makes the local person.
    ///
    /// The migrations are `domain`'s own -- the same files the server runs. A device
    /// and a server that have both been brought up to date have the same schema, which
    /// is what lets one talk to the other about rows rather than about JSON.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::Opening(e.to_string()))?;

        let (ctx, me) = runtime.block_on(async {
            let url = format!("sqlite://{}?mode=rwc", path.display());
            let db = sqlx::SqlitePool::connect(&url)
                .await
                .map_err(|e| Error::Opening(e.to_string()))?;

            domain::MIGRATOR
                .run(&db)
                .await
                .map_err(|e| Error::Opening(e.to_string()))?;

            let ctx = Ctx::new(db);

            // A device admits its own owner. On a server this switch is the difference
            // between "anyone with a Google account" and an invited list; here there is
            // no network to be open to, and the alternative is an admission list of one
            // that exists only to let the one person in.
            domain::models::admission::Server::set_admits_anyone(&ctx.db, true)
                .await
                .map_err(|e| Error::Opening(e.to_string()))?;

            let me = identity::from_claims(
                &ctx,
                LOCAL_PROVIDER,
                Sub(LOCAL_SUBJECT.to_string()),
                Some(UserName("You".to_string())),
                // No address, because there is no account and nothing to send to one.
                // `admits_anyone` above is what makes this admissible.
                None::<Email>,
            )
            .await
            .map_err(|e| Error::Opening(e.to_string()))?;

            Ok::<_, Error>((ctx, me))
        })?;

        Ok(Self { ctx, me, runtime })
    }

    /// The lists this person can see.
    pub fn lists(&self) -> Result<Vec<List>, Error> {
        self.runtime.block_on(async {
            // Everything, in the order a list screen shows them. The page exists for a
            // server answering over a network; a device reading its own file has no
            // reason to withhold the second hundred.
            let page = Paging {
                number: 1,
                size: i64::MAX,
            };
            let order = OrderBy {
                field: list::Field::UpdatedAt,
                direction: Direction::Descending,
            };

            lists::for_user(&self.ctx, &self.me, page, order)
                .await
                .map(|listing| listing.items)
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Makes a list, as the local person.
    pub fn make_list(&self, name: &str) -> Result<List, Error> {
        self.runtime.block_on(async {
            lists::create(&self.ctx, &self.me, Name(name.to_string()))
                .await
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Renames one.
    pub fn rename_list(&self, id: i64, name: &str) -> Result<(), Error> {
        self.runtime.block_on(async {
            lists::update(&self.ctx, &self.me, list::Id(id), Name(name.to_string()))
                .await
                .map(|_| ())
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Deletes one.
    pub fn delete_list(&self, id: i64) -> Result<(), Error> {
        self.runtime.block_on(async {
            lists::delete(&self.ctx, &self.me, list::Id(id))
                .await
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Watches one list: what is on it, and what it is called.
    pub fn watch_list(&self, id: i64) -> Watcher {
        Watcher {
            want: Want::List(list::Id(id), self.ctx.changes.watch()),
            stop: Arc::new(tokio::sync::Notify::new()),
            stopped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            runtime: Self::watching_runtime(),
        }
    }

    /// Watches which lists this person can see.
    ///
    /// A separate question from the one above, and the reason is in `domain`: a list
    /// that has just been made has no watchers at all, so its own channel cannot carry
    /// the news that it exists.
    pub fn watch_lists(&self) -> Watcher {
        let who = match &self.me {
            Actor::User(user) => user.id,
            _ => user::Id(0),
        };
        Watcher {
            want: Want::Lists(who, self.ctx.changes.watch_lists()),
            stop: Arc::new(tokio::sync::Notify::new()),
            stopped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            runtime: Self::watching_runtime(),
        }
    }

    // MARK: what is on a list

    /// What is on one list, in the order the shop is walked.
    pub fn items(&self, list_id: i64) -> Result<Vec<Item>, Error> {
        self.runtime.block_on(async {
            let page = Paging {
                number: 1,
                size: i64::MAX,
            };
            let order = OrderBy {
                field: item::Field::CreatedAt,
                direction: Direction::Ascending,
            };

            domain::service::items::for_list(&self.ctx, &self.me, list::Id(list_id), page, order)
                .await
                .map(|listing| listing.items)
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Adds what somebody typed.
    ///
    /// The whole line, read by `items::quick_add` -- which is the server's own reading
    /// of it, against the server's own units and this list's own history. That is the
    /// thing this crate exists for: `pint milk` became one pint of milk on the server
    /// and `pint milk`, one unit, on the phone, because the two read the line
    /// separately. There is now one reader.
    ///
    /// `uuid` is the client's if it has one -- a row it has already drawn keeps the
    /// name it was drawn under.
    pub fn add(&self, list_id: i64, line: &str, uuid: Option<String>) -> Result<Item, Error> {
        self.runtime.block_on(async {
            domain::service::items::quick_add(
                &self.ctx,
                &self.me,
                list::Id(list_id),
                uuid.map(item::Uuid),
                line,
            )
            .await
            .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Crosses something off, or puts it back.
    pub fn set_done(&self, item_id: i64, done: bool) -> Result<Item, Error> {
        self.runtime.block_on(async {
            domain::service::items::set_done(&self.ctx, &self.me, item::Id(item_id), done)
                .await
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Corrects a row: what it is called, how much, and in what.
    pub fn update(
        &self,
        item_id: i64,
        name: &str,
        amount: f64,
        unit_id: Option<i64>,
    ) -> Result<Item, Error> {
        self.runtime.block_on(async {
            domain::service::items::update(
                &self.ctx,
                &self.me,
                item::Id(item_id),
                item::Name(name.to_string()),
                item::Amount(amount),
                unit_id.map(domain::models::unit::Id),
                // `seen` is how the server tells a plain rename from somebody
                // correcting a row two people had both been editing. A device with one
                // user has no such case, and passing what this device last drew would
                // be inventing evidence about a disagreement that cannot happen.
                None,
            )
            .await
            .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    pub fn delete_item(&self, item_id: i64) -> Result<(), Error> {
        self.runtime.block_on(async {
            domain::service::items::delete(&self.ctx, &self.me, item::Id(item_id))
                .await
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// Takes everything crossed off, off.
    pub fn clear_done(&self, list_id: i64) -> Result<u64, Error> {
        self.runtime.block_on(async {
            // All of them, not a chosen few: the screen's button is "clear done", and
            // the narrowing exists for a caller that has a selection.
            domain::service::items::clear_done(&self.ctx, &self.me, list::Id(list_id), None)
                .await
                .map_err(|e| Error::Refused(e.to_string()))
        })
    }

    /// A runtime for one watcher. No I/O and no timers, so no threads and no reactor.
    fn watching_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime that starts no threads")
    }

    /// This person's id. Exposed because the client stores rows keyed by owner, and a
    /// local row's owner is a real user id here rather than the zero it used to invent.
    pub fn me(&self) -> i64 {
        match &self.me {
            Actor::User(user) => user.id.0,
            _ => 0,
        }
    }
}

/// Something to re-read.
///
/// Deliberately not the change itself -- the same reasoning as `domain::service::changes`,
/// which see. A watcher told "something moved" and re-reading cannot drift; a watcher
/// sent the new rows becomes a second opinion about them, and the two disagree the
/// first time an event is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// What is on this list.
    List(i64),
    /// Which lists this person can see -- one made, renamed, deleted or joined.
    Lists,
}

/// What one turn of the wait produced.
enum Heard {
    Something(Change),
    /// A change, but not to what this watcher was asked about.
    SomethingElse,
    Stopped,
}

/// What one watcher is waiting for.
enum Want {
    List(
        list::Id,
        tokio::sync::broadcast::Receiver<domain::service::changes::Changed>,
    ),
    Lists(
        user::Id,
        tokio::sync::broadcast::Receiver<domain::service::changes::ListsChanged>,
    ),
}

/// Waits for the next thing worth re-reading.
///
/// Blocking, and meant to be called on a thread of the client's own. That is the shape
/// the boundary wants: a callback would mean Rust calling into Swift or attaching to a
/// JVM thread, and a poll would be the two-second timer this project has just finished
/// deleting.
///
/// **It does not use the database's runtime.** `blocking_recv` parks the calling thread
/// on a plain synchronisation primitive, so a watcher waiting all afternoon does not
/// hold anything the next `lists()` call needs. That was the first thing worth checking
/// and the first thing that would have gone wrong: `Local` drives sqlx on a
/// current-thread runtime, and a watcher that blocked *inside* it would have deadlocked
/// the app on its own first read.
pub struct Watcher {
    want: Want,
    stop: Arc<tokio::sync::Notify>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    /// The watcher's own runtime, and it has to be its own.
    ///
    /// Waiting is two things at once -- a change, or a stop -- and `select!` needs
    /// somewhere to run. It cannot be `Local`'s: that one is current-thread, and a
    /// watcher parked in it would hold it against the next read, which is the deadlock
    /// this whole shape exists to avoid.
    ///
    /// A current-thread runtime spawns no threads. It drives on whichever thread calls
    /// `next`, which is the client's watching thread and has nothing else to do.
    runtime: tokio::runtime::Runtime,
}

/// Ends a watch, from any thread.
///
/// Separate from `Watcher` because the two are used from different threads by
/// construction: the watcher is parked in `next`, so whoever ends it cannot be holding
/// it.
#[derive(Clone)]
pub struct Stopper {
    stop: Arc<tokio::sync::Notify>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl Stopper {
    pub fn stop(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // `notify_one` rather than `notify_waiters`, and the difference is the whole
        // correctness of stopping: `notify_waiters` wakes whoever is parked *now* and
        // is lost on a watcher that has not parked yet, which is a screen closing in
        // the instant between two changes. `notify_one` leaves a permit, so the next
        // wait returns immediately whichever order the two happen in.
        self.stop.notify_one();
    }
}

impl Watcher {
    /// A handle that ends this watch, for the thread that owns the screen.
    pub fn stopper(&self) -> Stopper {
        Stopper {
            stop: Arc::clone(&self.stop),
            stopped: Arc::clone(&self.stopped),
        }
    }

    /// Blocks until there is something to re-read, or until the watch is stopped.
    ///
    /// `None` means stopped and never means "nothing happened" -- a caller that treats
    /// it as the latter would spin.
    ///
    /// Named `wait` rather than `next` because it is not an iterator step: it parks the
    /// calling thread, which is the opposite of what `next` leads a reader to expect.
    pub fn wait(&mut self) -> Option<Change> {
        use tokio::sync::broadcast::error::RecvError;

        loop {
            if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
                return None;
            }

            let stop = Arc::clone(&self.stop);
            let want = &mut self.want;

            let heard = self.runtime.block_on(async move {
                match want {
                    Want::List(watching, receiver) => {
                        tokio::select! {
                            _ = stop.notified() => Heard::Stopped,
                            received = receiver.recv() => match received {
                                Ok(changed) if changed.list_id == *watching => {
                                    Heard::Something(Change::List(watching.0))
                                }
                                // Somebody else's list. Not news for this screen.
                                Ok(_) => Heard::SomethingElse,
                                // More changes than the channel holds. Which ones is
                                // not knowable and does not matter: the answer to "you
                                // have missed some" is the same as the answer to "one
                                // happened" -- re-read. Treating it as an error would
                                // stop a screen updating for the rest of its life over
                                // a burst of edits.
                                Err(RecvError::Lagged(_)) => {
                                    Heard::Something(Change::List(watching.0))
                                }
                                Err(RecvError::Closed) => Heard::Stopped,
                            },
                        }
                    }
                    Want::Lists(who, receiver) => {
                        tokio::select! {
                            _ = stop.notified() => Heard::Stopped,
                            received = receiver.recv() => match received {
                                Ok(changed) if changed.user_id == *who => {
                                    Heard::Something(Change::Lists)
                                }
                                Ok(_) => Heard::SomethingElse,
                                Err(RecvError::Lagged(_)) => Heard::Something(Change::Lists),
                                Err(RecvError::Closed) => Heard::Stopped,
                            },
                        }
                    }
                }
            });

            match heard {
                Heard::Something(change) => return Some(change),
                // A nudge about something else leaves us waiting rather than returning.
                // The caller asked for the next change *to this*, and a spurious wake
                // would put a re-read behind every edit anywhere in the app.
                Heard::SomethingElse => continue,
                Heard::Stopped => return None,
            }
        }
    }
}

/// What can go wrong, flattened for a boundary that carries no Rust types.
#[derive(Debug)]
pub enum Error {
    /// The database could not be opened or migrated. Not recoverable by retrying.
    Opening(String),
    /// The domain said no -- not found, not allowed, or invalid input. The message is
    /// the domain's own.
    Refused(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Opening(said) => write!(f, "{said}"),
            Error::Refused(said) => write!(f, "{said}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path in the OS temporary directory, unique per test.
    fn scratch() -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "embedded-{}-{:?}.sqlite",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_fresh_device_has_a_person_and_no_lists() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");

        assert!(local.me() > 0, "nobody was made for the device to act as");
        assert!(
            local.lists().unwrap().is_empty(),
            "a fresh device had lists"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// The point of the exercise: a list made with no server anywhere is made by the
    /// server's own code, through the server's own schema.
    #[test]
    fn a_list_can_be_made_and_read_back() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");

        let made = local.make_list("Household").expect("a list");
        let seen = local.lists().unwrap();

        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].name.0, "Household");
        assert_eq!(seen[0].id, made.id);
        // Owned by the device's person, not by nobody. This is the multi-user model
        // doing its job with one user in it: the row records who, and the answer is a
        // real user id rather than the zero the client used to invent.
        assert_eq!(seen[0].owner_id.0, local.me());

        let _ = std::fs::remove_file(&path);
    }

    /// Reopening is what a device does every time the app starts. The same person must
    /// come back, or every launch would be a new user and the lists would vanish.
    #[test]
    fn reopening_finds_the_same_person_and_the_same_lists() {
        let path = scratch();

        let first = Local::open(&path).expect("a fresh database");
        first.make_list("Household").unwrap();
        let who = first.me();
        drop(first);

        let again = Local::open(&path).expect("the same database");

        assert_eq!(again.me(), who, "the device came back as somebody else");
        assert_eq!(
            again
                .lists()
                .unwrap()
                .iter()
                .map(|l| l.name.0.clone())
                .collect::<Vec<_>>(),
            vec!["Household"]
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_list_can_be_renamed_and_deleted() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        let made = local.make_list("Huosehold").unwrap();

        local.rename_list(made.id.0, "Household").unwrap();
        assert_eq!(local.lists().unwrap()[0].name.0, "Household");

        local.delete_list(made.id.0).unwrap();
        assert!(local.lists().unwrap().is_empty());

        let _ = std::fs::remove_file(&path);
    }

    // ------------------------------------------------------- being told things changed

    /// The one that decides whether the whole shape works.
    ///
    /// A watcher parked all afternoon must not be holding anything the app needs. If
    /// it blocked inside `Local`'s runtime, this test would hang for ever rather than
    /// fail -- which is why it has a deadline and why it is the first one here.
    #[test]
    fn a_parked_watcher_does_not_hold_the_database() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        let mut watching = local.watch_lists();

        std::thread::scope(|threads| {
            let watcher = threads.spawn(move || watching.wait());

            // While that thread is parked, the app carries on reading and writing. This
            // is a phone with a list on screen: the watch never stops, and every tap
            // still has to work.
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(
                local.lists().unwrap().is_empty(),
                "a read was blocked by a watcher"
            );
            local
                .make_list("Household")
                .expect("a write was blocked by a watcher");

            assert_eq!(watcher.join().unwrap(), Some(Change::Lists));
        });

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_change_to_a_watched_list_wakes_it() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        let made = local.make_list("Household").unwrap();
        let mut watching = local.watch_list(made.id.0);

        std::thread::scope(|threads| {
            let watcher = threads.spawn(move || watching.wait());
            std::thread::sleep(std::time::Duration::from_millis(50));
            local.rename_list(made.id.0, "Home").unwrap();

            assert_eq!(watcher.join().unwrap(), Some(Change::List(made.id.0)));
        });

        let _ = std::fs::remove_file(&path);
    }

    /// A screen watching one list should not re-read because a different one moved.
    /// Waking on everything would put a round trip behind every edit anywhere.
    #[test]
    fn a_change_to_another_list_does_not() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        let mine = local.make_list("Household").unwrap();
        let other = local.make_list("Boat").unwrap();
        let mut watching = local.watch_list(mine.id.0);
        let stopper = watching.stopper();

        std::thread::scope(|threads| {
            let watcher = threads.spawn(move || watching.wait());

            std::thread::sleep(std::time::Duration::from_millis(50));
            local.rename_list(other.id.0, "Dinghy").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Still waiting, so stopping is the only way this thread ends. If the
            // filter were wrong it would already have returned `List(mine)`.
            stopper.stop();
            assert_eq!(
                watcher.join().unwrap(),
                None,
                "a watcher woke for another list"
            );
        });

        let _ = std::fs::remove_file(&path);
    }

    /// A screen closing has to end its watch, or the thread outlives it.
    #[test]
    fn stopping_unblocks_a_waiting_watcher() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        let mut watching = local.watch_lists();
        let stopper = watching.stopper();

        std::thread::scope(|threads| {
            let watcher = threads.spawn(move || watching.wait());
            std::thread::sleep(std::time::Duration::from_millis(50));
            stopper.stop();

            assert_eq!(watcher.join().unwrap(), None);
        });

        let _ = std::fs::remove_file(&path);
    }

    /// Stopping before the watcher has parked. The two race by construction -- a screen
    /// can close in the instant between two changes -- and a permit-less notification
    /// would be lost, leaving the thread parked for ever.
    #[test]
    fn stopping_before_it_waits_is_not_lost() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        let mut watching = local.watch_lists();

        watching.stopper().stop();

        assert_eq!(
            watching.wait(),
            None,
            "a stop that arrived first was dropped"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Every change while nobody is listening is not an error. A screen that comes back
    /// re-reads, and `watch` only carries what happens after it is called -- so an app
    /// returning from the background is told nothing and reads everything, which is the
    /// right way round.
    #[test]
    fn changes_before_watching_are_not_delivered() {
        let path = scratch();
        let local = Local::open(&path).expect("a fresh database");
        local.make_list("Household").unwrap();

        let mut watching = local.watch_lists();
        let stopper = watching.stopper();

        std::thread::scope(|threads| {
            let watcher = threads.spawn(move || watching.wait());
            std::thread::sleep(std::time::Duration::from_millis(50));
            stopper.stop();
            assert_eq!(watcher.join().unwrap(), None, "an old change was replayed");
        });

        let _ = std::fs::remove_file(&path);
    }
}
