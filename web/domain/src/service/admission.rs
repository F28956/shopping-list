//! Who is allowed in at all.
//!
//! Distinct from authorisation, which is about what a person may do with a list. This
//! is the question before that one: whether this person gets an [`Actor`] at all. A
//! personal service is not made private by owning the domain — anyone with a Google
//! account can complete the sign-in flow, and without this every one of them becomes
//! a user on first sight.
//!
//! [`Actor`]: super::Actor

use std::collections::BTreeSet;

use crate::models::admission::{Admitted, Server, owner_count, set_owner};
use crate::models::user::{self, Email, User};

use super::{Ctx, Result};

/// Who may sign in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Anybody the identity provider vouches for. For an instance that is meant to be
    /// open, and only ever by saying so.
    Anyone,
    /// These addresses and no others, compared without regard to case.
    These(BTreeSet<String>),
}

/// Why a configured admission list could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdmissionError {
    /// A list that admits nobody. Almost certainly a typo or an empty variable, and
    /// starting anyway would lock the owner out of their own service with no clue
    /// why — so it is refused at the point it is read.
    #[error("no addresses were listed; use \"*\" to admit anyone")]
    AdmitsNobody,
}

/// Whether this address may sign in, according to what the server holds.
///
/// The check for somebody arriving for the first time. Anybody who has been here
/// before is answered by [`admits_user`], because a provider address is not stable and
/// the person is.
pub async fn admits_email(ctx: &Ctx, email: Option<&Email>) -> Result<bool> {
    if Server::admits_anyone(&ctx.db).await? {
        return Ok(true);
    }

    // An identity with no address is refused by a list, because there is nothing to
    // check it against. The providers supply one for the scopes this asks for, so in
    // practice this is the case where something has gone wrong -- and the safe answer
    // to "I cannot tell who this is" on a private server is no.
    let Some(email) = email else {
        return Ok(false);
    };

    Ok(Admitted::admits_email(&ctx.db, email).await?)
}

/// Whether this person may sign in, whatever address they arrive with now.
pub async fn admits_user(ctx: &Ctx, user_id: user::Id) -> Result<bool> {
    if Server::admits_anyone(&ctx.db).await? {
        return Ok(true);
    }

    Ok(Admitted::admits_user(&ctx.db, user_id).await?)
}

/// Ties an admitted address to whoever turned out to be behind it.
///
/// Called after a successful sign-in, and silent when there is no row — which is the
/// open-server case, where nothing was admitted by address in the first place.
pub async fn bind(ctx: &Ctx, email: Option<&Email>, user_id: user::Id) -> Result<()> {
    if let Some(email) = email {
        Admitted::bind(&ctx.db, email, user_id).await?;
    }

    Ok(())
}

impl Admission {
    /// Reads a configured value: `*` for anyone, otherwise a comma-separated list.
    ///
    /// Only for seeding. `ALLOWED_EMAILS` is read on the first boot of a server that
    /// has none of this in its database yet, and after that the rows are the truth —
    /// see [`seed`].
    pub fn parse(raw: &str) -> Result<Self, AdmissionError> {
        if raw.trim() == "*" {
            return Ok(Self::Anyone);
        }

        let listed: BTreeSet<String> = raw
            .split(',')
            .map(|entry| entry.trim().to_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect();

        if listed.is_empty() {
            return Err(AdmissionError::AdmitsNobody);
        }

        Ok(Self::These(listed))
    }

    /// Whether this address is in the configured list.
    ///
    /// The in-memory check, over what [`Self::parse`] read. What a running server
    /// asks is [`admits_email`]; this is kept because the seed uses it, and because
    /// the parsing rules deserve tests that do not need a database.
    pub fn admits(&self, email: Option<&Email>) -> bool {
        match self {
            Self::Anyone => true,
            Self::These(listed) => email
                .is_some_and(|address| listed.contains(&address.0.trim().to_lowercase())),
        }
    }
}

/// Moves a configured `ALLOWED_EMAILS` into the database, once.
///
/// Runs on every boot and does nothing on almost all of them. It applies only when
/// there is nothing stored yet — no admitted addresses and no owner — which is true of
/// a fresh install and of the first boot after this migration, and false for ever
/// after. Otherwise a variable left behind in a unit file would quietly undo every
/// removal somebody made through the app.
///
/// On a server that already has users, the earliest-created one becomes the owner.
/// "First person through the door" cannot apply to a server whose door has been open
/// for a year — it would hand it to whoever opened the app next.
pub async fn seed(ctx: &Ctx, configured: Option<&Admission>) -> Result<()> {
    if !Admitted::all(&ctx.db).await?.is_empty() || owner_count(&ctx.db).await? > 0 {
        return Ok(());
    }

    let Some(configured) = configured else {
        return Ok(());
    };

    // The earliest user, if there is one. `None` on a genuinely fresh install, which
    // is the case the claim covers instead.
    let existing = User::earliest(&ctx.db).await?;

    match configured {
        Admission::Anyone => {
            Server::set_admits_anyone(&ctx.db, true).await?;
            tracing::warn!("seeded from ALLOWED_EMAILS=\"*\": anyone may sign in");
        }
        Admission::These(listed) => {
            for address in listed {
                Admitted::seed(&ctx.db, &Email(address.clone()), existing.as_ref().map(|u| u.id))
                    .await?;
            }
            tracing::info!(admitted = listed.len(), "seeded admission from ALLOWED_EMAILS");
        }
    }

    if let Some(user) = existing {
        set_owner(&ctx.db, user.id, true).await?;
        Server::claim(&ctx.db).await?;
        tracing::info!(user = user.id.0, "existing server: the earliest person owns it");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn email(address: &str) -> Email {
        Email(address.to_string())
    }

    #[test]
    fn a_star_admits_anyone() {
        let admission = Admission::parse("*").unwrap();

        assert!(admission.admits(Some(&email("stranger@example.com"))));
        // Including an identity that arrived without an address: an open instance has
        // nothing to check, and has said it does not care.
        assert!(admission.admits(None));
    }

    #[rstest]
    #[case::one("me@example.com")]
    #[case::spaced("  me@example.com  ")]
    #[case::cased("Me@Example.COM")]
    #[case::among_others("someone@example.com, me@example.com ,other@example.com")]
    #[case::trailing_comma("me@example.com,")]
    fn a_listed_address_is_admitted(#[case] configured: &str) {
        let admission = Admission::parse(configured).unwrap();

        assert!(admission.admits(Some(&email("me@example.com"))));
    }

    /// The address on the token is compared the same way as the configured one, or a
    /// capitalised sign-in locks out the person who configured it in lower case.
    #[rstest]
    #[case("ME@EXAMPLE.COM")]
    #[case(" me@example.com ")]
    fn a_listed_address_is_admitted_however_it_arrives(#[case] arriving: &str) {
        let admission = Admission::parse("me@example.com").unwrap();

        assert!(admission.admits(Some(&email(arriving))));
    }

    #[test]
    fn anybody_else_is_not() {
        let admission = Admission::parse("me@example.com").unwrap();

        assert!(!admission.admits(Some(&email("stranger@example.com"))));
        // Nor a near miss: no prefix or domain matching, only the whole address.
        assert!(!admission.admits(Some(&email("me@example.com.evil.test"))));
        assert!(!admission.admits(Some(&email("notme@example.com"))));
        assert!(!admission.admits(None), "an identity with no address");
    }

    #[rstest]
    #[case::empty("")]
    #[case::spaces("   ")]
    #[case::commas(",,")]
    fn a_list_that_admits_nobody_is_refused(#[case] configured: &str) {
        assert_eq!(
            Admission::parse(configured),
            Err(AdmissionError::AdmitsNobody)
        );
    }
}
