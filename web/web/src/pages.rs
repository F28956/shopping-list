//! One module per page, each one thin: read the actor, call the service, render.

#[cfg(test)]
mod tests;

pub mod items;
pub mod lists;
pub mod notes;

use domain::models::user::User;

/// What to call someone in the header.
pub fn who(user: &User) -> String {
    user.name
        .as_ref()
        .map(|n| n.0.clone())
        .or_else(|| user.email.as_ref().map(|e| e.0.clone()))
        .unwrap_or_else(|| "you".to_string())
}
