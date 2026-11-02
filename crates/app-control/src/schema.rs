//! JSON Schemas of operation inputs and outputs, shaped the way MCP clients expect a tool's:
//! draft 2020-12, an object at the root that names its properties and takes no others, and no
//! title or description of its own (the Rust type's name and doc are noise to a model; each
//! field's doc comment is its description).

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value};

/// The schema of `T` as a JSON object: root `type` `object`, `properties` present even when
/// empty (some clients require it), and `additionalProperties` `false`.
pub fn object_schema<T: JsonSchema>() -> Map<String, Value> {
    let schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>();
    let mut object = match serde_json::to_value(schema) {
        Ok(Value::Object(object)) => object,
        _ => Map::new(),
    };
    object.remove("title");
    object.remove("description");
    object.insert("type".into(), "object".into());
    object
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    object.insert("additionalProperties".into(), false.into());
    object
}
