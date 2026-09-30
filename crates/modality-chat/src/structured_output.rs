//! Bounded deterministic JSON Schema witness compilation.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::json::JsonObject;

// -----------------------------------------------------------------------------
// Schema: Defines bounded compilation policy.
// -----------------------------------------------------------------------------

/// Maximum semantic nesting accepted by the local compiler.
const SCHEMA_MAX_DEPTH: usize = 16;

/// Maximum number of distinct schema objects accepted in one request.
const SCHEMA_MAX_NODES: usize = 256;

/// JSON Schema keywords understood by the bounded compiler.
const SCHEMA_SUPPORTED_KEYWORDS: [&str; 14] = [
    "$defs",
    "$ref",
    "additionalProperties",
    "anyOf",
    "const",
    "default",
    "description",
    "enum",
    "items",
    "minItems",
    "properties",
    "required",
    "title",
    "type",
];

// -----------------------------------------------------------------------------
// StructuredOutputError: Classifies invalid and unsupported schemas.
// -----------------------------------------------------------------------------

/// Stable category used by provider adapters to select native errors.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StructuredOutputErrorKind {
    /// The supported schema subset is malformed or unsatisfiable.
    Invalid,
    /// The schema requests a keyword or shape outside the supported subset.
    Unsupported,
}

/// Failure raised while compiling a deterministic response template.
#[derive(Debug, Error)]
#[error("schema at JSON pointer `{path}`: {message}")]
pub struct StructuredOutputError {
    /// Invalid-versus-unsupported classification retained by adapters.
    kind: StructuredOutputErrorKind,
    /// RFC 6901 pointer to the offending schema location.
    path: String,
    /// Concise explanation safe to expose in provider error envelopes.
    message: String,
}

impl StructuredOutputError {
    /// Construct an invalid-schema error.
    fn invalid(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: StructuredOutputErrorKind::Invalid,
            path: path.into(),
            message: message.into(),
        }
    }

    /// Construct an unsupported-schema error.
    fn unsupported(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: StructuredOutputErrorKind::Unsupported,
            path: path.into(),
            message: message.into(),
        }
    }

    /// Return the provider-neutral error category.
    #[must_use]
    pub const fn kind(&self) -> StructuredOutputErrorKind {
        self.kind
    }
}

// -----------------------------------------------------------------------------
// StructuredOutput: Applies one immutable text-response template.
// -----------------------------------------------------------------------------

/// Provider-neutral response formatting compiled before execution.
#[derive(Debug, Default)]
pub struct StructuredOutput {
    /// Immutable JSON template, absent for ordinary text.
    template: Option<Template>,
    /// Serialized schema characters included in request accounting.
    schema_chars: usize,
}

impl StructuredOutput {
    /// Compile schema-less JSON-object mode.
    #[must_use]
    pub fn json_object() -> Self {
        Self {
            template: Some(Template::Object(BTreeMap::from([(
                "response".to_owned(),
                Template::ResponseText,
            )]))),
            schema_chars: 0,
        }
    }

    /// Count serialized schema characters for limits and prompt usage.
    pub(super) const fn schema_chars(&self) -> usize {
        self.schema_chars
    }

    /// Apply this format to final text without touching tool calls.
    pub(super) fn render(&self, text: String) -> String {
        // Preserve unformatted text when the request selected no response format.
        let Some(template) = &self.template else {
            return text;
        };
        template.value(&text).to_string()
    }
}

impl TryFrom<JsonObject> for StructuredOutput {
    type Error = StructuredOutputError;

    /// Compile a bounded JSON Schema into a deterministic witness template.
    ///
    /// # Errors
    ///
    /// Returns a classified schema error when the input is malformed,
    /// unsupported, or cannot produce a deterministic witness.
    fn try_from(schema: JsonObject) -> Result<Self, Self::Error> {
        let schema_chars = schema.serialized().chars().count();
        let template = Compiler::compile(schema.into_map())?;
        Ok(Self {
            template: Some(template),
            schema_chars,
        })
    }
}

// -----------------------------------------------------------------------------
// Template: Retains only data needed to render one response.
// -----------------------------------------------------------------------------

/// Immutable witness with a placeholder for final ELIZA text.
#[derive(Debug, Clone, PartialEq)]
enum Template {
    /// Array witness items.
    Array(
        /// Ordered item templates.
        Vec<Self>,
    ),
    /// Constant primitive witness.
    Literal(
        /// Primitive value emitted unchanged.
        Value,
    ),
    /// Required object properties.
    Object(
        /// Deterministically ordered property templates.
        BTreeMap<String, Self>,
    ),
    /// Final ELIZA text inserted at execution time.
    ResponseText,
}

impl Template {
    /// Materialize this template as a JSON value.
    fn value(&self, response: &str) -> Value {
        match self {
            Self::Array(items) => {
                Value::Array(items.iter().map(|item| item.value(response)).collect())
            }
            Self::Literal(value) => value.clone(),
            Self::Object(properties) => Value::Object(
                properties
                    .iter()
                    .map(|(name, value)| (name.clone(), value.value(response)))
                    .collect::<Map<_, _>>(),
            ),
            Self::ResponseText => Value::String(response.to_owned()),
        }
    }
}

// -----------------------------------------------------------------------------
// JsonType: Classifies supported primitive schema types.
// -----------------------------------------------------------------------------

/// One supported JSON primitive type.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum JsonType {
    /// JSON array.
    Array,
    /// JSON Boolean.
    Boolean,
    /// Integral JSON number.
    Integer,
    /// JSON null.
    Null,
    /// Any JSON number.
    Number,
    /// JSON object.
    Object,
    /// JSON string.
    String,
}

impl JsonType {
    /// Parse a single supported JSON Schema type name.
    #[expect(
        clippy::missing_errors_doc,
        reason = "this private parser reports its complete failure contract at each match arm"
    )]
    fn parse(value: &str, path: &str) -> Result<Self, StructuredOutputError> {
        match value {
            "array" => Ok(Self::Array),
            "boolean" => Ok(Self::Boolean),
            "integer" => Ok(Self::Integer),
            "null" => Ok(Self::Null),
            "number" => Ok(Self::Number),
            "object" => Ok(Self::Object),
            "string" => Ok(Self::String),
            _ => Err(StructuredOutputError::unsupported(
                path,
                format!("unsupported type `{value}`"),
            )),
        }
    }

    /// Report whether a primitive value belongs to this JSON type.
    fn is_compatible_with(self, value: &Value) -> bool {
        match self {
            Self::Array => value.is_array(),
            Self::Boolean => value.is_boolean(),
            Self::Integer => value
                .as_number()
                .is_some_and(|number| number.is_i64() || number.is_u64()),
            Self::Null => value.is_null(),
            Self::Number => value.is_number(),
            Self::Object => value.is_object(),
            Self::String => value.is_string(),
        }
    }
}

// -----------------------------------------------------------------------------
// SchemaNode: Stores the supported assertions in one physical schema object.
// -----------------------------------------------------------------------------

/// Parsed assertions and graph edges for one unique schema node.
#[derive(Debug, Default)]
struct SchemaNode {
    /// Ordered alternatives for an `anyOf` assertion.
    any_of: Vec<usize>,
    /// Primitive constant, when present.
    constant: Option<Value>,
    /// Primitive enumeration values in request order.
    enumeration: Option<Vec<Value>>,
    /// Explicit single type assertion.
    explicit_type: Option<JsonType>,
    /// Array item schema.
    items: Option<usize>,
    /// Minimum array length, limited to zero or one.
    min_items: usize,
    /// Whether array-only assertions were supplied.
    has_array_assertions: bool,
    /// Whether object-only assertions were supplied.
    has_object_assertions: bool,
    /// Whether this physical node is nested inside an `anyOf` branch.
    is_optional_branch: bool,
    /// Canonical RFC 6901 pointer for diagnostics and reference lookup.
    path: String,
    /// Property schemas keyed deterministically.
    properties: BTreeMap<String, usize>,
    /// Resolved local reference target.
    reference: Option<usize>,
    /// Unresolved local reference spelling.
    reference_text: Option<String>,
    /// Required property names.
    required: Vec<String>,
}

impl SchemaNode {
    /// Return the type implied by local structural assertions.
    fn local_type(&self) -> Option<JsonType> {
        self.explicit_type.or({
            if self.has_object_assertions {
                Some(JsonType::Object)
            } else if self.has_array_assertions {
                Some(JsonType::Array)
            } else {
                None
            }
        })
    }
}

// -----------------------------------------------------------------------------
// NodeRole: Captures the three meaningful schema positions.
// -----------------------------------------------------------------------------

/// Position-dependent parsing policy for one schema node.
#[derive(Debug, Clone, Copy)]
enum NodeRole {
    /// One alternative may be unsatisfiable when another branch is valid.
    AnyOfBranch,
    /// An ordinary nested schema must be independently satisfiable.
    Nested,
    /// The request's top-level schema may own definitions.
    Root,
}

impl NodeRole {
    /// Report whether this node may fail while another alternative succeeds.
    const fn is_optional_branch(self) -> bool {
        matches!(self, Self::AnyOfBranch)
    }

    /// Report whether this is the request's top-level schema.
    const fn is_root(self) -> bool {
        matches!(self, Self::Root)
    }
}

// -----------------------------------------------------------------------------
// Clause: Tracks pending expansion for one conjunct.
// -----------------------------------------------------------------------------

/// One schema node participating in a logical conjunction.
#[derive(Debug, Clone, Copy)]
struct Clause {
    /// Whether this node's `anyOf` still needs expansion.
    should_expand_any_of: bool,
    /// Whether this node's local reference still needs expansion.
    should_expand_reference: bool,
    /// Arena node id.
    id: usize,
}

impl Clause {
    /// Start with every compound assertion pending expansion.
    const fn new(id: usize) -> Self {
        Self {
            should_expand_any_of: true,
            should_expand_reference: true,
            id,
        }
    }
}

// -----------------------------------------------------------------------------
// VisitState: Records depth-first graph traversal state.
// -----------------------------------------------------------------------------

/// Depth-first graph visitation state.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum VisitState {
    /// The node and all descendants have been validated.
    Done,
    /// The node has not been visited.
    New,
    /// The node is on the active traversal stack.
    Visiting,
}

// -----------------------------------------------------------------------------
// Compiler: Parses, validates, and solves one bounded schema graph.
// -----------------------------------------------------------------------------

/// Arena-backed compiler for a single request schema.
#[derive(Debug, Default)]
struct Compiler {
    /// Physical schema nodes in deterministic preorder.
    nodes: Vec<SchemaNode>,
    /// Canonical JSON pointers mapped to arena nodes.
    pointers: BTreeMap<String, usize>,
}

#[expect(
    clippy::missing_errors_doc,
    reason = "private compiler phases share the documented public compilation contract"
)]
#[expect(
    rlib::misordered_inherent_impl_items,
    reason = "the compiler entry point precedes its private parse-and-solve phases"
)]
#[expect(
    rlib::undocumented_early_returns,
    reason = "schema guard branches return diagnostics that state each rejection policy"
)]
impl Compiler {
    /// Compile one root object through every validation phase.
    fn compile(root: Map<String, Value>) -> Result<Template, StructuredOutputError> {
        let mut compiler = Self::default();
        let root = compiler.parse_node(Value::Object(root), String::new(), 1, NodeRole::Root)?;
        compiler.resolve_references()?;
        compiler.validate_graph(root)?;
        compiler.validate_unused_nodes()?;
        compiler.solve(&[Clause::new(root)])
    }

    /// Parse one schema object and all physical child schemas.
    #[expect(
        rlib::missing_code_phase_comments,
        reason = "normalized assertion fields are clearest as contiguous declarative groups"
    )]
    fn parse_node(
        &mut self,
        value: Value,
        path: String,
        depth: usize,
        role: NodeRole,
    ) -> Result<usize, StructuredOutputError> {
        // Bound work before accepting or traversing the schema object.
        if depth > SCHEMA_MAX_DEPTH {
            return Err(StructuredOutputError::unsupported(
                path,
                format!("semantic depth exceeds {SCHEMA_MAX_DEPTH}"),
            ));
        }
        if self.nodes.len() == SCHEMA_MAX_NODES {
            return Err(StructuredOutputError::unsupported(
                path,
                format!("schema contains more than {SCHEMA_MAX_NODES} unique nodes"),
            ));
        }
        let Value::Object(mut object) = value else {
            return Err(StructuredOutputError::invalid(
                path,
                "schema must be an object",
            ));
        };

        // Register the node before parsing children so local references can target it.
        Self::reject_unknown_keywords(&object, &path)?;
        let id = self.nodes.len();
        self.pointers.insert(path.clone(), id);
        self.nodes.push(SchemaNode {
            is_optional_branch: role.is_optional_branch(),
            path: path.clone(),
            ..SchemaNode::default()
        });

        // Consume each supported assertion into its normalized representation.
        let explicit_type = Self::parse_type(object.remove("type"), &path)?;
        let constant = Self::parse_constant(object.remove("const"), &path)?;
        let enumeration = Self::parse_enumeration(object.remove("enum"), &path)?;
        let any_of = self.parse_any_of(object.remove("anyOf"), &path, depth)?;
        let properties = self.parse_properties(object.remove("properties"), &path, depth)?;
        let required = Self::parse_required(object.remove("required"), &path)?;
        let has_closed_object =
            Self::is_closed_object(object.remove("additionalProperties"), &path)?;
        let items = self.parse_items(object.remove("items"), &path, depth)?;
        let has_min_items = object.contains_key("minItems");
        let min_items = Self::parse_min_items(object.remove("minItems"), &path)?;
        self.parse_definitions(object.remove("$defs"), &path, depth, role)?;
        let reference_text = Self::parse_reference(object.remove("$ref"), &path)?;
        Self::validate_annotations(&object, &path)?;

        // Validate and commit the completed node without exposing partial state.
        let has_object_assertions =
            !properties.is_empty() || !required.is_empty() || has_closed_object;
        let has_array_assertions = items.is_some() || has_min_items;
        let node = SchemaNode {
            any_of,
            constant,
            enumeration,
            explicit_type,
            items,
            min_items,
            has_array_assertions,
            has_object_assertions,
            is_optional_branch: role.is_optional_branch(),
            path,
            properties,
            reference: None,
            reference_text,
            required,
        };
        Self::validate_local_assertions(&node)?;
        self.nodes[id] = node;
        Ok(id)
    }

    /// Reject every keyword outside the deliberately small supported subset.
    fn reject_unknown_keywords(
        object: &Map<String, Value>,
        path: &str,
    ) -> Result<(), StructuredOutputError> {
        for keyword in object.keys() {
            if !SCHEMA_SUPPORTED_KEYWORDS.contains(&keyword.as_str()) {
                return Err(StructuredOutputError::unsupported(
                    pointer_join(path, keyword),
                    format!("unsupported keyword `{keyword}`"),
                ));
            }
        }
        Ok(())
    }

    /// Parse an optional single primitive `type` value.
    fn parse_type(
        value: Option<Value>,
        path: &str,
    ) -> Result<Option<JsonType>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let keyword_path = pointer_join(path, "type");
        match value {
            Value::String(value) => JsonType::parse(&value, &keyword_path).map(Some),
            Value::Array(_) => Err(StructuredOutputError::unsupported(
                keyword_path,
                "type arrays are not supported",
            )),
            _ => Err(StructuredOutputError::invalid(
                keyword_path,
                "type must be a string",
            )),
        }
    }

    /// Parse an optional primitive constant.
    fn parse_constant(
        value: Option<Value>,
        path: &str,
    ) -> Result<Option<Value>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if is_primitive(&value) {
            Ok(Some(value))
        } else {
            Err(StructuredOutputError::unsupported(
                pointer_join(path, "const"),
                "complex const values are not supported",
            ))
        }
    }

    /// Parse an optional nonempty primitive enumeration.
    fn parse_enumeration(
        value: Option<Value>,
        path: &str,
    ) -> Result<Option<Vec<Value>>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let keyword_path = pointer_join(path, "enum");
        let Value::Array(values) = value else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "enum must be an array",
            ));
        };
        if values.is_empty() {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "enum must not be empty",
            ));
        }
        if values.iter().any(|value| !is_primitive(value)) {
            return Err(StructuredOutputError::unsupported(
                keyword_path,
                "complex enum values are not supported",
            ));
        }
        Ok(Some(values))
    }

    /// Parse a nonempty array of child schemas.
    fn parse_any_of(
        &mut self,
        value: Option<Value>,
        path: &str,
        depth: usize,
    ) -> Result<Vec<usize>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        let keyword_path = pointer_join(path, "anyOf");
        let Value::Array(values) = value else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "anyOf must be an array",
            ));
        };
        if values.is_empty() {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "anyOf must not be empty",
            ));
        }
        let indexed_values = values.into_iter().enumerate();
        indexed_values
            .map(|(index, schema)| {
                self.parse_node(
                    schema,
                    pointer_join(&keyword_path, &index.to_string()),
                    depth + 1,
                    NodeRole::AnyOfBranch,
                )
            })
            .collect()
    }

    /// Parse object property schemas.
    fn parse_properties(
        &mut self,
        value: Option<Value>,
        path: &str,
        depth: usize,
    ) -> Result<BTreeMap<String, usize>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(BTreeMap::new());
        };
        let keyword_path = pointer_join(path, "properties");
        let Value::Object(properties) = value else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "properties must be an object",
            ));
        };
        properties
            .into_iter()
            .map(|(name, schema)| {
                let id = self.parse_node(
                    schema,
                    pointer_join(&keyword_path, &name),
                    depth + 1,
                    NodeRole::Nested,
                )?;
                Ok((name, id))
            })
            .collect()
    }

    /// Parse an optional required-property list.
    fn parse_required(
        value: Option<Value>,
        path: &str,
    ) -> Result<Vec<String>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        let keyword_path = pointer_join(path, "required");
        let Value::Array(values) = value else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "required must be an array",
            ));
        };
        let mut names = Vec::with_capacity(values.len());
        let mut unique = BTreeSet::new();
        for (index, value) in values.into_iter().enumerate() {
            let Value::String(name) = value else {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&keyword_path, &index.to_string()),
                    "required entries must be strings",
                ));
            };
            if !unique.insert(name.clone()) {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&keyword_path, &index.to_string()),
                    format!("duplicate required property `{name}`"),
                ));
            }
            names.push(name);
        }
        Ok(names)
    }

    /// Parse the sole supported `additionalProperties` value.
    fn is_closed_object(value: Option<Value>, path: &str) -> Result<bool, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(false);
        };
        let keyword_path = pointer_join(path, "additionalProperties");
        match value {
            Value::Bool(false) => Ok(true),
            Value::Bool(true) | Value::Object(_) => Err(StructuredOutputError::unsupported(
                keyword_path,
                "only additionalProperties: false is supported",
            )),
            _ => Err(StructuredOutputError::invalid(
                keyword_path,
                "additionalProperties must be false",
            )),
        }
    }

    /// Parse one optional child schema.
    fn parse_items(
        &mut self,
        value: Option<Value>,
        path: &str,
        depth: usize,
    ) -> Result<Option<usize>, StructuredOutputError> {
        value
            .map(|schema| {
                self.parse_node(
                    schema,
                    pointer_join(path, "items"),
                    depth + 1,
                    NodeRole::Nested,
                )
            })
            .transpose()
    }

    /// Parse the supported `minItems` values zero and one.
    fn parse_min_items(value: Option<Value>, path: &str) -> Result<usize, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(0);
        };
        let keyword_path = pointer_join(path, "minItems");
        let Some(value) = value.as_u64() else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "minItems must be a nonnegative integer",
            ));
        };
        match value {
            0 => Ok(0),
            1 => Ok(1),
            _ => Err(StructuredOutputError::unsupported(
                keyword_path,
                "minItems values above 1 are not supported",
            )),
        }
    }

    /// Parse root definitions and reject nested definition containers.
    fn parse_definitions(
        &mut self,
        value: Option<Value>,
        path: &str,
        depth: usize,
        role: NodeRole,
    ) -> Result<(), StructuredOutputError> {
        let Some(value) = value else {
            return Ok(());
        };
        let keyword_path = pointer_join(path, "$defs");
        if !role.is_root() {
            return Err(StructuredOutputError::unsupported(
                keyword_path,
                "$defs is supported only at the schema root",
            ));
        }
        let Value::Object(definitions) = value else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "$defs must be an object",
            ));
        };
        for (name, schema) in definitions {
            self.parse_node(
                schema,
                pointer_join(&keyword_path, &name),
                depth + 1,
                NodeRole::Nested,
            )?;
        }
        Ok(())
    }

    /// Parse an optional local-reference string.
    fn parse_reference(
        value: Option<Value>,
        path: &str,
    ) -> Result<Option<String>, StructuredOutputError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let keyword_path = pointer_join(path, "$ref");
        let Value::String(reference) = value else {
            return Err(StructuredOutputError::invalid(
                keyword_path,
                "$ref must be a string",
            ));
        };
        Ok(Some(reference))
    }

    /// Validate recognized annotations while intentionally ignoring their data.
    fn validate_annotations(
        object: &Map<String, Value>,
        path: &str,
    ) -> Result<(), StructuredOutputError> {
        for keyword in ["title", "description"] {
            if object.get(keyword).is_some_and(|value| !value.is_string()) {
                return Err(StructuredOutputError::invalid(
                    pointer_join(path, keyword),
                    format!("{keyword} must be a string"),
                ));
            }
        }
        Ok(())
    }

    /// Reject locally incompatible supported assertions.
    fn validate_local_assertions(node: &SchemaNode) -> Result<(), StructuredOutputError> {
        if node.has_object_assertions && node.has_array_assertions {
            return Err(StructuredOutputError::invalid(
                &node.path,
                "object and array assertions cannot be combined",
            ));
        }
        if let Some(explicit) = node.explicit_type {
            if node.has_object_assertions && explicit != JsonType::Object {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&node.path, "type"),
                    "type is incompatible with object assertions",
                ));
            }
            if node.has_array_assertions && explicit != JsonType::Array {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&node.path, "type"),
                    "type is incompatible with array assertions",
                ));
            }
        }
        for name in &node.required {
            if !node.properties.contains_key(name) {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&node.path, "required"),
                    format!("required property `{name}` is absent from properties"),
                ));
            }
        }
        if node.min_items == 1 && node.items.is_none() {
            return Err(StructuredOutputError::invalid(
                pointer_join(&node.path, "items"),
                "items is required when minItems is 1",
            ));
        }
        if let Some(kind) = node.local_type() {
            if node
                .constant
                .as_ref()
                .is_some_and(|value| !kind.is_compatible_with(value))
            {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&node.path, "const"),
                    "const is incompatible with type",
                ));
            }
            if node
                .enumeration
                .as_ref()
                .is_some_and(|values| values.iter().all(|value| !kind.is_compatible_with(value)))
            {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&node.path, "enum"),
                    "enum has no value compatible with type",
                ));
            }
        }
        if let (Some(constant), Some(values)) = (&node.constant, &node.enumeration)
            && !values.contains(constant)
        {
            return Err(StructuredOutputError::invalid(
                pointer_join(&node.path, "const"),
                "const is absent from enum",
            ));
        }
        Ok(())
    }

    /// Resolve every reference only after all physical nodes are registered.
    fn resolve_references(&mut self) -> Result<(), StructuredOutputError> {
        for id in 0..self.nodes.len() {
            let Some(reference) = self.nodes[id].reference_text.clone() else {
                continue;
            };
            let path = pointer_join(&self.nodes[id].path, "$ref");
            let pointer = local_reference_pointer(&reference, &path)?;
            let Some(target) = self.pointers.get(&pointer).copied() else {
                return Err(StructuredOutputError::invalid(
                    path,
                    format!("local reference `{reference}` does not name a schema"),
                ));
            };
            self.nodes[id].reference = Some(target);
        }
        Ok(())
    }

    /// Reject reference cycles and semantic paths beyond the depth limit.
    fn validate_graph(&self, root: usize) -> Result<(), StructuredOutputError> {
        let mut states = vec![VisitState::New; self.nodes.len()];
        let mut depths = vec![0; self.nodes.len()];
        self.graph_depth(root, &mut states, &mut depths)?;
        for id in 0..self.nodes.len() {
            if states[id] != VisitState::New {
                continue;
            }
            self.graph_depth(id, &mut states, &mut depths)?;
        }
        Ok(())
    }

    /// Return the longest semantic path beginning at one node.
    fn graph_depth(
        &self,
        id: usize,
        states: &mut [VisitState],
        depths: &mut [usize],
    ) -> Result<usize, StructuredOutputError> {
        match states[id] {
            VisitState::Done => return Ok(depths[id]),
            VisitState::Visiting => {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&self.nodes[id].path, "$ref"),
                    "recursive references are not supported",
                ));
            }
            VisitState::New => {}
        }
        states[id] = VisitState::Visiting;
        let node = &self.nodes[id];
        let mut maximum = 1;
        let branches_and_properties = node.any_of.iter().chain(node.properties.values());
        let items_and_reference = node.items.iter().chain(node.reference.iter());
        let children = branches_and_properties.chain(items_and_reference);
        for child in children {
            let depth = 1 + self.graph_depth(*child, states, depths)?;
            if depth > SCHEMA_MAX_DEPTH {
                return Err(StructuredOutputError::unsupported(
                    &self.nodes[*child].path,
                    format!("semantic depth exceeds {SCHEMA_MAX_DEPTH}"),
                ));
            }
            maximum = maximum.max(depth);
        }
        states[id] = VisitState::Done;
        depths[id] = maximum;
        Ok(maximum)
    }

    /// Prove that unused property and definition schemas are executable too.
    fn validate_unused_nodes(&self) -> Result<(), StructuredOutputError> {
        for (id, node) in self.nodes.iter().enumerate() {
            if node.is_optional_branch {
                continue;
            }
            self.solve(&[Clause::new(id)])?;
        }
        Ok(())
    }

    /// Solve one conjunction, selecting `anyOf` branches in request order.
    #[expect(
        clippy::manual_ok_err,
        reason = "the explicit match documents deliberate branch-error suppression required by rlib"
    )]
    fn solve(&self, clauses: &[Clause]) -> Result<Template, StructuredOutputError> {
        if let Some((index, target)) = clauses.iter().enumerate().find_map(|(index, clause)| {
            clause
                .should_expand_reference
                .then_some(self.nodes[clause.id].reference)
                .flatten()
                .map(|target| (index, target))
        }) {
            let mut expanded = clauses.to_vec();
            expanded[index].should_expand_reference = false;
            expanded.push(Clause::new(target));
            return self.solve(&expanded);
        }

        if let Some((index, branches)) = clauses.iter().enumerate().find_map(|(index, clause)| {
            let branches = &self.nodes[clause.id].any_of;
            (clause.should_expand_any_of && !branches.is_empty()).then_some((index, branches))
        }) {
            let template = branches.iter().find_map(|branch| {
                let mut expanded = clauses.to_vec();
                expanded[index].should_expand_any_of = false;
                expanded.push(Clause::new(*branch));
                match self.solve(&expanded) {
                    Ok(template) => Some(template),
                    Err(_) => None,
                }
            });
            if let Some(template) = template {
                return Ok(template);
            }
            return Err(StructuredOutputError::invalid(
                pointer_join(&self.nodes[clauses[index].id].path, "anyOf"),
                "anyOf has no satisfiable branch",
            ));
        }

        self.solve_assertions(clauses)
    }

    /// Construct a witness after references and alternatives are expanded.
    #[expect(
        rlib::missing_code_phase_comments,
        reason = "the exhaustive witness match is clearer kept as one complete decision table"
    )]
    fn solve_assertions(&self, clauses: &[Clause]) -> Result<Template, StructuredOutputError> {
        // Prefer explicit primitive witnesses before synthesizing a type default.
        let kind = self.combined_type(clauses)?;
        if let Some(constant) = self.combined_constant(clauses, kind)? {
            return Ok(Template::Literal(constant));
        }
        if let Some(value) = self.combined_enum(clauses, kind)? {
            return Ok(Template::Literal(value));
        }

        // Synthesize the smallest deterministic witness for the resolved type.
        match kind.unwrap_or(JsonType::String) {
            JsonType::Array => self.solve_array(clauses),
            JsonType::Boolean => Ok(Template::Literal(Value::Bool(false))),
            JsonType::Integer | JsonType::Number => {
                Ok(Template::Literal(Value::Number(Number::from(0))))
            }
            JsonType::Null => Ok(Template::Literal(Value::Null)),
            JsonType::Object => self.solve_object(clauses),
            JsonType::String => Ok(Template::ResponseText),
        }
    }

    /// Reconcile explicit and structurally implied types.
    fn combined_type(&self, clauses: &[Clause]) -> Result<Option<JsonType>, StructuredOutputError> {
        let mut combined = None;
        for clause in clauses {
            let node = &self.nodes[clause.id];
            let Some(candidate) = node.local_type() else {
                continue;
            };
            if combined.is_some_and(|kind| kind != candidate) {
                return Err(StructuredOutputError::invalid(
                    &node.path,
                    "combined type assertions are unsatisfiable",
                ));
            }
            combined = Some(candidate);
        }
        Ok(combined)
    }

    /// Select a compatible constant before all other witnesses.
    fn combined_constant(
        &self,
        clauses: &[Clause],
        kind: Option<JsonType>,
    ) -> Result<Option<Value>, StructuredOutputError> {
        let mut selected: Option<&Value> = None;
        for clause in clauses {
            let node = &self.nodes[clause.id];
            let Some(constant) = &node.constant else {
                continue;
            };
            if selected.is_some_and(|value| value != constant) {
                return Err(StructuredOutputError::invalid(
                    pointer_join(&node.path, "const"),
                    "combined const assertions are unsatisfiable",
                ));
            }
            selected = Some(constant);
        }
        let Some(selected) = selected else {
            return Ok(None);
        };
        if !self.is_value_compatible(selected, clauses, kind) {
            return Err(StructuredOutputError::invalid(
                &self.nodes[clauses[0].id].path,
                "const is incompatible with combined assertions",
            ));
        }
        Ok(Some(selected.clone()))
    }

    /// Select the first primitive value compatible with every enumeration.
    fn combined_enum(
        &self,
        clauses: &[Clause],
        kind: Option<JsonType>,
    ) -> Result<Option<Value>, StructuredOutputError> {
        let enumerations = clauses.iter().find_map(|clause| {
            let values = self.nodes[clause.id].enumeration.as_ref()?;
            Some((clause.id, values))
        });
        let Some((owner, values)) = enumerations else {
            return Ok(None);
        };
        let compatible = values
            .iter()
            .find(|value| self.is_value_compatible(value, clauses, kind));
        compatible.cloned().map(Some).ok_or_else(|| {
            StructuredOutputError::invalid(
                pointer_join(&self.nodes[owner].path, "enum"),
                "combined enum assertions are unsatisfiable",
            )
        })
    }

    /// Check a primitive candidate against type and enum assertions.
    fn is_value_compatible(
        &self,
        value: &Value,
        clauses: &[Clause],
        kind: Option<JsonType>,
    ) -> bool {
        kind.is_none_or(|kind| kind.is_compatible_with(value))
            && clauses.iter().all(|clause| {
                self.nodes[clause.id]
                    .enumeration
                    .as_ref()
                    .is_none_or(|values| values.contains(value))
            })
    }

    /// Emit one item only when a combined `minItems` assertion requires it.
    fn solve_array(&self, clauses: &[Clause]) -> Result<Template, StructuredOutputError> {
        let minimums = clauses.iter().map(|clause| self.nodes[clause.id].min_items);
        let minimum = minimums.max().unwrap_or(0);
        if minimum == 0 {
            return Ok(Template::Array(Vec::new()));
        }
        let item_ids = clauses
            .iter()
            .filter_map(|clause| self.nodes[clause.id].items);
        let items = item_ids.map(Clause::new).collect::<Vec<_>>();
        if items.is_empty() {
            return Err(StructuredOutputError::invalid(
                &self.nodes[clauses[0].id].path,
                "array item constraints are missing",
            ));
        }
        Ok(Template::Array(vec![self.solve(&items)?]))
    }

    /// Emit every required property and omit every optional property.
    fn solve_object(&self, clauses: &[Clause]) -> Result<Template, StructuredOutputError> {
        let required_names = clauses
            .iter()
            .flat_map(|clause| self.nodes[clause.id].required.iter().cloned());
        let required = required_names.collect::<BTreeSet<_>>();
        let mut properties = BTreeMap::new();
        for name in required {
            let schema_ids = clauses
                .iter()
                .filter_map(|clause| self.nodes[clause.id].properties.get(&name).copied());
            let schemas = schema_ids.map(Clause::new).collect::<Vec<_>>();
            if schemas.is_empty() {
                return Err(StructuredOutputError::invalid(
                    &self.nodes[clauses[0].id].path,
                    format!("required property `{name}` has no schema"),
                ));
            }
            properties.insert(name, self.solve(&schemas)?);
        }
        Ok(Template::Object(properties))
    }
}

// -----------------------------------------------------------------------------
// Tests: Pin witness selection, bounds, and rejection behavior.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions are the intended panic contract"
)]
#[expect(
    clippy::items_after_test_module,
    reason = "dylint dependency ordering places test-only wildcard consumers before private helpers"
)]
#[expect(
    rlib::missing_section_dividers,
    reason = "scenario-named tests are clearer than one divider per independent assertion"
)]
mod tests {
    use super::*;

    /// Compile and render one schema fixture.
    fn schema_render_fixture(schema: Value, response: &str) -> String {
        let schema = serde_json::from_value::<JsonObject>(schema).unwrap();
        StructuredOutput::try_from(schema)
            .unwrap()
            .render(response.to_owned())
    }

    /// Compile one expected schema failure.
    #[expect(
        rlib::ad_hoc_conversions,
        reason = "the test helper deliberately unwraps JSON before returning the expected error"
    )]
    fn schema_compile_error(schema: Value) -> StructuredOutputError {
        let schema = serde_json::from_value::<JsonObject>(schema).unwrap();
        StructuredOutput::try_from(schema).unwrap_err()
    }

    #[test]
    fn schema_json_object_mode_escapes_final_text() {
        let rendered = StructuredOutput::json_object().render("a \"quote\"\n".to_owned());
        assert_eq!(rendered, r#"{"response":"a \"quote\"\n"}"#);
    }

    #[test]
    fn schema_primitive_witnesses_are_deterministic() {
        assert_eq!(
            schema_render_fixture(serde_json::json!({"type":"string"}), "hello"),
            r#""hello""#
        );
        assert_eq!(
            schema_render_fixture(serde_json::json!({"type":"number"}), "hello"),
            "0"
        );
        assert_eq!(
            schema_render_fixture(serde_json::json!({"type":"integer"}), "hello"),
            "0"
        );
        assert_eq!(
            schema_render_fixture(serde_json::json!({"type":"boolean"}), "hello"),
            "false"
        );
        assert_eq!(
            schema_render_fixture(serde_json::json!({"type":"null"}), "hello"),
            "null"
        );
    }

    #[test]
    fn schema_const_precedes_the_first_compatible_enum_value() {
        let schema = serde_json::json!({
            "type":"string",
            "const":"fixed",
            "enum":["first","fixed"]
        });
        assert_eq!(schema_render_fixture(schema, "hello"), r#""fixed""#);

        let schema = serde_json::json!({"type":"number","enum":["wrong",2,3]});
        assert_eq!(schema_render_fixture(schema, "hello"), "2");
    }

    #[test]
    fn schema_objects_emit_required_properties_only() {
        let schema = serde_json::json!({
            "type":"object",
            "properties":{
                "optional":{"type":"boolean"},
                "answer":{"type":"string"},
                "count":{"type":"integer"}
            },
            "required":["count","answer"],
            "additionalProperties":false
        });
        assert_eq!(
            schema_render_fixture(schema, "hello"),
            r#"{"answer":"hello","count":0}"#
        );
    }

    #[test]
    fn schema_arrays_follow_min_items() {
        let empty = serde_json::json!({"type":"array","items":{"type":"string"}});
        let one = serde_json::json!({
            "type":"array",
            "items":{"type":"string"},
            "minItems":1
        });
        assert_eq!(schema_render_fixture(empty, "hello"), "[]");
        assert_eq!(schema_render_fixture(one, "hello"), r#"["hello"]"#);
    }

    #[test]
    fn schema_any_of_selects_the_first_satisfiable_branch() {
        let schema = serde_json::json!({
            "type":"string",
            "anyOf":[{"type":"number"},{"const":"chosen"},{"const":"later"}]
        });
        assert_eq!(schema_render_fixture(schema, "hello"), r#""chosen""#);
    }

    #[test]
    fn schema_local_references_decode_rfc_6901_tokens() {
        let schema = serde_json::json!({
            "$defs":{"a/b~c":{"type":"string"}},
            "$ref":"#/$defs/a~1b~0c"
        });
        assert_eq!(schema_render_fixture(schema, "hello"), r#""hello""#);

        let schema = serde_json::json!({
            "type":"string",
            "$defs":{"rootAlias":{"$ref":"#"}}
        });
        assert_eq!(schema_render_fixture(schema, "hello"), r#""hello""#);
    }

    #[test]
    fn schema_validates_unused_properties_and_definitions() {
        for schema in [
            serde_json::json!({
                "type":"object",
                "properties":{"unused":{"pattern":"x"}}
            }),
            serde_json::json!({
                "type":"string",
                "$defs":{"unused":{"format":"date"}}
            }),
        ] {
            assert_eq!(
                schema_compile_error(schema).kind(),
                StructuredOutputErrorKind::Unsupported
            );
        }
    }

    #[test]
    fn schema_rejects_malformed_and_incompatible_assertions() {
        for schema in [
            serde_json::json!({"type":1}),
            serde_json::json!({"type":"object","required":["missing"]}),
            serde_json::json!({"type":"array","minItems":1}),
            serde_json::json!({"type":"string","const":1}),
            serde_json::json!({"type":"boolean","enum":["no"]}),
        ] {
            assert_eq!(
                schema_compile_error(schema).kind(),
                StructuredOutputErrorKind::Invalid
            );
        }
    }

    #[test]
    fn schema_rejects_unsupported_schema_features() {
        for schema in [
            serde_json::json!({"type":["string","null"]}),
            serde_json::json!({"oneOf":[{"type":"string"}]}),
            serde_json::json!({"allOf":[{"type":"string"}]}),
            serde_json::json!({"type":"string","pattern":"x"}),
            serde_json::json!({"type":"number","minimum":1}),
            serde_json::json!({"type":"array","maxItems":1}),
            serde_json::json!({"const":[]}),
        ] {
            assert_eq!(
                schema_compile_error(schema).kind(),
                StructuredOutputErrorKind::Unsupported
            );
        }
    }

    #[test]
    fn schema_rejects_missing_external_and_recursive_references() {
        let missing = schema_compile_error(serde_json::json!({"$ref":"#/$defs/missing"}));
        assert_eq!(missing.kind(), StructuredOutputErrorKind::Invalid);
        let external = schema_compile_error(serde_json::json!({"$ref":"other.json"}));
        assert_eq!(external.kind(), StructuredOutputErrorKind::Unsupported);
        let recursive = schema_compile_error(serde_json::json!({"$ref":"#"}));
        assert_eq!(recursive.kind(), StructuredOutputErrorKind::Invalid);
    }

    #[test]
    fn schema_accepts_exactly_sixteen_semantic_levels() {
        let mut schema = serde_json::json!({"type":"string"});
        for _ in 1..SCHEMA_MAX_DEPTH {
            schema = serde_json::json!({"type":"array","items":schema,"minItems":1});
        }
        assert!(schema_render_fixture(schema.clone(), "hello").contains("hello"));
        schema = serde_json::json!({"type":"array","items":schema,"minItems":1});
        assert_eq!(
            schema_compile_error(schema).kind(),
            StructuredOutputErrorKind::Unsupported
        );
    }

    #[test]
    fn schema_accepts_exactly_two_hundred_fifty_six_unique_nodes() {
        let properties = (0..SCHEMA_MAX_NODES - 1)
            .map(|index| (format!("p{index}"), serde_json::json!({"type":"string"})))
            .collect::<Map<_, _>>();
        let schema = serde_json::json!({"type":"object","properties":properties});
        assert_eq!(schema_render_fixture(schema, "hello"), "{}");

        let properties = (0..SCHEMA_MAX_NODES)
            .map(|index| (format!("p{index}"), serde_json::json!({"type":"string"})))
            .collect::<Map<_, _>>();
        let schema = serde_json::json!({"type":"object","properties":properties});
        assert_eq!(
            schema_compile_error(schema).kind(),
            StructuredOutputErrorKind::Unsupported
        );
    }
}

// -----------------------------------------------------------------------------
// IsPrimitive: Identifies supported constant and enumeration values.
// -----------------------------------------------------------------------------

/// Return whether a value is a supported primitive witness.
fn is_primitive(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}

// -----------------------------------------------------------------------------
// PointerJoin: Appends an escaped RFC 6901 token.
// -----------------------------------------------------------------------------

/// Append one escaped token to an RFC 6901 pointer.
#[expect(
    rlib::ambiguous_primitive_parameters,
    reason = "the private two-term pointer operation is explicit at every call site"
)]
fn pointer_join(pointer: &str, token: &str) -> String {
    let token = token.replace('~', "~0").replace('/', "~1");
    format!("{pointer}/{token}")
}

// -----------------------------------------------------------------------------
// LocalReferencePointer: Canonicalizes local URI-fragment references.
// -----------------------------------------------------------------------------

/// Decode and canonicalize a local URI-fragment JSON pointer.
///
/// # Errors
///
/// Returns a classified schema error for external or malformed references.
#[expect(
    rlib::ambiguous_primitive_parameters,
    reason = "reference text and diagnostic path are named and never interchangeable at callers"
)]
fn local_reference_pointer(reference: &str, path: &str) -> Result<String, StructuredOutputError> {
    // Reject references outside the deliberately local-only subset.
    let Some(fragment) = reference.strip_prefix('#') else {
        return Err(StructuredOutputError::unsupported(
            path,
            "external references are not supported",
        ));
    };
    let decoded = percent_decode(fragment).map_err(|message| {
        StructuredOutputError::invalid(path, format!("malformed local reference: {message}"))
    })?;

    // The empty URI fragment denotes the schema root.
    if decoded.is_empty() {
        return Ok(String::new());
    }

    // Nonempty local fragments must use RFC 6901 pointer syntax.
    let Some(tokens) = decoded.strip_prefix('/') else {
        return Err(StructuredOutputError::invalid(
            path,
            "local reference must contain an RFC 6901 pointer",
        ));
    };
    let mut pointer = String::new();
    for token in tokens.split('/') {
        let token = pointer_token_decode(token).map_err(|message| {
            StructuredOutputError::invalid(path, format!("malformed local reference: {message}"))
        })?;
        pointer = pointer_join(&pointer, &token);
    }
    Ok(pointer)
}

// -----------------------------------------------------------------------------
// PercentDecode: Decodes one UTF-8 URI fragment.
// -----------------------------------------------------------------------------

/// Percent-decode a URI fragment as UTF-8.
///
/// # Errors
///
/// Returns a static diagnostic for malformed escapes or non-UTF-8 output.
fn percent_decode(value: &str) -> Result<String, &'static str> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }

        // A percent marker must be followed by one complete byte escape.
        let Some(pair) = bytes.get(index + 1..index + 3) else {
            return Err("incomplete percent escape");
        };
        let high = hex_value(pair[0]).ok_or("invalid percent escape")?;
        let low = hex_value(pair[1]).ok_or("invalid percent escape")?;
        decoded.push(high * 16 + low);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| "fragment is not UTF-8")
}

// -----------------------------------------------------------------------------
// PointerTokenDecode: Decodes one escaped RFC 6901 token.
// -----------------------------------------------------------------------------

/// Decode one RFC 6901 reference token.
///
/// # Errors
///
/// Returns a static diagnostic when a tilde escape is not defined by RFC 6901.
fn pointer_token_decode(value: &str) -> Result<String, &'static str> {
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }

        // Reject every tilde escape outside the two defined by RFC 6901.
        let decoded_character = match chars.next() {
            Some('0') => '~',
            Some('1') => '/',
            _ => Err("invalid `~` escape")?,
        };
        decoded.push(decoded_character);
    }
    Ok(decoded)
}

// -----------------------------------------------------------------------------
// HexValue: Converts one ASCII hexadecimal digit.
// -----------------------------------------------------------------------------

/// Convert one ASCII hexadecimal digit.
const fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
