-- Persist the display metadata for user attachments so switching threads or
-- restarting the desktop app never removes attachment chips from history.
ALTER TABLE messages ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]';
