DROP INDEX IF EXISTS tags_by_sort_order;
ALTER TABLE tags DROP COLUMN sort_order;
