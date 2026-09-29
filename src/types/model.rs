//! Provider-visible model identifiers.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};

use super::errors::ModelError;

/// Provider-visible model identifier accepted from CLI and JSON bodies.
///
/// Empty or whitespace-only model ids are rejected at the boundary. The default
/// is the bundled DOCTOR model id used by the server.
///
/// ```
/// use eliza::types::ModelId;
///
/// let model: ModelId = "eliza-1966".parse().unwrap();
/// assert_eq!(model.as_str(), "eliza-1966");
/// assert_eq!(ModelId::default().as_str(), "eliza-1966");
/// assert!("   ".parse::<ModelId>().is_err());
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct ModelId(
    /// Validated provider-visible identifier.
    String,
);

impl ModelId {
    /// Return the provider-visible model id.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ModelId {
    fn default() -> Self {
        Self("eliza-1966".to_owned())
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<String> for ModelId {
    type Error = ModelError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err(ModelError::Empty)
        } else {
            Ok(Self(value))
        }
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.try_into().map_err(de::Error::custom)
    }
}

impl FromStr for ModelId {
    type Err = ModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}
