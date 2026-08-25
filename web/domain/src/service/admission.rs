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

use crate::models::user::Email;

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

impl Admission {
    /// Reads a configured value: `*` for anyone, otherwise a comma-separated list.
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

    /// Whether this address may sign in.
    ///
    /// An identity with no address is refused by a list, because there is nothing to
    /// check it against. Google supplies one for the scopes this asks for, so in
    /// practice this is the case where something has gone wrong — and the safe answer
    /// to "I cannot tell who this is" on a private service is no.
    pub fn admits(&self, email: Option<&Email>) -> bool {
        match self {
            Self::Anyone => true,
            Self::These(listed) => email
                .is_some_and(|address| listed.contains(&address.0.trim().to_lowercase())),
        }
    }
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
