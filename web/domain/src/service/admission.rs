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

use crate::models::admission::{Admitted, Note, Server, key, owner_count, owners, set_owner};
use crate::models::user::{self, Email, User};

use super::{Actor, Ctx, Result, ServiceError};

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

/// Whether this person may administer the server.
pub async fn is_owner(ctx: &Ctx, user_id: user::Id) -> Result<bool> {
    Ok(owners(&ctx.db).await?.contains(&user_id))
}

/// Refuses anybody who is not an owner.
///
/// Here rather than in a handler, per D1: the browser and the API ask the same
/// question and must get the same answer, and a check written twice is a check that
/// disagrees with itself eventually.
async fn owner_only(ctx: &Ctx, actor: &Actor) -> Result<user::Id> {
    let person = actor.person()?;

    if !is_owner(ctx, person.id).await? {
        return Err(ServiceError::Forbidden);
    }

    Ok(person.id)
}

/// Every admitted address, for the screen that manages them.
pub async fn listing(ctx: &Ctx, actor: &Actor) -> Result<Vec<Admitted>> {
    owner_only(ctx, actor).await?;
    Ok(Admitted::all(&ctx.db).await?)
}

/// Admits an address. Admitting one twice is a double-click, not an error.
pub async fn admit(
    ctx: &Ctx,
    actor: &Actor,
    email: &Email,
    note: Option<&Note>,
) -> Result<()> {
    let by = owner_only(ctx, actor).await?;

    if key(email).is_empty() {
        return Err(ServiceError::InvalidInput);
    }

    Admitted::add(&ctx.db, email, by, note).await?;
    tracing::info!(by = by.0, "address admitted");
    Ok(())
}

/// Withdraws an address.
///
/// The rule that matters is A5's second half: the last owner cannot withdraw their
/// own admission. Removal takes effect on the next request, so doing it would sign
/// them out of a server with nobody left who can let anybody back in — and the way
/// back involves `sqlite3` on the host. The person most likely to try it is the one
/// tidying up their own address at two in the morning.
pub async fn withdraw(ctx: &Ctx, actor: &Actor, email: &Email) -> Result<()> {
    let by = owner_only(ctx, actor).await?;

    let theirs = Admitted::all(&ctx.db)
        .await?
        .into_iter()
        .find(|row| row.email.0 == key(email))
        .and_then(|row| row.user_id);

    if theirs == Some(by) && owner_count(&ctx.db).await? <= 1 {
        return Err(ServiceError::InUse);
    }

    if !Admitted::remove(&ctx.db, email).await? {
        return Err(ServiceError::NotFound);
    }

    tracing::info!(by = by.0, "address withdrawn");
    Ok(())
}

/// Promotes somebody to owner, or demotes them.
///
/// A5's first half: the last owner cannot be demoted, including by themselves. A
/// server with no owner has no way back that does not involve a shell on the host.
///
/// Promotion makes a second owner equal to the first, deliberately — the alternative
/// is a hierarchy where somebody cannot be demoted by the person they promoted, and
/// nobody has asked for that.
pub async fn set_ownership(
    ctx: &Ctx,
    actor: &Actor,
    subject: user::Id,
    owner: bool,
) -> Result<()> {
    let by = owner_only(ctx, actor).await?;

    if !owner && owner_count(&ctx.db).await? <= 1 && is_owner(ctx, subject).await? {
        return Err(ServiceError::InUse);
    }

    // An owner who cannot sign in is the same problem as no owner, arrived at from a
    // different direction.
    if owner && !admits_user(ctx, subject).await? {
        return Err(ServiceError::InvalidInput);
    }

    set_owner(&ctx.db, subject, owner).await?;
    tracing::info!(by = by.0, subject = subject.0, owner, "ownership changed");
    Ok(())
}

/// Opens the server to anybody a provider vouches for, or closes it again.
///
/// Logged loudly on the way in, exactly as `ALLOWED_EMAILS="*"` is: it is a
/// legitimate thing to want and it should never be something that happened quietly.
pub async fn set_open(ctx: &Ctx, actor: &Actor, open: bool) -> Result<()> {
    let by = owner_only(ctx, actor).await?;

    Server::set_admits_anyone(&ctx.db, open).await?;

    if open {
        tracing::warn!(by = by.0, "the server now admits anyone who can sign in");
    } else {
        tracing::info!(by = by.0, "the server admits only listed addresses");
    }

    Ok(())
}

/// Claims an unclaimed server, making this person its first owner.
///
/// A1: an empty server admits its first caller and hands them the keys. A2: which
/// makes the gap between starting the process and somebody claiming it a land grab, on
/// a machine anybody can reach — and the person it happens to gets no warning at all,
/// they are simply refused from their own server.
///
/// The code closes it. It is printed to the log at boot, which a self-hoster starting
/// a process is already looking at, and it needs no configuration and no environment
/// variable — which is the thing this whole design is trying to stop requiring.
///
/// Admission is deliberately not consulted. This is the one path where checking it
/// would refuse everybody, there being nobody yet who could have admitted anyone.
pub async fn claim(
    ctx: &Ctx,
    code: &str,
    provider: &str,
    sub: user::Sub,
    name: Option<user::Name>,
    email: Option<Email>,
) -> Result<Actor> {
    // Answered before the code is looked at, so that a claimed server tells a late
    // arrival the same thing whatever they typed.
    if Server::is_claimed(&ctx.db).await? {
        tracing::warn!("a claim arrived for a server that is already claimed");
        return Err(ServiceError::Forbidden);
    }

    let Some(expected) = ctx.claim_code.as_deref() else {
        tracing::warn!("a claim arrived, and this process is not offering one");
        return Err(ServiceError::Forbidden);
    };

    if !constant_time_eq(code, expected) {
        tracing::warn!("a claim arrived with the wrong code");
        return Err(ServiceError::Forbidden);
    }

    // The person first, because the claim below has to name them and a foreign key
    // says so. A user created here and then beaten to the claim is left admitted by
    // nobody, which means they cannot sign in — the stray row is harmless and the
    // race is not.
    let user = super::identity::create(ctx, provider, sub, name, email.clone()).await?;

    if !Server::claim_for(&ctx.db, user.id).await? {
        tracing::warn!("two claims raced and this one lost");
        return Err(ServiceError::Forbidden);
    }

    // Only now: an address admitted before the claim was won would admit somebody who
    // did not win it.
    if let Some(email) = email {
        Admitted::seed(&ctx.db, &email, Some(user.id)).await?;
        Admitted::bind(&ctx.db, &email, user.id).await?;
    } else {
        // No address means nothing to admit by, and `admits_user` reads that table —
        // so an owner with no address would be an owner who cannot sign in. Refused
        // rather than left to be discovered on the next request.
        tracing::error!(user = user.id.0, "claimed with no address to admit");
        return Err(ServiceError::InvalidInput);
    }

    tracing::info!(user = user.id.0, "the server has been claimed");
    Ok(Actor::User(user))
}

/// Compares without telling the caller how far it got.
///
/// A claim code is short enough to be guessed a character at a time if the comparison
/// stops at the first difference and somebody can measure it. Unlikely over a home
/// network and cheap to remove as a question.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A code somebody can read off a log and type without a mistake.
///
/// No vowels, so it cannot spell anything; no `0`, `O`, `1`, `I` or `L`, which is
/// where transcription actually goes wrong. Grouped, because a person is copying it
/// from a terminal into a phone.
pub fn new_claim_code() -> String {
    use rand::RngExt;

    const ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRSTVWXYZ";

    let mut rng = rand::rng();
    let mut pick = || ALPHABET[rng.random_range(0..ALPHABET.len())] as char;

    let code: String = (0..8).map(|_| pick()).collect();
    format!("{}-{}", &code[..4], &code[4..])
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
