//-- ./src/domains/refresh_tokens.rs

// #![allow(unused)] // For beginning only.

//! JSON Web Token used to authorise a request for a new Access Token
//!
//! Generate a new instance of a Refresh Token and decode an existing Refresh Token
//! into a Token Claim
//! ---

use cookie::Cookie;
use core::time;
use jsonwebtoken::{encode, EncodingKey, Header};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::collections::{self, HashMap};
use tonic::{metadata::MetadataValue, Status};
use uuid::Uuid;

use crate::{database, domain::jwt_token::TokenType, prelude::*};

use super::TokenClaim;

// TODO: Sanitise before parsing
// TODO: Write an into method from a token claim

/// What paths within the domain should the browser send the cookie back to the server.
/// Set to root, so it will be sent for all paths in the domain set within the cookie
pub static COOKIE_PATH: &str = "/";

/// Refresh Token for authorising a new Access Token
// #[derive(serde::Deserialize, Debug, Clone, PartialEq)]
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct RefreshToken(String);

/// Get string reference of the Refresh Token
impl AsRef<str> for RefreshToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Roll our own Display trait for Access Token
impl std::fmt::Display for RefreshToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Convert a String into a Refresh Token
impl From<String> for RefreshToken {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl RefreshToken {
    /// Generate a new Access Token, returning a Result with an AccessToken or BackEnd error
    ///
    /// ## Parameters
    ///
    /// - `secret<&SecretString>` - The token encryption secret
    /// - `issuer<&SecretString>` - The token issuer secret
    /// - 'duration<&time::Duration>` - How long the token is valid for
    /// - `user<&database::Users>` - A database instance of the user to generate the token for
    /// ---
    #[tracing::instrument(
        name = "Generate a new Refresh Token for: "
        skip(secret)
    )]
    pub fn new(
        secret: &SecretString,
        issuer: &SecretString,
        duration: &time::Duration,
        user: &database::Users,
    ) -> Result<Self, AuthenticationError> {
        // Build the Access Token Claim
        let token_claim =
            TokenClaim::new(issuer, duration, user, &TokenType::Refresh);

        // Encode the Token Claim into a URL-safe hash encryption
        let token = encode(
            &Header::default(),
            &token_claim,
            &EncodingKey::from_secret(secret.expose_secret().as_bytes()),
        )?;

        Ok(Self(token))
    }

    #[cfg(test)]
    pub fn mock_data(user: &database::Users) -> Result<Self, AuthenticationError> {
        use fake::faker::company::en::CompanyName;
        use fake::faker::internet::en::Password;
        use fake::Fake;
        // use rand::distr::DistString;
        use rand::distr::SampleString;


        // Generate a random token secret
        let random_secret = rand::distr::Alphanumeric
            .sample_string(&mut rand::rng(), 60);
        let random_secret = SecretString::from(random_secret);

        // Generate a random issuer company
        let random_issuer = CompanyName().fake::<String>();
        let random_issuer = SecretString::from(random_issuer);

        // Generate a random duration between 1 and 10 hours
        // TODO: This should not be random
        let random_duration =
            std::time::Duration::from_secs((1..36000).fake::<u64>());

        let mock_refresh_token = RefreshToken::new(
            &random_secret,
            &random_issuer,
            &random_duration,
            &user,
        )?;

        Ok(mock_refresh_token)
    }

    /// Build the Refresh Token cookie from the token string
    ///
    /// # Build the Refresh Token Cookie
    /// 
    /// !Important notes: 
    ///  - Chrome does not like ports in domain 
    ///  - Browsers will not accept a cookie with a domain that is set as a ip address.
    ///  - GrpcWebFetchTransport on the browser end needs `fetchInit: { credentials: "include", },` set to let cookies header through to the browser
    /// 
    /// TODO: Change to a impl into method
    ///
    /// ## Parameters
    ///
    /// - `domain<&str>` - The domain of the cookie
    /// - `duration<&time::Duration>` - How long is the cookie validate for
    ///
    /// ## References
    /// - https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie
    /// - https://github.com/Houndie/open-dance-registration/blob/main/src/api/authentication.rs
    /// - https://github.com/dlunch/account/blob/main/server/src/handlers/auth.rs
    /// - https://docs.rs/cookie/latest/cookie/struct.CookieBuilder.html
    ///
    #[tracing::instrument(name = "Build a Refresh Token cookie from: ")]
    pub fn build_cookie(&self, domain: &str, duration: &time::Duration) -> Cookie {
        let duration = cookie::time::Duration::new(duration.as_secs() as i64, 0);

        let refresh_cookie = Cookie::build(("refresh_token", self.to_string()))
            // Set the domain of the cookie
            .domain(domain.to_owned())
            // Do not use http
            // .domain("localhost") 
            // Indicates the path that must exist in the requested URL for the browser to send the Cookie header.
            .path(COOKIE_PATH)
            // Indicates the number of seconds until the cookie expires.
            .max_age(duration)
            // Forbids JavaScript from accessing the cookie
            .http_only(true)
            // Indicates that the cookie is sent to the server only when a request is made with the https or localhost
            .secure(false)
            // .same_site(cookie::SameSite::None)
            .build();

        refresh_cookie
    }

    /// # Extract Token Form Header
    ///
    /// Extract the Refresh token string from the tonic request header
    // TODO: Use Cookie parse instead of manually parsing the cookie string
    #[tracing::instrument(
        name = "Extract Refresh Token from http header: ",
        skip(token_secret)
    )]
    pub fn from_header(
        token_secret: &SecretString,
        request_metadata: &tonic::metadata::MetadataMap,
    ) -> Result<Self, AuthenticationError> {
        // Collect all cookies from the request metadata into a vector
        let cookies = request_metadata
            .get_all("cookie")
            .into_iter()
            .collect::<Vec<_>>();
        tracing::debug!("Cookies vector: {cookies:#?}");

        // Create hashmap for storing the header cookies in
        let mut cookies_map: HashMap<&str, &str> = HashMap::new();

        // Inetrate over cookies in the header, adding them to the cookie has map
        for cookie in cookies {
            // Convert the cookie to a string
            let cookie = cookie.to_str().map_err(|_| {
                tracing::error!("Error converting cookie to Ascii string.");
                AuthenticationError::AuthenticationError(
                    "Error converting cookie Ascii to string".to_string(),
                )
            })?;

            // Split the cookie string into key-value pairs
            let parts: Vec<&str> = cookie.split("=").collect();

            // Ensure that the cookie has a key and a value
            if parts.len() != 2 {
                tracing::error!("Key or value missing from cookie string");
                AuthenticationError::AuthenticationError(
                    "Key or value missing from cookie string".to_string(),
                );
            }

            // Insert the key-value pair into the cookie map
            cookies_map.insert(parts[0], parts[1]);
        }

        let refresh_token = cookies_map.get("refresh_token").ok_or(
            AuthenticationError::AuthenticationError(
                "No refresh token in cookie map".to_string(),
            ),
        )?;

        // Build a new refresh token from the refresh token string
        let refresh_token = RefreshToken(refresh_token.to_string());

        Ok(refresh_token)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use fake::faker::company::en::CompanyName;
    use fake::faker::internet::en::DomainSuffix;
    use fake::Fake;
    use rand::distr::{Alphanumeric, SampleString};
    // use rand::distributions::{Alphanumeric, DistString};

    use crate::database;

    // Bring module into test scope
    use super::*;

    // Override with more flexible error
    pub type Result<T> = core::result::Result<T, Error>;
    pub type Error = Box<dyn std::error::Error>;

    #[tokio::test]
    async fn generate_new_refresh_token() -> Result<()> {
        // Generate random secret string
        let secret = Alphanumeric.sample_string(&mut rand::rng(), 60);
        let secret = SecretString::from(secret);

        // Get a random user_id for subject
        let random_user = database::Users::mock_data()?;

        let random_issuer = CompanyName().fake::<String>();
        let random_issuer = SecretString::from(random_issuer);

        let random_duration =
            std::time::Duration::from_secs(Duration::days(30).num_seconds() as u64);

        // Generate a new refresh token
        let refresh_token = RefreshToken::new(
            &secret,
            &random_issuer,
            &random_duration,
            &random_user,
        )?;

        // Encode the refresh token into a Token Claim
        let token_claim =
            TokenClaim::parse(refresh_token.as_ref(), &secret, &random_issuer)?;

        assert_eq!(token_claim.iss, *random_issuer.expose_secret());
        assert_eq!(token_claim.sub, random_user.id.to_string());
        assert_eq!(token_claim.jty, TokenType::Refresh.to_string());

        Ok(())
    }

    #[tokio::test]
    async fn build_refresh_cookie() -> Result<()> {
        // Generate random secret string
        let random_secret = Alphanumeric.sample_string(&mut rand::rng(), 60);
        let random_secret = SecretString::from(random_secret);

        // Generate a random duration between 1 and 10 hours
        let random_duration =
            std::time::Duration::from_secs((1..36000).fake::<u64>());

        // Get a random user_id for subject
        let random_user = database::Users::mock_data()?;

        let random_issuer = CompanyName().fake::<String>();
        let random_issuer = SecretString::from(random_issuer);

        let random_duration =
            std::time::Duration::from_secs(Duration::days(30).num_seconds() as u64);

        // Generate a new refresh token
        let refresh_token = RefreshToken::new(
            &random_secret,
            &random_issuer,
            &random_duration,
            &random_user,
        )?;

        // Encode the refresh token into a Token Claim
        let token_claim = TokenClaim::parse(
            refresh_token.as_ref(),
            &random_secret,
            &random_issuer,
        )?;

        // Generate a random domain
        let domain = DomainSuffix().fake::<String>();

        // Build the refresh token cookie
        let cookie = refresh_token.build_cookie(&domain, &random_duration);

        // Convert to a cookie duration for assertion
        let random_duration: cookie::time::Duration =
            cookie::time::Duration::new(random_duration.as_secs() as i64, 0);

        assert_eq!(cookie.domain(), Some(domain.as_str()));
        assert_eq!(cookie.path(), Some(COOKIE_PATH));
        assert_eq!(cookie.max_age(), Some(random_duration));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(false));

        Ok(())
    }
}
