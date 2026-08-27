DROP TABLE server;
ALTER TABLE users DROP COLUMN is_owner;
DROP INDEX admitted_emails_by_user;
DROP TABLE admitted_emails;
