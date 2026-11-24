use crate::{
    Result, flow, native, preview,
    project::{CanvasMode, Document, Screen, sprite_size, text},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Dependencies {
    sprites: BTreeSet<String>,
    fonts: BTreeSet<String>,
    nine_patches: BTreeSet<String>,
    styles: BTreeSet<String>,
    components: BTreeSet<String>,
    uses_default_font: bool,
}

pub(crate) fn manifest(document: &Document) -> Result<Value> {
    let mut screens = Vec::new();
    for screen in &document.screens {
        screens.push(view_manifest(document, screen, false)?);
    }

    let mut frames = Vec::new();
    if let Some(board) = &document.flow {
        let mut candidate = document.clone();
        for frame in &board.frames {
            flow::apply(&mut candidate, &frame.id)?;
            let screen_id = preview::screen_id(&candidate).ok_or("Flow frame has no screen")?;
            let screen = candidate.screen(screen_id)?;
            frames.push(json!({"frame":frame,"view":view_manifest(&candidate, screen, true)?}));
        }
    }
    let mut sprites = Vec::new();
    for sprite in &document.sprites {
        sprites.push(sprite_metadata(sprite)?);
    }
    let fonts: Vec<_> = document.fonts.iter().map(|(id, font)| json!({
        "id":id,"embedded":font.data.is_some(),"localNames":font.local_names,"weight":font.weight
    })).collect();
    Ok(json!({
        "version":document.version, "session":document.id, "name":document.name,
        "revision":document.revision, "entry":document.entry, "screens":screens,
        "frames":frames, "transitions":document.flow.as_ref().map_or(&[][..], |board| board.edges.as_slice()),
        "assets":{"sprites":sprites,"fonts":fonts,"ninePatches":document.nine_patches},
        "styles":document.styles.keys().collect::<Vec<_>>(),
        "components":document.components.keys().collect::<Vec<_>>()
    }))
}

fn view_manifest(document: &Document, screen: &Screen, visible_only: bool) -> Result<Value> {
    let mut dependencies = Dependencies::default();
    let values = preview::control_values(document, screen);
    for element in &screen.elements {
        if visible_only && !preview::visible(element, &values) {
            continue;
        }
        if let Some(id) = element["component"].as_str() {
            dependencies.components.insert(id.to_owned());
            let component = document.components.get(id).ok_or("Unknown component")?;
            for child in &component.elements {
                if let Some(style) = child["appearanceRef"].as_str() {
                    dependencies.styles.insert(style.to_owned());
                }
            }
        }
    }
    let mut height = screen.height;
    let mut count = 0;
    for element in document.expand_elements(screen)? {
        if visible_only && !preview::visible(&element, &values) {
            continue;
        }
        count += 1;
        if matches!(screen.canvas_mode, CanvasMode::ContentHeight) {
            height = height.max(
                element["y"].as_f64().unwrap_or_default()
                    + element["height"].as_f64().unwrap_or_default(),
            );
        }
        if let Some(id) = element["appearanceRef"].as_str() {
            dependencies.styles.insert(id.to_owned());
        }
        if let Some(id) = element["sprite"].as_str() {
            dependencies.sprites.insert(id.to_owned());
        }
        let appearance = document.resolved_appearance(&element)?;
        if let Some(id) = appearance["patch"].as_str() {
            let patch = document.nine_patches.get(id).ok_or("Unknown nine-patch")?;
            dependencies.nine_patches.insert(id.to_owned());
            dependencies.sprites.insert(patch.sprite.clone());
        }
        if let Some(id) = appearance["font"].as_str() {
            dependencies.fonts.insert(id.to_owned());
        } else if element["text"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
            || matches!(
                element["type"].as_str(),
                Some("input" | "checkbox" | "select")
            )
        {
            dependencies.uses_default_font = true;
        }
    }
    Ok(json!({
        "id":screen.id,"label":screen.label,"width":screen.width,"height":height,
        "canvasMode":screen.canvas_mode,"elementCount":count,"dependencies":dependencies
    }))
}

fn sprite_metadata(sprite: &Value) -> Result<Value> {
    Ok(
        json!({"id":sprite["id"],"kind":"sprite","format":sprite["type"],
        "size":sprite_size(sprite)?,"prompt":sprite["prompt"]}),
    )
}

pub(crate) fn inspect(document: &Document, request: &Value) -> Result<Value> {
    let kind = text(request, "kind")?;
    match kind {
        "sprite" => sprite_metadata(document.sprite(text(request, "id")?)?),
        "font" => {
            let (id, font) = font_reference(document, request)?;
            let (_, path) = native::resolve_font(font)?;
            Ok(
                json!({"id":id,"kind":"font","source":if path.is_some() {"local"} else {"embedded"},
                "path":path,"localNames":font.map(|font| &font.local_names),"weight":font.and_then(|font| font.weight)}),
            )
        }
        "nine_patch" => {
            let id = text(request, "id")?;
            let patch = document.nine_patches.get(id).ok_or("Unknown nine-patch")?;
            Ok(
                json!({"id":id,"kind":"nine_patch","patch":patch,"sourceSize":sprite_size(document.sprite(&patch.sprite)?)?}),
            )
        }
        _ => Err("Asset kind must be sprite, font, or nine_patch".into()),
    }
}

pub(crate) fn read<'a>(document: &'a Document, request: &'a Value) -> Result<(Value, &'a str)> {
    let kind = text(request, "kind")?;
    let data = match kind {
        "sprite" => document.sprite(text(request, "id")?)?["data"]
            .as_str()
            .ok_or("Missing sprite data")?,
        "font" => font_reference(document, request)?
            .1
            .and_then(|font| font.data.as_deref())
            .ok_or("Local fonts are not embedded; use asset_inspect to locate the file")?,
        _ => return Err(
            "Read supports sprite or embedded font files; inspect a nine-patch for its sprite ID"
                .into(),
        ),
    };
    Ok((inspect(document, request)?, data))
}

fn font_reference<'a>(
    document: &'a Document,
    request: &'a Value,
) -> Result<(Option<&'a str>, Option<&'a crate::project::WorkspaceFont>)> {
    match request.get("id") {
        Some(value) => {
            let id = value.as_str().ok_or("Font ID must be a string")?;
            let font = document.fonts.get(id).ok_or("Unknown workspace font")?;
            Ok((Some(id), Some(font)))
        }
        None => Ok((None, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lists_indirect_patch_assets_without_payloads() -> Result<()> {
        let pixels = native::png(&native::Scene::new(8, 8))?;
        let data = crate::project::encode_base64(&pixels);
        let design: Document = serde_json::from_value(json!({
            "version":1,"id":"sample","name":"Sample","revision":4,"entry":"home",
            "sprites":[{"id":"paper","type":"png","size":[8,8],"data":data}],
            "nine_patches":{"card":{"sprite":"paper","insets":[2,2,2,2]}},
            "styles":{"surface":{"appearance":{"patch":"card"}}},
            "components":{"tile":{"elements":[
                {"id":"panel","type":"panel","x":0,"y":0,"width":90,"height":50,"appearanceRef":"surface"}
            ]}},
            "screens":[{"id":"home","label":"Home","width":120,"height":1,"canvasMode":"content_height",
                "background":"#ffffff","elements":[
                    {"id":"tile","type":"component","component":"tile","x":0,"y":10}
                ]}]
        }))?;
        let index = manifest(&design)?;
        let view = &index["screens"][0];
        assert_eq!(view["height"], 60.0);
        assert_eq!(view["dependencies"]["sprites"], json!(["paper"]));
        assert_eq!(view["dependencies"]["ninePatches"], json!(["card"]));
        assert_eq!(view["dependencies"]["styles"], json!(["surface"]));
        assert_eq!(view["dependencies"]["components"], json!(["tile"]));
        assert!(index["assets"]["sprites"][0].get("data").is_none());
        let request = json!({"kind":"sprite","id":"paper"});
        let (_, source) = read(&design, &request)?;
        assert_eq!(native::decode_base64(source)?, pixels);
        Ok(())
    }
}
