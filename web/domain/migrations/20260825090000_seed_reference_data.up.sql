-- Shared reference data: units and tags.
--
-- These belong to no user. Only Actor::System may write them, and no request can
-- produce an Actor::System -- so a migration is the only way they ever get here.
-- Adding or renaming one is a schema change, deliberately: renaming `kg` renames it
-- on every list in the system, which is not a thing one user should be able to do.
--
-- INSERT OR IGNORE so this is safe against a database that already has some of them,
-- and so a later migration can add to the set without re-checking what landed.
--
-- created_at is left to its default here. The test fixtures re-seed these tables with
-- deliberately staggered timestamps, because ordering tests need to tell created_at
-- order from id order -- see src/models/fixtures/README.md.

INSERT OR IGNORE INTO units (name) VALUES
    ("unit"),
    ("pair"),
    ("dozen"),
    ("pack"),
    ("box"),
    ("bag"),
    ("bottle"),
    ("can"),
    ("jar"),
    ("tin"),
    ("tube"),
    ("sachet"),
    ("roll"),
    ("bunch"),
    ("punnet"),
    ("loaf"),
    ("slice"),
    ("g"),
    ("kg"),
    ("oz"),
    ("pound"),
    ("ml"),
    ("litre"),
    ("fl oz"),
    ("pint"),
    ("gallon"),
    ("tsp"),
    ("tbsp"),
    ("cup"),
    ("cm"),
    ("m");

-- The shop tags are a UK default and the likeliest thing to want changed; add or
-- replace them in a later migration rather than editing this one.
INSERT OR IGNORE INTO tags (name, colour, emoji) VALUES
    ("tesco", "#00539F", "🛒"),
    ("aldi", "#24A9E1", "🛒"),
    ("b&q", "#FFA500", "🛠️"),
    ("boots", "#005EB8", "💊"),
    ("produce", "#4CAF50", "🥬"),
    ("fruits", "#008000", "🍎"),
    ("dairy", "#FFF3C4", "🧀"),
    ("bakery", "#C68642", "🥖"),
    ("meat & fish", "#B03A2E", "🥩"),
    ("frozen", "#7FDBFF", "🧊"),
    ("drinks", "#8E44AD", "🥤"),
    ("pantry", "#8D6E63", "🥫"),
    ("baking", "#F5CBA7", "🧁"),
    ("snacks", "#E67E22", "🍿"),
    ("cleaning", "#00BCD4", "🧽"),
    ("toiletries", "#EC407A", "🪥"),
    ("household", "#607D8B", "🏠"),
    ("diy", "#795548", "🔩"),
    ("party", "#FF4081", "🎉"),
    ("urgent", "#D32F2F", "⚡"),
    ("treat", "#FFD700", "⭐");
