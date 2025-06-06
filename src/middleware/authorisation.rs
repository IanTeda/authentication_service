//-- ./src/middleware/authorisation.rs

// #![allow(unused)] // For beginning only.

/// # Access Token Interceptor
///
/// This interceptor is used to validate the access token in the request
/// metadata. It checks if the access token is present and valid.
/// If the access token is not present or invalid, it returns an error.
use secrecy::SecretString;

use crate::{domain, prelude::*};
use std::str::FromStr;

#[derive(Clone)]
pub struct AuthorisationInterceptor {
    pub(crate) token_secret: SecretString,
    pub(crate) issuer: SecretString,
    pub(crate) allowable_roles: Vec<domain::UserRole>,
}

impl tonic::service::Interceptor for AuthorisationInterceptor {
    /// Intercept the request and check if the access token is valid
    #[tracing::instrument(
        name = "Authorisation Interceptor: ", 
        skip(self),
        fields(
            header = ?request.metadata(),
        )
    )]
    fn call(
        &mut self,
        request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        // Get the metadata from the tonic request
        let metadata: &tonic::metadata::MetadataMap = request.metadata();

        // Get the access token from the header metadata
        let access_token_bearer = domain::AccessToken::parse_header(metadata)?;

        println!("Access Token: {:?}", access_token_bearer);

        // Using the Token Secret decode the Access Token string into a Token Claim.
        // This validates the token expiration, not before and Issuer.
        // TODO: Map out domains and refactor tokens
        let access_token_claim = domain::TokenClaim::parse(
            &access_token_bearer.to_string(),
            &self.token_secret,
            &self.issuer,
        )
        .map_err(|_| {
            tracing::error!("Access Token is invalid! Unable to parse token claim.");
            // Return error
            AuthenticationError::AuthenticationError(
                "Authentication Failed!".to_string(),
            )
        })?;
        tracing::debug!(
            "Access Token authenticated for user: {}",
            access_token_claim.sub
        );

        // Parse Token Claim user role into domain user role
        let role = domain::UserRole::from_str(access_token_claim.jur.as_str())
            .map_err(|_| {
                tracing::error!("Access Token user role is invalid!");
                // Return error
                AuthenticationError::AuthenticationError(
                    "Authentication Failed! No valid auth token.".to_string(),
                )
            })?;

        // Check if the access token user role is in the list of allowable roles
        // If not, return an error
        if !self.allowable_roles.contains(&role) {
            tracing::error!("Access Token user role is not authorised!");
            // Return error
            return Err(tonic::Status::unauthenticated("Authentication Failed!"));
        }

        tracing::info!("Authorization request header validated.");

        Ok(request)
    }
}