use crate::{
    Result, native, preview,
    project::{Document, encode_base64},
};
use serde_json::{Value, json};

pub const CSS: &str = include_str!("../web/viewer.css");
pub const RENDERER: &str = include_str!("../web/render.js");
pub const VIEWER: &str = include_str!("../web/viewer.js");
pub const FLOW: &str = include_str!("../web/flow.js");
pub const INDEX: &str = include_str!("../web/index.html");
pub const POLICY: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none';";

pub fn snapshot(document: &Document) -> Result<Value> {
    let mut elements = Vec::new();
    let mut actions = Vec::new();
    let mut screen_data = Value::Null;
    let mut sprites = Vec::new();
    let mut nine_patches = std::collections::BTreeMap::new();
    let mut fonts = std::collections::BTreeMap::new();
    if let Some(id) = preview::screen_id(document) {
        let screen = document.screen(id)?;
        let values = preview::control_values(document, screen);
        for element in &document.expand_elements(screen)? {
            if !preview::visible(element, &values) {
                continue;
            }
            let mut node = element.clone();
            let appearance = document.resolved_appearance(element)?;
            if let Some(id) = appearance["patch"].as_str() {
                nine_patches.insert(
                    id.to_owned(),
                    document
                        .nine_patches
                        .get(id)
                        .ok_or_else(|| format!("Unknown nine-patch {id}"))?,
                );
            }
            if let Some(id) = appearance["font"].as_str() {
                fonts.insert(
                    id.to_owned(),
                    document
                        .fonts
                        .get(id)
                        .ok_or_else(|| format!("Unknown workspace font {id}"))?,
                );
            }
            node["appearance"] = appearance;
            let id = element["id"]
                .as_str()
                .ok_or("Saved element is missing its ID")?;
            match element["type"].as_str() {
                Some("button" | "hotspot") if element["action"].is_object() => {
                    let mut action = json!({"target":id,"label":element["text"],"operation":"press"});
                    if let Some(cue) = element["action"]["audioCue"].as_str() {
                        action["audioCue"] = json!(cue);
                    }
                    actions.push(action);
                }
                Some("input" | "checkbox" | "select") => {
                    if let Some(key) = element["key"].as_str() {
                        node["value"] = values[key].clone();
                        actions.push(json!({"target":id,"label":element["text"],"operation":"input","value":values[key]}));
                    }
                }
                _ => {}
            }
            if let Some(fields) = node.as_object_mut() {
                for field in ["action", "when", "key", "appearanceRef"] {
                    fields.remove(field);
                }
            }
            elements.push(node);
        }
        let height = match screen.canvas_mode {
            crate::project::CanvasMode::FixedAspect => screen.height,
            crate::project::CanvasMode::ContentHeight => {
                elements.iter().fold(screen.height, |height, element| {
                    let bottom = element["y"].as_f64().unwrap_or_default()
                        + element["height"].as_f64().unwrap_or_default();
                    height.max(bottom)
                })
            }
        };
        sprites.extend(document.sprites.iter().filter(|asset| {
            elements.iter().any(|node| node["sprite"] == asset["id"])
                || nine_patches
                    .values()
                    .any(|patch| asset["id"] == patch.sprite)
        }));
        screen_data = json!({"id":screen.id,"label":screen.label,"width":screen.width,"height":height,"canvasMode":screen.canvas_mode,
            "background":screen.background,"elements":elements});
    }
    Ok(
        json!({"version":1,"id":document.id,"name":document.name,"revision":document.revision,
        "screen":screen_data,"sprites":sprites,"ninePatches":nine_patches,"fonts":fonts,"actions":actions,
        "gaze":crate::gaze::current(document)?,"flow":document.flow}),
    )
}

pub fn html(snapshot: &Value, mode: native::RenderMode) -> Result<String> {
    let title = snapshot["name"]
        .as_str()
        .unwrap_or("atk-ui")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let scene = native::scene(snapshot)?;
    let image = encode_base64(&native::png_mode(&scene, mode)?);
    Ok(format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title}</title><style>body{{margin:0;background:#181b23}}img{{display:block;max-width:100%;height:auto;margin:auto}}</style>\
         <img alt=\"{title}\" width=\"{}\" height=\"{}\" src=\"data:image/png;base64,{image}\"></html>",
        scene.width, scene.height
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_height_and_hotspots_share_native_geometry() -> Result<()> {
        let mut document: Document = serde_json::from_value(json!({
            "version": 1, "id": "example", "name": "Example", "revision": 1, "entry": "start",
            "screens": [
                {"id":"start","label":"Start","width":200,"height":1,
                 "canvasMode":"content_height","background":"#ffffff","elements":[
                  {"id":"panel","type":"panel","x":0,"y":20,"width":200,"height":60,
                   "appearance":{"fill":[12,24,36,255]}},
                  {"id":"next","type":"hotspot","x":10,"y":35,"width":80,"height":40,
                   "action":{"type":"navigate","screen":"done"}}
                 ]},
                {"id":"done","label":"Done","width":200,"height":50,"background":"#ffffff","elements":[]}
            ]
        }))?;
        let frame = snapshot(&document)?;
        let scene = native::scene(&frame)?;
        assert_eq!([scene.width, scene.height], [200, 80]);
        assert_eq!(scene.commands.len(), 2);
        preview::apply(&mut document, &json!({"target":"next"}))?;
        assert_eq!(snapshot(&document)?["screen"]["id"], "done");
        Ok(())
    }
}
