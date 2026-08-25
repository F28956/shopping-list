-- Names are capitalised on the way in from now on. Rows already stored were written
-- before that rule and would sit in lower case forever, so the list would read as
-- half transcript and half title until every one of them happened to be edited.
--
-- ASCII only: SQLite's upper() does not know about ångström, and adding ICU to a
-- personal service for one migration is not a trade worth making. Anything it misses
-- is fixed the next time that item is added, by the rule in the domain.
UPDATE items
SET name = upper(substr(name, 1, 1)) || substr(name, 2)
WHERE substr(name, 1, 1) BETWEEN 'a' AND 'z';

-- The same for the memory, or a suggestion would put back the lower-case spelling
-- the moment it was accepted.
UPDATE item_history
SET display = upper(substr(display, 1, 1)) || substr(display, 2)
WHERE substr(display, 1, 1) BETWEEN 'a' AND 'z';
