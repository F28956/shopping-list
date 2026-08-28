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

use domain::models::{Direction, OrderBy, Paging};
use domain::models::list::{self, List, Name};
use domain::models::user::{Email, Name as UserName, Sub};
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
            let page = Paging { number: 1, size: i64::MAX };
            let order = OrderBy { field: list::Field::UpdatedAt, direction: Direction::Descending };

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

    /// This person's id. Exposed because the client stores rows keyed by owner, and a
    /// local row's owner is a real user id here rather than the zero it used to invent.
    pub fn me(&self) -> i64 {
        match &self.me {
            Actor::User(user) => user.id.0,
            _ => 0,
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
        assert!(local.lists().unwrap().is_empty(), "a fresh device had lists");

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
            again.lists().unwrap().iter().map(|l| l.name.0.clone()).collect::<Vec<_>>(),
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
}
