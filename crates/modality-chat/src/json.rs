//! Validated JSON boundary values.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Arbitrary JSON whose outer value is guaranteed to be an object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct JsonObject(
    /// Validated object value hidden from provider route logic.
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    Map<String, Value>,
);

impl JsonObject {
    /// Consume the wrapper and return its validated object members.
    pub(super) fn into_map(self) -> Map<String, Value> {
        self.0
    }

    /// Serialize the validated object for provider string fields.
    #[must_use]
    pub fn serialized(&self) -> String {
        Value::Object(self.0.clone()).to_string()
    }
}
