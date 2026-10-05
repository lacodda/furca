//! What the registries already hold. A number a registry has seen is taken
//! for good: crates.io keeps yanked versions and npm never reuses an
//! unpublished one.

use std::time::Duration;

use semver::Version;

use crate::config::Registry;

/// Asks a registry which versions of a package it has ever held.
pub trait Registries {
    /// Every version `registry` holds or has held for `name`; `Ok(None)` when
    /// it has never heard of the package. `Err` says why it could not be
    /// asked.
    fn versions(&self, registry: Registry, name: &str) -> Result<Option<Vec<Version>>, String>;
}

/// The public registries over HTTPS.
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            // crates.io refuses anonymous clients; it asks for a name and a
            // way to reach whoever runs it.
            .user_agent(concat!(
                "furca/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/lacodda/furca)"
            ))
            .http_status_as_error(false)
            .build();
        Http {
            agent: config.into(),
        }
    }
}

impl Registries for Http {
    fn versions(&self, registry: Registry, name: &str) -> Result<Option<Vec<Version>>, String> {
        let url = match registry {
            Registry::Crates => format!("https://crates.io/api/v1/crates/{name}"),
            Registry::Npm => format!("https://registry.npmjs.org/{name}"),
        };
        let mut response = self
            .agent
            .get(&url)
            .header("Accept", "application/json")
            .call()
            .map_err(|e| format!("{} did not answer: {e}", registry.label()))?;
        let status = response.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        if status != 200 {
            return Err(format!(
                "{} answered {status} for `{name}`",
                registry.label()
            ));
        }
        let body: serde_json::Value = response
            .body_mut()
            .read_json()
            .map_err(|e| format!("{} sent what is not JSON: {e}", registry.label()))?;
        Ok(Some(match registry {
            Registry::Crates => crates_versions(&body),
            Registry::Npm => npm_versions(&body),
        }))
    }
}

/// `versions[].num` of a crates.io crate document, yanked ones included.
fn crates_versions(body: &serde_json::Value) -> Vec<Version> {
    body.get("versions")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("num")?.as_str())
        .filter_map(|num| Version::parse(num).ok())
        .collect()
}

/// The keys of `time` in an npm packument: every version ever published,
/// unpublished ones included, besides `created` and `modified`.
fn npm_versions(body: &serde_json::Value) -> Vec<Version> {
    body.get("time")
        .and_then(|t| t.as_object())
        .into_iter()
        .flatten()
        .filter_map(|(key, _)| Version::parse(key).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_yanked_crate_version_is_still_taken() {
        let body = serde_json::json!({
            "crate": { "max_version": "0.2.0" },
            "versions": [
                { "num": "0.2.0", "yanked": false },
                { "num": "0.1.1", "yanked": true },
                { "num": "0.1.0", "yanked": false }
            ]
        });
        let versions: Vec<String> = crates_versions(&body)
            .iter()
            .map(|v| v.to_string())
            .collect();
        assert_eq!(versions, ["0.2.0", "0.1.1", "0.1.0"]);
    }

    #[test]
    fn an_unpublished_npm_version_is_still_taken() {
        let body = serde_json::json!({
            "versions": { "0.2.0": {} },
            "time": {
                "created": "2026-01-01T00:00:00.000Z",
                "modified": "2026-02-01T00:00:00.000Z",
                "0.1.0": "2026-01-01T00:00:00.000Z",
                "0.2.0": "2026-02-01T00:00:00.000Z"
            }
        });
        let mut versions: Vec<String> = npm_versions(&body).iter().map(|v| v.to_string()).collect();
        versions.sort();
        assert_eq!(versions, ["0.1.0", "0.2.0"]);
    }
}
