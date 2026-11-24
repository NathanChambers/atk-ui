use crate::{
    Result, preview,
    project::{Document, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Study {
    pub targets: Vec<Target>,
    #[serde(default)]
    pub path: Vec<String>,
    pub rationale: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub rect: [f64; 4],
    pub score: u8,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct SavedStudy {
    screen: String,
    state: Value,
    study: Study,
}

pub fn set(document: &mut Document, request: &Value) -> Result<()> {
    let screen_id =
        preview::screen_id(document).ok_or("Select a screen before saving a gaze study")?;
    let screen = document.screen(screen_id)?;
    let study: Study = serde_json::from_value(request["study"].clone())?;
    if study.targets.is_empty() || study.targets.len() > 32 || study.rationale.trim().is_empty() {
        return Err("Gaze study needs 1–32 targets and a rationale".into());
    }

    let mut ids = BTreeSet::new();
    for target in &study.targets {
        let id = identifier(&target.id, "Gaze target ID")?;
        let [x, y, width, height] = target.rect;
        let inside = x >= 0.0
            && y >= 0.0
            && width > 0.0
            && height > 0.0
            && x + width <= screen.width
            && y + height <= screen.height;
        if !inside || target.score > 100 || target.label.trim().is_empty() || !ids.insert(id) {
            return Err(format!("Gaze target {id} needs a unique ID, label, in-bounds rectangle, and score from 0 to 100").into());
        }
    }

    let mut visited = BTreeSet::new();
    for id in &study.path {
        if !ids.contains(id.as_str()) || !visited.insert(id) {
            return Err(format!("Gaze path target {id} must exist and appear only once").into());
        }
    }

    let state = serde_json::to_value(&document.preview)?;
    let screen = screen_id.to_owned();
    let entry = SavedStudy {
        screen,
        state,
        study,
    };
    if let Some(previous) = document
        .gaze
        .iter_mut()
        .find(|item| item.screen == entry.screen && item.state == entry.state)
    {
        *previous = entry;
    } else {
        document.gaze.push(entry);
    }
    Ok(())
}

pub fn current(document: &Document) -> Result<Option<&Study>> {
    let Some(screen) = preview::screen_id(document) else {
        return Ok(None);
    };
    let state = serde_json::to_value(&document.preview)?;
    Ok(document
        .gaze
        .iter()
        .find(|item| item.screen == screen && item.state == state)
        .map(|item| &item.study))
}
