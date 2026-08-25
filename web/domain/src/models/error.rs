use sqlx::error::{DatabaseError, ErrorKind};

/// Models Result
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Model errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid input")]
    InvalidInput,
    #[error("not found")]
    NotFound,
    #[error("already exists")]
    Conflict,
    #[error(transparent)]
    Database(sqlx::Error),
    #[error("system")]
    System,
    #[error("in use")]
    InUse,
}

/// SQLITE_CONSTRAINT_TRIGGER. SQLite implements `ON DELETE RESTRICT` with an internal
/// trigger program, so a delete held back by a child row surfaces as 1811 instead of
/// SQLITE_CONSTRAINT_FOREIGNKEY (787), which is the only code sqlx maps to
/// `ErrorKind::ForeignKeyViolation`.
const SQLITE_CONSTRAINT_TRIGGER: &str = "1811";

/// A delete the database refused because rows still point at the row being removed —
/// `unit::Unit::delete` on a unit some item still uses.
///
/// This is *not* the same failure as a write naming a parent that does not exist, and
/// the two must not collapse into one error: the first says "something else depends
/// on this", the second says "you named something that is not there". Both are the
/// caller's to fix, but only one of them is fixed by deleting the dependants.
fn is_restricted_delete(db: &dyn DatabaseError) -> bool {
    db.code().as_deref() == Some(SQLITE_CONSTRAINT_TRIGGER)
        && db.message().contains("FOREIGN KEY constraint failed")
}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(db) if db.kind() == ErrorKind::UniqueViolation => Self::Conflict,
            sqlx::Error::Database(db) if is_restricted_delete(db.as_ref()) => Self::InUse,
            // a reference to a row that is not there, and a value the column refuses,
            // are both the caller handing us something the database cannot store
            sqlx::Error::Database(db)
                if db.kind() == ErrorKind::ForeignKeyViolation
                    || db.kind() == ErrorKind::CheckViolation
                    || db.kind() == ErrorKind::NotNullViolation =>
            {
                Self::InvalidInput
            }
            _ => Self::Database(err),
        }
    }
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::NotFound, Self::NotFound) => true,
            (Self::Conflict, Self::Conflict) => true,
            (Self::System, Self::System) => true,
            (Self::InUse, Self::InUse) => true,
            (Self::InvalidInput, Self::InvalidInput) => true,
            (Self::Database(a), Self::Database(b)) => a.to_string() == b.to_string(),
            _ => false,
        }
    }
}
