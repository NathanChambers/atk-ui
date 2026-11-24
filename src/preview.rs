use crate::{
    Result,
    project::{Document, Screen, text},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Preview {
    #[serde(default)]
    pub screen: Option<String>,
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
}

pub fn screen_id(document: &Document) -> Option<&str> {
    document
        .preview
        .screen
        .as_deref()
        .or(document.entry.as_deref())
}

pub fn control_values(document: &Document, screen: &Screen) -> BTreeMap<String, Value> {
    let mut values = document.preview.values.clone();
    for element in &screen.elements {
        if let Some(key) = element["key"].as_str() {
            let default = match element["type"].as_str() {
                Some("checkbox") => json!(false),
                Some("select") => element["options"][0].clone(),
                _ => json!(""),
            };
            values.entry(key.to_owned()).or_insert(default);
        }
    }
    values
}

pub fn visible(element: &Value, values: &BTreeMap<String, Value>) -> bool {
    element["when"]["key"]
        .as_str()
        .is_none_or(|key| values.get(key) == element["when"].get("equals"))
}

pub fn apply(document: &mut Document, request: &Value) -> Result<()> {
    let target = text(request, "target")?;
    let (element_id, operation) = target.split_once(':').unwrap_or((target, ""));
    let screen_id = screen_id(document)
        .ok_or("Create a screen before interacting")?
        .to_owned();
    let screen = document.screen(&screen_id)?;
    let values = control_values(document, screen);
    let element = screen
        .elements
        .iter()
        .find(|element| element["id"] == element_id && visible(element, &values))
        .ok_or_else(|| format!("No visible element {element_id} on screen {screen_id}"))?;
    match element["type"].as_str() {
        Some("button" | "hotspot") if operation.is_empty() => {
            let action = &element["action"];
            match action["type"].as_str() {
                Some("navigate") => {
                    let destination = text(action, "screen")?.to_owned();
                    document.screen(&destination)?;
                    document.preview.screen = Some(destination);
                    document.preview.values = values;
                }
                Some("set" | "toggle") => {
                    let key = text(action, "key")?.to_owned();
                    let value = if action["type"] == "set" {
                        action["value"].clone()
                    } else {
                        json!(!truthy(values.get(&key)))
                    };
                    document.preview.values = values;
                    document.preview.values.insert(key, value);
                }
                _ => return Err("Button has no action".into()),
            }
        }
        Some("input" | "checkbox" | "select") if operation.is_empty() => {
            let key = text(element, "key")?.to_owned();
            let value = &request["value"];
            let valid = match element["type"].as_str() {
                Some("input") => value.is_string(),
                Some("checkbox") => value.is_boolean(),
                _ => element["options"]
                    .as_array()
                    .is_some_and(|options| options.contains(value)),
            };
            if !valid {
                return Err("Invalid value for this control".into());
            }
            document.preview.values = values;
            document.preview.values.insert(key, value.clone());
        }
        _ => return Err("Element has no matching preview action".into()),
    }
    Ok(())
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        _ => true,
    }
}
