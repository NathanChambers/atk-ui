use crate::{
    Result, preview,
    project::{Document, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub frames: Vec<Frame>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub id: String,
    pub title: String,
    pub x: u32,
    pub y: u32,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub action: String,
    pub label: String,
    #[serde(
        default,
        rename = "reuseView",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub reuse_view: bool,
}

pub fn set(document: &mut Document, request: &Value) -> Result<()> {
    let board: Board = serde_json::from_value(request["board"].clone())?;
    validate(document, &board)?;
    document.flow = Some(board);
    Ok(())
}

pub(crate) fn validate(document: &Document, board: &Board) -> Result<()> {
    if board.frames.is_empty() || board.frames.len() > 24 || board.edges.len() > 48 {
        return Err("A flow board needs 1–24 frames and at most 48 connections".into());
    }
    let mut states = BTreeMap::new();
    let mut candidate = document.clone();
    for frame in &board.frames {
        let id = identifier(&frame.id, "Flow frame ID")?;
        if frame.title.trim().is_empty()
            || frame.x > 3500
            || frame.y > 3500
            || frame.path.len() > 32
        {
            return Err(format!("Invalid title, position or path for flow frame {id}").into());
        }

        replay(&mut candidate, frame)?;
        if states.insert(id, (candidate.preview.clone(), frame.screen.is_some())).is_some() {
            return Err(format!("Duplicate flow frame {id}").into());
        }
    }
    for edge in &board.edges {
        let (source, source_reference) = states
            .get(edge.from.as_str())
            .ok_or("Unknown source frame")?;
        let (destination, destination_reference) = states
            .get(edge.to.as_str())
            .ok_or("Unknown destination frame")?;
        if *source_reference || *destination_reference {
            return Err("Standalone screen frames cannot have flow connections".into());
        }
        candidate.preview = source.clone();
        if edge.label.trim().is_empty() {
            return Err("Flow connections need a label".into());
        }
        preview::apply(&mut candidate, &serde_json::json!({"target": edge.action}))?;
        let matches = if edge.reuse_view {
            let result = view_signature(&candidate)?;
            candidate.preview = destination.clone();
            result == view_signature(&candidate)?
        } else {
            serde_json::to_value(&candidate.preview)? == serde_json::to_value(destination)?
        };
        if !matches {
            return Err(format!(
                "Action {} does not lead from {} to {}",
                edge.action, edge.from, edge.to
            )
            .into());
        }
    }
    Ok(())
}

fn view_signature(document: &Document) -> Result<Value> {
    let id = preview::screen_id(document).ok_or("A flow view needs a screen")?;
    let screen = document.screen(id)?;
    let values = preview::control_values(document, screen);
    let mut visible = Vec::new();
    for element in screen
        .elements
        .iter()
        .filter(|element| preview::visible(element, &values))
    {
        let element_id = element["id"]
            .as_str()
            .ok_or("A flow view element needs an ID")?;
        visible.push(element_id);
    }
    Ok(serde_json::json!([id, visible]))
}

pub fn apply(document: &mut Document, id: &str) -> Result<()> {
    let entry = document
        .flow
        .as_ref()
        .and_then(|board| board.frames.iter().find(|frame| frame.id == id));
    let frame = entry.ok_or("Unknown flow frame")?.clone();
    replay(document, &frame)
}

fn replay(document: &mut Document, frame: &Frame) -> Result<()> {
    document.preview = preview::Preview::default();
    if let Some(screen) = &frame.screen {
        if !frame.path.is_empty() {
            return Err("A frame can select a standalone screen or replay a path".into());
        }
        document.screen(screen)?;
        document.preview.screen = Some(screen.clone());
        return Ok(());
    }

    for target in &frame.path {
        preview::apply(document, &serde_json::json!({"target":target}))?;
    }
    Ok(())
}
