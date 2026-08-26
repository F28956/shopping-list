DROP TRIGGER lists_keep_their_uuid;
DROP TRIGGER items_keep_their_uuid;
DROP TRIGGER lists_are_given_a_uuid;
DROP TRIGGER items_are_given_a_uuid;
DROP INDEX lists_by_uuid;
DROP INDEX items_by_uuid;
ALTER TABLE lists DROP COLUMN uuid;
ALTER TABLE items DROP COLUMN uuid;
