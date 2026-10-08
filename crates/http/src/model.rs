//! Provider-visible model identifiers.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};

use super::errors::ModelError;

// -----------------------------------------------------------------------------
// ModelId: Validates provider-visible names.
// -----------------------------------------------------------------------------

/// Provider-visible model identifier accepted from CLI and JSON bodies.
///
/// Empty or whitespace-only model ids are rejected at the boundary.
///
/// ```
/// use eliza_http::model::ModelId;
///
/// let model: ModelId = "eliza-1966".parse().unwrap();
/// assert_eq!(model.as_str(), "eliza-1966");
/// assert!("   ".parse::<ModelId>().is_err());
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ModelId(
    /// Validated provider-visible identifier.
    String,
);

impl ModelId {
    /// Return the provider-visible model id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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

// -----------------------------------------------------------------------------
// ModelNames: Lists aliases for one local engine.
// -----------------------------------------------------------------------------

/// Ordered, unique aliases for one local engine; empty means its built-in name.
#[derive(Debug, Clone, Default)]
pub struct ModelNames(
    /// Validated names in their first occurrence order.
    Vec<ModelId>,
);

impl ModelNames {
    /// Retain the first occurrence of each configured name.
    #[must_use]
    pub fn new(names: Vec<ModelId>) -> Self {
        let mut unique = Vec::new();
        for name in names {
            if unique.contains(&name) {
                continue;
            }
            unique.push(name);
        }
        Self(unique)
    }

    /// Check a requested name without allocating.
    #[must_use]
    pub fn has_model(&self, model: &ModelId, builtin: &str) -> bool {
        self.has_name(model.as_str(), builtin)
    }

    /// Check a wire model name without allocating an owned identifier.
    #[must_use]
    pub fn has_name(&self, name: &str, builtin: &str) -> bool {
        if self.0.is_empty() {
            name == builtin
        } else {
            self.0.iter().any(|model| model.as_str() == name)
        }
    }

    /// Copy advertised IDs, falling back to the engine's built-in name.
    ///
    /// # Panics
    /// Panics if an invalid built-in name is supplied when no aliases exist.
    pub fn ids(&self, builtin: &str) -> impl Iterator<Item = ModelId> {
        self.0.iter().cloned().chain(
            self.0
                .is_empty()
                .then(|| builtin.parse().expect("built-in model IDs must be valid")),
        )
    }

    /// Describe accepted names for provider-native validation errors.
    #[must_use]
    pub fn expected(&self, builtin: &str) -> String {
        let names = self
            .0
            .iter()
            .map(ModelId::as_str)
            .chain(self.0.is_empty().then_some(builtin));
        let quoted = names.map(|name| format!("`{name}`")).collect::<Vec<_>>();
        quoted.join(" or ")
    }
}

// -----------------------------------------------------------------------------
// ModelAliases: Groups names by their local engine.
// -----------------------------------------------------------------------------

/// Configured model names, replacing only the supplied modality's defaults.
#[derive(Debug, Clone, Default)]
pub struct ModelAliases {
    /// Chat aliases; empty uses the route configuration's chat model.
    pub chat: ModelNames,
    /// Embedding aliases; empty uses the embedding engine's built-in name.
    pub embeddings: ModelNames,
    /// Image aliases; empty uses the image engine's built-in name.
    pub images: ModelNames,
    /// Speech aliases; empty uses the speech engine's built-in name.
    pub speech: ModelNames,
}

impl ModelAliases {
    /// Use built-in names for every modality, including const configurations.
    pub const EMPTY: Self = Self {
        chat: ModelNames(Vec::new()),
        embeddings: ModelNames(Vec::new()),
        images: ModelNames(Vec::new()),
        speech: ModelNames(Vec::new()),
    };
}
