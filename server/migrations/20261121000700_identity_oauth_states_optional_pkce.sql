-- Sign in with Apple (T0.3.5) uses the confidential-client code flow without PKCE (the client
-- secret is a signed JWT), so the verifier is optional.
ALTER TABLE oauth_states ALTER COLUMN pkce_verifier DROP NOT NULL;
