use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, Utc};
use inquire::{Select, Text};
use jsonwebtoken::{
    decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde_json::{Map, Value};

use crate::{
    cli::{JwtAlgorithmArg, JwtArgs, JwtCommand},
    output,
};

pub fn run(args: JwtArgs) -> Result<()> {
    match args.command {
        JwtCommand::Generate {
            secret,
            algorithm,
            claim,
            expires_in,
        } => {
            let claims = claims_from_pairs(&claim, expires_in)?;
            let token = generate(&secret, algorithm, &claims)?;
            anstream::println!("{token}");
        }
        JwtCommand::Decode {
            token,
            secret,
            algorithm,
        } => {
            let header = decode_header(&token).context("failed to decode JWT header")?;
            let claims = if let Some(secret) = secret {
                verify(&token, &secret, algorithm)?
            } else {
                decode_payload_unverified(&token)?
            };
            let result = serde_json::json!({ "header": header, "claims": claims });
            output::print_json(&result)?;
        }
        JwtCommand::Verify {
            token,
            secret,
            algorithm,
        } => {
            let claims = verify(&token, &secret, algorithm)?;
            output::print_json(&claims)?;
        }
        JwtCommand::Interactive => {
            let secret = Text::new("Secret").prompt()?;
            let selected = Select::new(
                "Algorithm",
                vec![
                    JwtAlgorithmArg::Hs256,
                    JwtAlgorithmArg::Hs384,
                    JwtAlgorithmArg::Hs512,
                ],
            )
            .prompt()?;
            let mut pairs = Vec::new();
            loop {
                let key = Text::new("Claim key (blank to finish)").prompt()?;
                if key.trim().is_empty() {
                    break;
                }
                let value = Text::new("Claim value").prompt()?;
                pairs.push(format!("{key}={value}"));
            }
            let expires = Text::new("Expires in seconds (blank for none)").prompt()?;
            let expires_in = if expires.trim().is_empty() {
                None
            } else {
                Some(
                    expires
                        .trim()
                        .parse::<i64>()
                        .context("expiration must be seconds")?,
                )
            };
            let claims = claims_from_pairs(&pairs, expires_in)?;
            anstream::println!("{}", generate(&secret, selected, &claims)?);
        }
    }
    Ok(())
}

pub fn generate(secret: &str, algorithm: JwtAlgorithmArg, claims: &Value) -> Result<String> {
    let algorithm = algorithm.into();
    let mut header = Header::new(algorithm);
    header.typ = Some("JWT".to_string());
    encode(
        &header,
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("failed to generate JWT")
}

pub fn verify(token: &str, secret: &str, algorithm: JwtAlgorithmArg) -> Result<Value> {
    let algorithm = algorithm.into();
    let mut validation = Validation::new(algorithm);
    validation.validate_exp = true;
    validation.required_spec_claims.clear();
    let data = decode::<Value>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .context("JWT verification failed")?;
    Ok(data.claims)
}

fn claims_from_pairs(pairs: &[String], expires_in: Option<i64>) -> Result<Value> {
    let mut map = Map::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            anyhow::bail!("claim must use key=value format: {pair}");
        };
        map.insert(key.to_string(), parse_claim_value(value));
    }
    if let Some(seconds) = expires_in {
        let expires = Utc::now() + Duration::seconds(seconds);
        map.insert("exp".to_string(), Value::from(expires.timestamp()));
    }
    Ok(Value::Object(map))
}

fn parse_claim_value(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

fn decode_payload_unverified(token: &str) -> Result<Value> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        anyhow::bail!("JWT must have three dot-separated segments");
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("failed to base64-decode JWT payload")?;
    serde_json::from_slice(&bytes).context("JWT payload is not valid JSON")
}

impl From<JwtAlgorithmArg> for Algorithm {
    fn from(value: JwtAlgorithmArg) -> Self {
        match value {
            JwtAlgorithmArg::Hs256 => Algorithm::HS256,
            JwtAlgorithmArg::Hs384 => Algorithm::HS384,
            JwtAlgorithmArg::Hs512 => Algorithm::HS512,
        }
    }
}

impl std::fmt::Display for JwtAlgorithmArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JwtAlgorithmArg::Hs256 => write!(formatter, "HS256"),
            JwtAlgorithmArg::Hs384 => write!(formatter, "HS384"),
            JwtAlgorithmArg::Hs512 => write!(formatter, "HS512"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn jwt_round_trip() {
        let mut claims = BTreeMap::new();
        claims.insert("sub", "123");
        let claims = serde_json::to_value(claims).unwrap();
        let token = generate("secret", JwtAlgorithmArg::Hs256, &claims).unwrap();
        let verified = verify(&token, "secret", JwtAlgorithmArg::Hs256).unwrap();
        assert_eq!(verified["sub"], "123");
    }
}
