use std::sync::OnceLock;

use synveil_core::{LoginIdentifier, Timestamp, User, UserId, UserStatus};
use synveil_metadata::{
    AuthRepository, BootstrapAttempt, BootstrapState, DatabasePool, SessionRow, UserCredentialRow,
    UserRow,
};

use crate::{
    AuthError, AuthenticatedSession, PasswordHasherConfig, PasswordVerification, PlaintextPassword,
    SessionConfig, SessionCredential, SessionExpiry, SessionId, SessionPrincipal, SessionToken,
    StoredPasswordHash, sessions::truncate_to_microseconds,
};

const DUMMY_PASSWORD: &str = "synveil-authentication-dummy-password";

/// Authentication application boundary for password credentials, one-time
/// setup, and transport-neutral browser sessions. HTTP sessions, cookies, and
/// login transport remain deliberately absent.
pub struct AuthenticationService<'pool> {
    repository: AuthRepository<'pool>,
    password_config: PasswordHasherConfig,
    session_config: SessionConfig,
    dummy_password_hash: OnceLock<Result<StoredPasswordHash, AuthError>>,
}

impl<'pool> AuthenticationService<'pool> {
    #[must_use]
    pub fn new(pool: &'pool DatabasePool, password_config: PasswordHasherConfig) -> Self {
        Self::with_session_config(pool, password_config, SessionConfig::default())
    }

    #[must_use]
    pub fn with_session_config(
        pool: &'pool DatabasePool,
        password_config: PasswordHasherConfig,
        session_config: SessionConfig,
    ) -> Self {
        Self {
            repository: AuthRepository::new(pool),
            password_config,
            session_config,
            dummy_password_hash: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn new_with_session_config(
        pool: &'pool DatabasePool,
        password_config: PasswordHasherConfig,
        session_config: SessionConfig,
    ) -> Self {
        Self::with_session_config(pool, password_config, session_config)
    }

    #[must_use]
    pub fn with_default_password_config(pool: &'pool DatabasePool) -> Self {
        Self::new(pool, PasswordHasherConfig::default())
    }

    #[must_use]
    pub fn with_default_password_config_and_session_config(
        pool: &'pool DatabasePool,
        session_config: SessionConfig,
    ) -> Self {
        Self::with_session_config(pool, PasswordHasherConfig::default(), session_config)
    }

    #[must_use]
    pub const fn password_config(&self) -> PasswordHasherConfig {
        self.password_config
    }

    #[must_use]
    pub const fn session_config(&self) -> SessionConfig {
        self.session_config
    }

    pub async fn bootstrap_state(&self) -> Result<BootstrapState, AuthError> {
        match self.repository.bootstrap_state().await? {
            BootstrapState::Open => Ok(BootstrapState::Open),
            BootstrapState::Closed => Ok(BootstrapState::Closed),
            BootstrapState::Inconsistent => Err(AuthError::BootstrapStateInvalid),
        }
    }

    /// Create the one and only first administrator through the repository's
    /// locked PostgreSQL transaction.
    pub async fn create_first_admin(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
        observed_at: Timestamp,
    ) -> Result<User, AuthError> {
        // A live server clock commonly contains nanoseconds while the
        // PostgreSQL metadata boundary deliberately accepts microseconds only.
        // Normalize once, as session creation already does, so user,
        // credential, initial library and root share one durable timestamp.
        let observed_at = truncate_to_microseconds(observed_at);
        let password_hash = StoredPasswordHash::hash(self.password_config, &password)?;
        let user = User::rehydrate_with_admin(
            UserId::new(),
            login,
            UserStatus::Active,
            true,
            observed_at,
            observed_at,
            synveil_core::Revision::new(0),
        );
        let user_row = UserRow::from_domain(&user).map_err(|_| AuthError::InvalidPersistedData)?;
        let credential_row = UserCredentialRow::new(
            user.id().into_uuid(),
            password_hash.as_phc_str().to_owned(),
            observed_at.as_offset_datetime(),
            observed_at.as_offset_datetime(),
        );

        match self
            .repository
            .create_first_admin(&user_row, &credential_row)
            .await?
        {
            BootstrapAttempt::Created => Ok(user),
            BootstrapAttempt::AlreadyClosed => Err(AuthError::BootstrapClosed),
            BootstrapAttempt::Inconsistent => Err(AuthError::BootstrapStateInvalid),
        }
    }

    /// Authenticate a login identifier and password using the server clock.
    /// A successful call always creates a fresh session credential.
    pub async fn authenticate(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
    ) -> Result<AuthenticatedSession, AuthError> {
        self.authenticate_at(login, password, Timestamp::now())
            .await
    }

    /// Deterministic-time form used by callers that already have a server-
    /// observed timestamp, including integration tests.
    pub async fn authenticate_at(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
        observed_at: Timestamp,
    ) -> Result<AuthenticatedSession, AuthError> {
        let Some(login_row) = self.repository.load_login(&login).await? else {
            self.verify_dummy_password(&password)?;
            return Err(AuthError::InvalidCredentials);
        };

        let user = login_row
            .user_row()
            .try_into_domain()
            .map_err(|_| AuthError::InvalidPersistedData)?;
        let Some(credential) = login_row.credential_row() else {
            self.verify_dummy_password(&password)?;
            return Err(AuthError::InvalidCredentials);
        };

        let stored = StoredPasswordHash::try_from_phc(credential.password_hash)?;
        let needs_rehash = match self.verify_password(&stored, &password)? {
            PasswordVerification::Verified { needs_rehash } => needs_rehash,
            PasswordVerification::Rejected => return Err(AuthError::InvalidCredentials),
        };

        if user.status() != UserStatus::Active {
            return Err(AuthError::InvalidCredentials);
        }

        // Prompt 13 exposes rehash-needed. Rehash only after successful
        // verification and before creating the new session; no session is
        // issued if the replacement cannot be persisted.
        if needs_rehash {
            let replacement = StoredPasswordHash::hash(self.password_config, &password)?;
            let replaced = self
                .repository
                .replace_password_hash(
                    user.id(),
                    replacement.as_phc_str(),
                    truncate_to_microseconds(observed_at).as_offset_datetime(),
                )
                .await?;
            if !replaced {
                return Err(AuthError::InvalidPersistedData);
            }
        }

        self.create_session_for_user(&user, observed_at).await
    }

    /// Load only the verifier needed by a future authentication flow. The
    /// service never exposes a raw database row or formats the PHC string.
    pub async fn load_password_hash(
        &self,
        user_id: UserId,
    ) -> Result<Option<StoredPasswordHash>, AuthError> {
        let Some(row) = self.repository.load_credential(user_id).await? else {
            return Ok(None);
        };
        StoredPasswordHash::try_from_phc(row.password_hash)
            .map(Some)
            .map_err(AuthError::from)
    }

    pub fn verify_password(
        &self,
        stored: &StoredPasswordHash,
        password: &PlaintextPassword,
    ) -> Result<PasswordVerification, AuthError> {
        stored
            .verify(self.password_config, password)
            .map_err(AuthError::from)
    }

    /// Resolve a typed raw bearer credential with the server clock. The
    /// caller receives only the minimal authorization principal.
    pub async fn authenticate_session(
        &self,
        token: &SessionToken,
    ) -> Result<SessionPrincipal, AuthError> {
        self.authenticate_session_at(token, Timestamp::now()).await
    }

    pub async fn authenticate_session_at(
        &self,
        token: &SessionToken,
        observed_at: Timestamp,
    ) -> Result<SessionPrincipal, AuthError> {
        let digest = token.digest();
        let Some(row) = self
            .repository
            .load_session_by_token_digest(&digest)
            .await?
        else {
            return Err(AuthError::InvalidSession);
        };

        if !token.digest_matches(&row.token_digest) {
            return Err(AuthError::InvalidSession);
        }

        let session_id =
            SessionId::try_from_uuid(row.id).map_err(|_| AuthError::InvalidPersistedData)?;
        let user_id =
            UserId::try_from_uuid(row.user_id).map_err(|_| AuthError::InvalidPersistedData)?;
        let expiry = SessionExpiry::new(Timestamp::from_offset_datetime(row.expires_at));

        if row.revoked_at.is_some() || expiry.is_expired_at(observed_at) {
            return Err(AuthError::InvalidSession);
        }

        let Some(user_row) = self.repository.load_user(user_id).await? else {
            return Err(AuthError::InvalidSession);
        };
        let user = user_row
            .try_into_domain()
            .map_err(|_| AuthError::InvalidPersistedData)?;
        if user.status() != UserStatus::Active {
            return Err(AuthError::InvalidSession);
        }

        Ok(SessionPrincipal::new(
            user.id(),
            user.is_instance_admin(),
            session_id,
        ))
    }

    /// Transport adapters can use this boundary when they have not yet
    /// parsed the cookie/header representation into a typed token.
    pub async fn authenticate_session_raw(
        &self,
        raw_token: &str,
    ) -> Result<SessionPrincipal, AuthError> {
        let token = SessionToken::try_from(raw_token)?;
        self.authenticate_session(&token).await
    }

    pub async fn authenticate_session_raw_at(
        &self,
        raw_token: &str,
        observed_at: Timestamp,
    ) -> Result<SessionPrincipal, AuthError> {
        let token = SessionToken::try_from(raw_token)?;
        self.authenticate_session_at(&token, observed_at).await
    }

    /// Revoke a known session record. Repeated calls are safe and do not
    /// reveal whether the record was already revoked or absent.
    pub async fn revoke_session(&self, session_id: SessionId) -> Result<(), AuthError> {
        self.revoke_session_at(session_id, Timestamp::now()).await
    }

    pub async fn revoke_session_at(
        &self,
        session_id: SessionId,
        revoked_at: Timestamp,
    ) -> Result<(), AuthError> {
        self.repository
            .revoke_session(
                session_id.into_uuid(),
                truncate_to_microseconds(revoked_at).as_offset_datetime(),
            )
            .await
            .map_err(AuthError::from)
    }

    /// Revoke the session represented by the current bearer credential. This
    /// does not require the session to be unexpired; logout remains useful at
    /// an expiry boundary while an unknown token is rejected generically.
    pub async fn revoke_current_session(&self, token: &SessionToken) -> Result<(), AuthError> {
        self.revoke_current_session_at(token, Timestamp::now())
            .await
    }

    pub async fn revoke_current_session_at(
        &self,
        token: &SessionToken,
        revoked_at: Timestamp,
    ) -> Result<(), AuthError> {
        let digest = token.digest();
        let Some(row) = self
            .repository
            .load_session_by_token_digest(&digest)
            .await?
        else {
            return Err(AuthError::InvalidSession);
        };
        if !token.digest_matches(&row.token_digest) {
            return Err(AuthError::InvalidSession);
        }
        let session_id =
            SessionId::try_from_uuid(row.id).map_err(|_| AuthError::InvalidPersistedData)?;
        self.revoke_session_at(session_id, revoked_at).await
    }

    /// Define the cleanup boundary without introducing a scheduler. Correct
    /// authentication never depends on this deletion having run.
    pub async fn cleanup_sessions(&self, observed_at: Timestamp) -> Result<u64, AuthError> {
        self.repository
            .cleanup_sessions(truncate_to_microseconds(observed_at).as_offset_datetime())
            .await
            .map_err(AuthError::from)
    }

    /// Replace a verifier for a future, already-authorized password-change or
    /// rehash flow. No caller-supplied session is accepted by this operation.
    pub async fn replace_password_hash(
        &self,
        user_id: UserId,
        password_hash: &StoredPasswordHash,
        updated_at: Timestamp,
    ) -> Result<(), AuthError> {
        if self
            .repository
            .replace_password_hash(
                user_id,
                password_hash.as_phc_str(),
                updated_at.as_offset_datetime(),
            )
            .await?
        {
            Ok(())
        } else {
            Err(AuthError::CredentialNotFound)
        }
    }

    async fn create_session_for_user(
        &self,
        user: &User,
        observed_at: Timestamp,
    ) -> Result<AuthenticatedSession, AuthError> {
        let session_id = SessionId::new();
        let token = SessionToken::generate();
        let created_at = truncate_to_microseconds(observed_at);
        let expiry = self
            .session_config
            .expiry_at(observed_at)
            .map_err(|_| AuthError::InvalidPersistedData)?;
        let expiry = SessionExpiry::new(truncate_to_microseconds(expiry.expires_at()));
        let row = SessionRow::new(
            session_id.into_uuid(),
            user.id().into_uuid(),
            token.digest().to_vec(),
            created_at.as_offset_datetime(),
            expiry.expires_at().as_offset_datetime(),
            None,
        );

        self.repository.create_session(&row).await?;

        let principal = SessionPrincipal::new(user.id(), user.is_instance_admin(), session_id);
        let credential = SessionCredential::new(session_id, token, expiry);
        Ok(AuthenticatedSession::new(credential, principal))
    }

    fn verify_dummy_password(&self, password: &PlaintextPassword) -> Result<(), AuthError> {
        let result = self.dummy_password_hash.get_or_init(|| {
            let dummy = PlaintextPassword::new(DUMMY_PASSWORD).map_err(AuthError::from)?;
            StoredPasswordHash::hash(self.password_config, &dummy).map_err(AuthError::from)
        });
        let dummy_hash = result.as_ref().map_err(|error| *error)?;
        let _ = dummy_hash.verify(self.password_config, password)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AuthError;

    #[test]
    fn authentication_failures_are_stable_and_safe() {
        assert_eq!(
            AuthError::InvalidCredentials.to_string(),
            "invalid_credentials"
        );
        assert_eq!(AuthError::InvalidSession.to_string(), "invalid_session");
        assert_eq!(AuthError::InvalidCredentials, AuthError::InvalidCredentials);
        assert!(!AuthError::InvalidCredentials.to_string().contains("lookup"));
        assert!(
            !AuthError::InvalidCredentials
                .to_string()
                .contains("password")
        );
    }

    #[test]
    fn bootstrap_closed_error_is_stable_and_safe() {
        let error = AuthError::BootstrapClosed;
        assert_eq!(error.to_string(), "bootstrap_closed");
        assert!(!error.to_string().contains("password"));
    }
}
