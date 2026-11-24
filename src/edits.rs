use crate::{
    Result, flow, native, preview,
    project::{Document, identifier, text, validate_component_overrides, validate_element},
    render,
};
use serde_json::{Value, json};

pub(crate) const COMMANDS: &[&str] = &[
    "session_rename",
    "screen_duplicate",
    "screen_rename",
    "screen_remove",
    "element_patch",
    "element_duplicate",
    "element_move",
    "element_reorder",
    "component_element_patch",
    "component_remove",
    "style_remove",
    "sprite_remove",
    "font_remove",
    "nine_patch_remove",
    "flow_frame_set",
    "flow_frame_patch",
    "flow_frame_remove",
    "flow_edge_set",
    "flow_edge_patch",
    "flow_edge_remove",
];

pub(crate) fn apply(document: &mut Document, command: &str, request: &Value) -> Result<()> {
    match command {
        "session_rename" => {
            let name = text(request, "name")?.trim();
            if name.is_empty() {
                return Err("Workspace name must not be blank".into());
            }
            document.name = name.to_owned();
        }
        "screen_duplicate" | "screen_rename" | "screen_remove" => {
            screen_edit(document, command, request)?
        }
        "element_patch" | "element_duplicate" | "element_move" | "element_reorder" => {
            element_edit(document, command, request)?
        }
        "component_element_patch" => patch_component(document, request)?,
        "component_remove" | "style_remove" | "sprite_remove" | "font_remove"
        | "nine_patch_remove" => remove_resource(document, command, request)?,
        _ => flow_edit(document, command, request)?,
    }
    Ok(())
}

fn screen_edit(document: &mut Document, command: &str, request: &Value) -> Result<()> {
    let id = identifier(text(request, "id")?, "Screen ID")?;
    let index = document
        .screens
        .iter()
        .position(|screen| screen.id == id)
        .ok_or("Unknown screen")?;
    if command == "screen_remove" {
        if document.entry.as_deref() == Some(id) {
            if document.screens.len() != 1 {
                return Err("Select a different entry screen before removing it".into());
            }
            document.entry = None;
        }
        document.screens.remove(index);
        return Ok(());
    }

    let target = identifier(text(request, "to")?, "Destination screen ID")?;
    if document.screens.iter().any(|screen| screen.id == target) {
        return Err(format!("Screen {target} already exists").into());
    }
    let mut screen = document.screens[index].clone();
    screen.id = target.to_owned();
    if let Some(label) = request.get("label") {
        screen.label = label
            .as_str()
            .ok_or("Screen label must be a string")?
            .to_owned();
    }
    if command == "screen_duplicate" {
        document.screens.push(screen);
        return Ok(());
    }
    document.screens[index] = screen;
    if document.entry.as_deref() == Some(id) {
        document.entry = Some(target.to_owned());
    }
    if let Some(board) = &mut document.flow {
        for frame in &mut board.frames {
            if frame.screen.as_deref() == Some(id) {
                frame.screen = Some(target.to_owned());
            }
        }
    }

    for element in document
        .screens
        .iter_mut()
        .flat_map(|screen| &mut screen.elements)
    {
        if element["action"]["type"] == "navigate" && element["action"]["screen"] == id {
            element["action"]["screen"] = json!(target);
        }
    }
    Ok(())
}

fn element_edit(document: &mut Document, command: &str, request: &Value) -> Result<()> {
    let screen = identifier(text(request, "screen")?, "Screen ID")?;
    let source = document
        .screens
        .iter()
        .position(|item| item.id == screen)
        .ok_or("Unknown screen")?;
    let id = identifier(text(request, "id")?, "Element ID")?;
    let index = document.screens[source]
        .elements
        .iter()
        .position(|node| node["id"] == id)
        .ok_or("Unknown element")?;

    let mut element = document.screens[source].elements[index].clone();
    if let Some(changes) = request.get("changes") {
        patch_element(&mut element, changes, false)?;
    } else if command == "element_patch" {
        return Err("Element patch requires changes".into());
    }
    let destination = match request.get("toScreen") {
        Some(value) if matches!(command, "element_move" | "element_duplicate") => {
            let name = value.as_str().ok_or("toScreen must be a screen ID")?;
            document
                .screens
                .iter()
                .position(|screen| screen.id == name)
                .ok_or("Unknown destination screen")?
        }
        Some(_) => return Err("Only move and duplicate accept toScreen".into()),
        None => source,
    };

    if command == "element_duplicate" {
        element["id"] = json!(identifier(text(request, "to")?, "Duplicate element ID")?);
    }
    if (command == "element_duplicate" || destination != source)
        && document.screens[destination]
            .elements
            .iter()
            .any(|node| node["id"] == element["id"])
    {
        return Err("Destination already contains that element ID".into());
    }

    let validated = validate_element(document, &element)?;
    let original_position = source == destination && command == "element_patch";
    if original_position {
        document.screens[source].elements[index] = validated;
        return Ok(());
    }
    if command != "element_duplicate" {
        document.screens[source].elements.remove(index);
    }

    let elements = &mut document.screens[destination].elements;
    let position = match request.get("index") {
        Some(value) => {
            let index = value
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .ok_or("index must be a nonnegative integer")?;
            if index > elements.len() {
                return Err("Element index is outside the destination paint order".into());
            }
            index
        }
        None if command == "element_reorder" => return Err("Reorder requires a paint index".into()),
        None if command == "element_move" && source == destination => index,
        None => elements.len(),
    };
    elements.insert(position, validated);
    Ok(())
}

fn patch_component(document: &mut Document, request: &Value) -> Result<()> {
    let id = text(request, "id")?;
    let child_id = text(request, "child")?;
    let definition = document.components.get(id).ok_or("Unknown component")?;
    let index = definition
        .elements
        .iter()
        .position(|node| node["id"] == child_id)
        .ok_or("Unknown component child")?;
    let mut child = definition.elements[index].clone();
    patch_element(&mut child, &request["changes"], true)?;
    let validated = validate_element(document, &child)?;
    document
        .components
        .get_mut(id)
        .ok_or("Unknown component")?
        .elements[index] = validated;
    Ok(())
}

fn remove_resource(document: &mut Document, command: &str, request: &Value) -> Result<()> {
    let id = identifier(text(request, "id")?, "Resource ID")?;
    let removed = match command {
        "component_remove" => document.components.remove(id).is_some(),
        "style_remove" => document.styles.remove(id).is_some(),
        "font_remove" => document.fonts.remove(id).is_some(),
        "nine_patch_remove" => document.nine_patches.remove(id).is_some(),
        "sprite_remove" => {
            let index = document
                .sprites
                .iter()
                .position(|sprite| sprite["id"] == id)
                .ok_or("Unknown sprite")?;
            document.sprites.remove(index);
            true
        }
        _ => false,
    };
    if !removed {
        return Err(format!("Unknown resource {id}").into());
    }
    Ok(())
}

fn flow_edit(document: &mut Document, command: &str, request: &Value) -> Result<()> {
    if document.flow.is_none() && command != "flow_frame_set" {
        return Err("Create a flow frame first".into());
    }
    let board = document.flow.get_or_insert_with(|| flow::Board {
        frames: Vec::new(),
        edges: Vec::new(),
    });
    if command.starts_with("flow_frame_") {
        let id = identifier(text(request, "id")?, "Flow frame ID")?;
        let position = board.frames.iter().position(|frame| frame.id == id);
        match command {
            "flow_frame_set" => {
                let mut value = request["frame"].clone();
                value
                    .as_object_mut()
                    .ok_or("Frame must be an object")?
                    .insert("id".to_owned(), json!(id));
                let frame = serde_json::from_value(value)?;
                match position {
                    Some(index) => board.frames[index] = frame,
                    None => board.frames.push(frame),
                }
            }
            "flow_frame_patch" => {
                let frame = &mut board.frames[position.ok_or("Unknown flow frame")?];
                let mut value = serde_json::to_value(&*frame)?;
                merge(&mut value, &request["changes"], &["id"])?;
                *frame = serde_json::from_value(value)?;
            }
            "flow_frame_remove" => {
                board.frames.remove(position.ok_or("Unknown flow frame")?);
                board.edges.retain(|edge| edge.from != id && edge.to != id);
            }
            _ => return Err("Unknown flow frame command".into()),
        }
    } else if command == "flow_edge_set" {
        let edge: flow::Edge = serde_json::from_value(request["edge"].clone())?;
        if let Some(position) = board
            .edges
            .iter()
            .position(|item| edge_key(item) == edge_key(&edge))
        {
            board.edges[position] = edge;
        } else {
            board.edges.push(edge);
        }
    } else {
        let key = (
            text(request, "from")?,
            text(request, "to")?,
            text(request, "action")?,
        );
        let position = board
            .edges
            .iter()
            .position(|edge| edge_key(edge) == key)
            .ok_or("Unknown flow edge")?;
        if command == "flow_edge_remove" {
            board.edges.remove(position);
        } else {
            let mut value = serde_json::to_value(&board.edges[position])?;
            merge(&mut value, &request["changes"], &[])?;
            board.edges[position] = serde_json::from_value(value)?;
        }
    }
    if board.frames.is_empty() {
        document.flow = None;
    }
    Ok(())
}

fn edge_key(edge: &flow::Edge) -> (&str, &str, &str) {
    (&edge.from, &edge.to, &edge.action)
}

fn patch_element(value: &mut Value, changes: &Value, visual_only: bool) -> Result<()> {
    let allowed = [
        "x",
        "y",
        "width",
        "height",
        "text",
        "appearance",
        "appearanceRef",
        "sprite",
        "action",
        "when",
        "key",
        "options",
        "component",
        "overrides",
    ];
    let fields = changes.as_object().ok_or("Changes must be an object")?;
    if let Some(field) = fields.keys().find(|key| {
        !allowed.contains(&key.as_str())
            || (visual_only
                && ["action", "when", "key", "component", "overrides"].contains(&key.as_str()))
    }) {
        return Err(format!("Cannot patch element field {field}").into());
    }
    merge(value, changes, &[])
}

fn merge(value: &mut Value, changes: &Value, protected: &[&str]) -> Result<()> {
    let changes = changes.as_object().ok_or("Changes must be an object")?;
    let fields = value.as_object_mut().ok_or("Expected an editable object")?;
    for (key, change) in changes {
        if protected.contains(&key.as_str()) {
            return Err(format!("Cannot patch {key}").into());
        }
        if change.is_null() {
            fields.remove(key);
        } else if fields.get(key).is_some_and(Value::is_object) && change.is_object() {
            merge(fields.get_mut(key).ok_or("Missing field")?, change, &[])?;
        } else {
            fields.insert(key.clone(), change.clone());
        }
    }
    Ok(())
}

pub(crate) fn validate(document: &Document) -> Result<Value> {
    if let Some(entry) = &document.entry {
        document.screen(entry)?;
    }
    for style in document.styles.values() {
        document.validate_appearance(&style.appearance)?;
    }
    for patch in document.nine_patches.values() {
        let sprite = document.sprite(&patch.sprite)?;
        patch.validate(crate::project::sprite_size(sprite)?)?;
    }
    for definition in document.components.values() {
        for child in &definition.elements {
            validate_element(document, child)?;
        }
    }
    let mut candidate = document.clone();
    for screen in &document.screens {
        for element in &screen.elements {
            validate_element(document, element)?;
            if let Some(component) = element["component"].as_str()
                && let Some(overrides) = element.get("overrides")
            {
                validate_component_overrides(
                    document,
                    document
                        .components
                        .get(component)
                        .ok_or("Unknown component")?,
                    overrides,
                )?;
            }
        }
        candidate.preview = preview::Preview {
            screen: Some(screen.id.clone()),
            ..Default::default()
        };
        native::scene(&render::snapshot(&candidate)?)?;
    }
    if let Some(board) = &document.flow {
        let mut keys = std::collections::BTreeSet::new();
        if board.edges.iter().any(|edge| !keys.insert(edge_key(edge))) {
            return Err("Duplicate flow edge".into());
        }
        flow::validate(document, board)?;
    }
    Ok(
        json!({"valid":true,"revision":document.revision,"screens":document.screens.len(),"frames":document.flow.as_ref().map_or(0, |board| board.frames.len())}),
    )
}
