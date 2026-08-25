//! Notes belong to the person who wrote them, and to no one else.
//!
//! Every function here loads the note, checks it against the actor, and only then
//! acts. There is no shortcut for the "obviously mine" case: the check is what makes
//! the in-process web path as safe as the HTTP one.

use crate::models::note::{self, Body, Note};
use crate::models::{OffsetPage, OrderBy, Paging};

use super::{Actor, Ctx, Result, ServiceError};

/// Writes a note for the actor. A person can only write their own.
pub async fn create(ctx: &Ctx, actor: &Actor, body: Body) -> Result<Note> {
    let author = actor.person()?;
    Ok(Note::create(&ctx.db, author.id, body).await?)
}

/// One page of the actor's own notes.
///
/// There is no way to ask for anybody else's: the author is taken from the actor
/// rather than from an argument, so a caller cannot pass the wrong one.
pub async fn for_user(
    ctx: &Ctx,
    actor: &Actor,
    page: Paging,
    order_by: OrderBy<note::Field>,
) -> Result<OffsetPage<Note>> {
    let author = actor.person()?;
    Ok(Note::for_user(&ctx.db, author.id, page, order_by).await?)
}

/// One note, if it is the actor's.
///
/// Someone else's note reads as missing rather than as forbidden — see
/// [`ServiceError::forbidden`].
pub async fn get(ctx: &Ctx, actor: &Actor, id: note::Id) -> Result<Note> {
    let reader = actor.person()?;
    let note = Note::get(&ctx.db, note::Lookup::Id(id)).await?;

    if note.user_id != reader.id {
        return Err(ServiceError::forbidden("note", reader));
    }

    Ok(note)
}

/// Rewrites the body of the actor's own note.
pub async fn update(ctx: &Ctx, actor: &Actor, id: note::Id, body: Body) -> Result<Note> {
    // load-and-check first: an unauthorised rewrite must not land and then be undone
    get(ctx, actor, id).await?;
    Ok(Note::update(&ctx.db, id, body).await?)
}

/// Deletes the actor's own note.
pub async fn delete(ctx: &Ctx, actor: &Actor, id: note::Id) -> Result<()> {
    get(ctx, actor, id).await?;
    Ok(Note::delete(&ctx.db, id).await?)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;

    use super::*;
    use crate::models::Direction;
    use crate::models::pool;
    use crate::service::tests::person;

    fn all() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by_id() -> OrderBy<note::Field> {
        OrderBy {
            field: note::Field::Id,
            direction: Direction::Ascending,
        }
    }

    /// Two people and a note belonging to the first.
    async fn two_people(pool: &SqlitePool) -> (Actor, Actor, Note) {
        let mine = person(pool, "google-oauth2|owner").await;
        let theirs = person(pool, "google-oauth2|stranger").await;
        let ctx = Ctx::new(pool.clone());
        let note = create(&ctx, &mine, Body("buy milk".into()))
            .await
            .expect("could not write the note");
        (mine, theirs, note)
    }

    // ----------------------------------------------------------------- happy path

    #[rstest]
    #[tokio::test]
    async fn create_writes_for_the_actor(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let author = person(&pool, "github|1").await;
        let ctx = Ctx::new(pool.clone());

        let note = create(&ctx, &author, Body("  buy milk ".into())).await?;

        assert_eq!(note.user_id, author.person()?.id, "written for the actor");
        assert_eq!(
            note.body,
            Body("buy milk".into()),
            "normalised on the way in"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_returns_only_the_actors_notes(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let (mine, theirs, note) = two_people(&pool).await;
        let ctx = Ctx::new(pool.clone());
        create(&ctx, &theirs, Body("not mine".into())).await?;

        let page = for_user(&ctx, &mine, all(), by_id()).await?;

        assert_eq!(page.total, 1, "the other person's note is not counted");
        assert_eq!(page.items[0].id, note.id);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn the_owner_can_read_edit_and_delete(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let (mine, _, note) = two_people(&pool).await;
        let ctx = Ctx::new(pool.clone());

        assert_eq!(get(&ctx, &mine, note.id).await?, note);
        let edited = update(&ctx, &mine, note.id, Body("buy oat milk".into())).await?;
        assert_eq!(edited.body, Body("buy oat milk".into()));
        delete(&ctx, &mine, note.id).await?;
        assert_eq!(get(&ctx, &mine, note.id).await, Err(ServiceError::NotFound));
        Ok(())
    }

    // -------------------------------------------------------------- wrong actor
    //
    // The rule the whole design rests on: a person who is not the owner gets exactly
    // the answer they would get for a note that does not exist, and the row is left
    // untouched.

    #[rstest]
    #[tokio::test]
    async fn a_stranger_cannot_read_it(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let (_, theirs, note) = two_people(&pool).await;
        let ctx = Ctx::new(pool.clone());

        assert_eq!(
            get(&ctx, &theirs, note.id).await,
            Err(ServiceError::NotFound),
            "someone else's note must read as missing, not as forbidden"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn a_stranger_cannot_edit_it(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let (mine, theirs, note) = two_people(&pool).await;
        let ctx = Ctx::new(pool.clone());

        let result = update(&ctx, &theirs, note.id, Body("vandalised".into())).await;

        assert_eq!(result, Err(ServiceError::NotFound));
        assert_eq!(
            get(&ctx, &mine, note.id).await?.body,
            note.body,
            "the refused edit must not have landed"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn a_stranger_cannot_delete_it(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let (mine, theirs, note) = two_people(&pool).await;
        let ctx = Ctx::new(pool.clone());

        let result = delete(&ctx, &theirs, note.id).await;

        assert_eq!(result, Err(ServiceError::NotFound));
        assert_eq!(get(&ctx, &mine, note.id).await?, note, "still there");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn a_missing_note_reads_the_same_as_a_forbidden_one(
        #[future(awt)] pool: SqlitePool,
    ) -> Result<()> {
        let (_, theirs, note) = two_people(&pool).await;
        let ctx = Ctx::new(pool.clone());

        let forbidden = get(&ctx, &theirs, note.id).await;
        let missing = get(&ctx, &theirs, note::Id(9999)).await;

        assert_eq!(forbidden, missing, "the two must be indistinguishable");
        Ok(())
    }

    // ------------------------------------------------------------------- system

    /// The system has no notes, so every note operation refuses it. This is the
    /// counterpart to units and tags, which only the system may write.
    #[rstest]
    #[tokio::test]
    async fn the_system_is_not_a_person(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let ctx = Ctx::new(pool.clone());
        let sys = Actor::System;

        assert_eq!(
            create(&ctx, &sys, Body("whose?".into())).await.unwrap_err(),
            ServiceError::Unauthenticated
        );
        assert_eq!(
            for_user(&ctx, &sys, all(), by_id()).await.unwrap_err(),
            ServiceError::Unauthenticated
        );
        assert_eq!(
            get(&ctx, &sys, note::Id(1)).await.unwrap_err(),
            ServiceError::Unauthenticated
        );
        Ok(())
    }

    // ------------------------------------------------------------ input handling

    #[rstest]
    #[case::empty("")]
    #[case::whitespace_only("   ")]
    #[tokio::test]
    async fn an_empty_body_is_invalid_input(
        #[future(awt)] pool: SqlitePool,
        #[case] body: &str,
    ) -> Result<()> {
        let author = person(&pool, "github|2").await;
        let ctx = Ctx::new(pool.clone());

        let result = create(&ctx, &author, Body(body.into())).await;

        assert_eq!(result, Err(ServiceError::InvalidInput));
        Ok(())
    }
}
