//! Validated JSON boundary values.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Arbitrary JSON whose outer value is guaranteed to be an object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct JsonObject(
    /// Validated object value hidden from provider route logic.
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    Map<String, Value>,
);

impl JsonObject {
    /// Wrap `value` when its outer JSON shape is an object.
    pub(crate) fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Object(fields) => Some(Self(fields)),
            _ => None,
        }
    }

    /// Serialize the validated object for provider string fields.
    pub(crate) fn serialized(&self) -> String {
        Value::Object(self.0.clone()).to_string()
    }
}

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    rlib::missing_section_dividers,
    reason = "two compact contract tests need no navigation ceremony"
)]
mod tests {
    use super::*;

    #[test]
    fn object_round_trips() {
        let object = serde_json::from_str::<JsonObject>(r#"{"value":"hello"}"#).unwrap();
        assert_eq!(object.serialized(), r#"{"value":"hello"}"#);
    }

    #[test]
    fn scalar_is_rejected() {
        assert!(serde_json::from_str::<JsonObject>("42").is_err());
    }
}
