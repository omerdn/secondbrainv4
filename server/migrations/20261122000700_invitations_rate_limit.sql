-- Counting a person's recent invitations for the rate limit (ARCHITECTURE §15, T1.2.9).
-- Additive only.

CREATE INDEX invitations_sender_recent_idx ON invitations (from_user, created_at DESC);
