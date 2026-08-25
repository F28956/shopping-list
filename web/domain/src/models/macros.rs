macro_rules! string {
    ($($t:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, ::sqlx::Type, ::serde::Serialize)]
        #[sqlx(transparent)]
        pub struct $t(pub String);
    )*};
}

/// Gives each [`string!`] type a `normalized` method: the stored form of that value,
/// trimmed and lowercased.
///
/// Opt in per type rather than generating this for every string newtype, because
/// case-folding is not always right — `user::Sub` is an identity key, and folding it
/// would merge two identities.
///
/// Normalising in Rust rather than in SQL is deliberate. SQLite's `lower()` and its
/// `COLLATE NOCASE` both fold ASCII only, so `lower(trim(?))` stores `Ångström`
/// unchanged and a `UNIQUE` index then happily accepts `ångström` alongside it — two
/// rows for one value. `str::to_lowercase` is Unicode-aware, and `str::trim` strips
/// every Unicode space rather than just `U+0020`.
///
/// A `CHECK (name <> '' AND name = trim(name))` on the column is the backstop, not
/// the mechanism: it is what turns a value that normalises to empty into
/// [`crate::models::Error::InvalidInput`].
macro_rules! normalized {
    ($($t:ident),* $(,)?) => {$(
        impl $t {
            /// This value as it is stored: trimmed, then lowercased — see
            /// [`normalized!`].
            ///
            /// Takes `self` by value because callers own the value they are about to
            /// write or look up, and normalising reallocates either way.
            pub fn normalized(self) -> Self {
                Self(self.0.trim().to_lowercase())
            }
        }
    )*};
}

/// Gives each [`string!`] type a `trimmed` method: surrounding whitespace removed,
/// case left alone.
///
/// The counterpart to [`normalized!`], for free text a person reads back — display
/// names, list titles, note bodies. Case is meaning there (`Ana María López` is not
/// `ana maría lópez`), so only the padding comes off. Use [`normalized!`] instead
/// when the value is a key that must dedupe across case.
///
/// `str::trim` strips every Unicode space, where SQLite's `trim()` strips only
/// `U+0020` — so a `CHECK (x = trim(x))` in the schema is a weaker backstop than
/// this, not an equivalent one.
macro_rules! trimmed {
    ($($t:ident),* $(,)?) => {$(
        impl $t {
            /// This value as it is stored: surrounding whitespace removed, case kept
            /// — see [`trimmed!`].
            pub fn trimmed(self) -> Self {
                Self(self.0.trim().to_string())
            }
        }
    )*};
}

/// Gives each [`string!`] type a `capitalised` method: the first letter upper-cased,
/// so a list reads as a list rather than as a transcript of typing.
///
/// A first word that already carries a capital is left alone. `iPhone charger` and
/// `BBQ sauce` are spelled that way on purpose, and "capitalise" would spell them
/// `IPhone charger` and `BBQ sauce` — one of those is worse than doing nothing.
macro_rules! capitalised {
    ($($t:ident),* $(,)?) => {$(
        impl $t {
            pub fn capitalised(self) -> Self {
                Self($crate::models::capitalise(&self.0))
            }
        }
    )*};
}

macro_rules! i64 {
($($t:ident),* $(,)?) => {$(
    #[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq, Hash, sqlx::Type, serde::Serialize)]
    #[sqlx(transparent)]
    pub struct $t(pub i64);
)*};
}

macro_rules! f64 {
($($t:ident),* $(,)?) => {$(
    /// No `Eq`, `Ord` or `Hash`: a float has neither a total order nor reflexive
    /// equality, and deriving them here would be a lie the compiler cannot catch.
    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, sqlx::Type, serde::Serialize)]
    #[sqlx(transparent)]
    pub struct $t(pub f64);
)*};
}

macro_rules! timestamp {
($($t:ident),* $(,)?) => {$(
    #[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq, Hash, sqlx::Type, serde::Serialize)]
    #[sqlx(transparent)]
    pub struct $t(#[serde(with = "time::serde::rfc3339")] pub OffsetDateTime);
)*};
}

/// The contents of one or more fixture files, concatenated in the order given —
/// a single `&'static str` to hand to [`crate::models::pool`].
///
/// Paths are relative to `src/models/`, and are embedded at compile time, so a
/// typo fails the build rather than the test run. List the files in dependency
/// order: `users → lists → units → items → tags`.
///
/// ```ignore
/// #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
/// #[future(awt)]
/// pool: SqlitePool,
/// ```
#[cfg(test)]
macro_rules! seeds {
    ($($path:literal),+ $(,)?) => {
        concat!($(include_str!($path), "\n"),+)
    };
}
