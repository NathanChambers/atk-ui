use crate::{Result, edits, preview, render};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize, Serialize)]
pub struct Document {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub entry: Option<String>,
    pub screens: Vec<Screen>,
    #[serde(default)]
    pub sprites: Vec<Value>,
    #[serde(default)]
    pub nine_patches: BTreeMap<String, NinePatch>,
    #[serde(default)]
    pub styles: BTreeMap<String, WorkspaceStyle>,
    #[serde(default)]
    pub fonts: BTreeMap<String, WorkspaceFont>,
    #[serde(default)]
    pub components: BTreeMap<String, Component>,
    #[serde(default)]
    preview_epoch: u64,
    #[serde(skip)]
    pub preview: preview::Preview,
    #[serde(default)]
    pub gaze: Vec<crate::gaze::SavedStudy>,
    #[serde(default)]
    pub flow: Option<crate::flow::Board>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasMode {
    #[default]
    FixedAspect,
    ContentHeight,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Screen {
    pub id: String,
    pub label: String,
    pub width: f64,
    pub height: f64,
    #[serde(default, rename = "canvasMode")]
    pub canvas_mode: CanvasMode,
    pub background: String,
    pub elements: Vec<Value>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Appearance {
    pub fill: Option<[u8; 4]>,
    pub ink: Option<[u8; 4]>,
    pub font_size: Option<u16>,
    pub radius: Option<u16>,
    pub padding: Option<u16>,
    pub cut: Option<u16>,
    pub font: Option<String>,
    pub patch: Option<String>,
    pub centered: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NinePatch {
    pub sprite: String,
    pub insets: [u32; 4],
    #[serde(default = "default_edge_scale")]
    pub edge_scale: f32,
}

fn default_edge_scale() -> f32 {
    1.0
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceStyle {
    pub appearance: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_size: Option<[f64; 2]>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceFont {
    pub local_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Component {
    pub elements: Vec<Value>,
}

#[derive(Serialize, Deserialize)]
struct PreviewFile {
    epoch: u64,
    state: preview::Preview,
}

pub struct Project {
    root: PathBuf,
}

impl Project {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn sessions(&self) -> PathBuf {
        self.root.join(".ui").join("sessions")
    }

    pub fn list(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(self.sessions()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            if let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json"))
                && identifier(id, "Session").is_ok()
                && entry.file_type()?.is_file()
            {
                ids.push(id.to_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn read(&self, id: &str) -> Result<Document> {
        identifier(id, "Session")?;
        let path = self.sessions().join(format!("{id}.json"));
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err("Session must be a regular file".into());
        }
        let source = fs::read_to_string(path)?;
        let document: Document = serde_json::from_str(source.trim_start_matches('\u{feff}'))?;
        if document.version != 1 || document.id != id {
            return Err("Unsupported session version or mismatched session ID".into());
        }
        Ok(document)
    }

    fn preview_path(&self, id: &str) -> PathBuf {
        self.root
            .join(".ui")
            .join("preview")
            .join(format!("{id}.json"))
    }

    fn read_preview(&self, document: &Document) -> Result<preview::Preview> {
        let path = self.preview_path(&document.id);
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(preview::Preview::default());
            }
            Err(error) => return Err(error.into()),
        };
        let preview: PreviewFile = serde_json::from_slice(&bytes)?;
        Ok(if preview.epoch == document.preview_epoch {
            preview.state
        } else {
            preview::Preview::default()
        })
    }

    fn change_preview(&self, id: &str, command: &str, request: &Value) -> Result<Value> {
        identifier(id, "Session")?;
        let _lock = lock_session(&self.sessions().join(format!("{id}.lock")))?;
        let mut document = self.read(id)?;
        check_revision(&document, request)?;
        if command == "preview_action" {
            document.preview = self.read_preview(&document)?;
            preview::apply(&mut document, request)?;
        }
        let path = self.preview_path(id);
        fs::create_dir_all(path.parent().ok_or("Preview path needs a directory")?)?;
        let state = PreviewFile {
            epoch: document.preview_epoch,
            state: document.preview,
        };
        save_document(&state, &path.with_extension("tmp"), &path)?;
        Ok(json!({"session":id,"revision":document.revision,"preview":state.state}))
    }

    fn read_render_view(&self, id: &str, request: &Value) -> Result<Document> {
        let mut document = self.read(id)?;
        match (request.get("screen"), request.get("frame")) {
            (Some(_), Some(_)) => return Err("Select either a screen or a flow frame".into()),
            (Some(screen), None) => {
                let screen = screen.as_str().ok_or("Screen must be an ID")?;
                document.screen(screen)?;
                document.preview.screen = Some(screen.to_owned());
            }
            (None, Some(frame)) => {
                let frame = frame.as_str().ok_or("Frame must be an ID")?;
                crate::flow::apply(&mut document, frame)?;
            }
            (None, None) => document.preview = self.read_preview(&document)?,
        }
        Ok(document)
    }

    fn duplicate_session(&self, id: &str, request: &Value) -> Result<Value> {
        let mut copy = self.read(id)?;
        check_revision(&copy, request)?;
        let destination = identifier(text(request, "to")?, "Destination session ID")?;
        let directory = self.sessions();
        let _lock = lock_session(&directory.join(format!("{destination}.lock")))?;
        let path = directory.join(format!("{destination}.json"));
        if path.try_exists()? {
            return Err(format!("Session {destination} already exists").into());
        }
        copy.id = destination.to_owned();
        copy.name = if let Some(value) = request.get("name") {
            let name = value
                .as_str()
                .ok_or("Workspace name must be a string")?
                .trim();
            if name.is_empty() {
                return Err("Workspace name must not be blank".into());
            }
            name.to_owned()
        } else {
            format!("{} copy", copy.name)
        };
        copy.revision = 1;
        copy.preview_epoch = 1;
        edits::validate(&copy)?;
        save_document(&copy, &directory.join(format!("{destination}.tmp")), &path)?;
        Ok(json!({"session":destination,"revision":copy.revision}))
    }

    fn write_output(&self, path: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.root.join(path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(path)
    }

    pub fn execute(&self, id: &str, request: &Value) -> Result<Value> {
        let command = text(request, "command")?;
        match command {
            "help" => Ok(crate::help::execute(request, help())?),
            "project_list" => Ok(json!(self.list()?)),
            "session_inspect" => {
                let mut document = self.read(id)?;
                for sprite in &mut document.sprites {
                    if let Some(fields) = sprite.as_object_mut() {
                        fields.remove("data");
                    }
                }
                Ok(serde_json::to_value(document)?)
            }
            "session_manifest" => crate::query::manifest(&self.read(id)?),
            "image_compare" => {
                let output = text(request, "path")?;
                let (mut guide, html) = crate::compare::prepare(&self.root, request)?;
                guide["path"] = json!(self.write_output(output, html.as_bytes())?);
                Ok(guide)
            }
            "session_handoff" => {
                let document = self.read(id)?;
                check_revision(&document, request)?;
                let output = text(request, "path")?;
                let bundle = handoff(&document)?;
                let bytes = serde_json::to_vec(&bundle)?;
                let path = self.write_output(output, &bytes)?;
                Ok(json!({"path":path,"format":"ui-workbench-handoff","version":1,"revision":document.revision}))
            }
            "asset_inspect" => crate::query::inspect(&self.read(id)?, request),
            "asset_read" => {
                let document = self.read(id)?;
                let (mut metadata, data) = crate::query::read(&document, request)?;
                if let Some(path) = request.get("path") {
                    let path = path.as_str().ok_or("Asset output path must be a string")?;
                    let bytes = crate::native::decode_base64(data)?;
                    metadata["path"] = json!(self.write_output(path, &bytes)?);
                } else {
                    metadata["dataBase64"] = json!(data);
                }
                Ok(metadata)
            }
            "session_validate" => edits::validate(&self.read(id)?),
            "session_duplicate" => self.duplicate_session(id, request),
            "element_inspect" => {
                let snapshot = render::snapshot(&self.read_render_view(id, request)?)?;
                let name = text(request, "id")?;
                let element = snapshot["screen"]["elements"]
                    .as_array()
                    .and_then(|elements| elements.iter().find(|element| element["id"] == name))
                    .ok_or("No visible element with that ID")?;
                Ok(json!({"screen":snapshot["screen"]["id"],"element":element}))
            }
            "session_render" | "session_export" => {
                let snapshot = render::snapshot(&self.read_render_view(id, request)?)?;
                let format = request["format"]
                    .as_str()
                    .unwrap_or(if command == "session_export" {
                        "html"
                    } else {
                        "json"
                    });
                let mode = match request.get("mode") {
                    Some(value) => crate::native::RenderMode::parse(
                        value.as_str().ok_or("Render mode must be a string")?,
                    )?,
                    None => crate::native::RenderMode::Color,
                };
                if mode != crate::native::RenderMode::Color && !matches!(format, "png" | "html") {
                    return Err("Grayscale is available for PNG and HTML renders".into());
                }
                let bytes = match format {
                    "json" if request.get("path").is_none() => return Ok(snapshot),
                    "json" => serde_json::to_vec_pretty(&snapshot)?,
                    "html" => render::html(&snapshot, mode)?.into_bytes(),
                    "scene" => serde_json::to_vec_pretty(&crate::native::scene(&snapshot)?)?,
                    "png" => crate::native::png_mode(&crate::native::scene(&snapshot)?, mode)?,
                    _ => return Err("Render format must be json, html, scene, or png".into()),
                };
                let path = self.write_output(text(request, "path")?, &bytes)?;
                Ok(json!({"path": path, "revision": snapshot["revision"], "format": format}))
            }
            "preview_inspect" => {
                let document = self.read(id)?;
                Ok(json!({"revision":document.revision,"preview":self.read_preview(&document)?}))
            }
            "preview_reset" | "preview_action" => self.change_preview(id, command, request),
            "session_start" | "screen_set" | "screen_entry_set" | "element_set"
            | "component_set" | "element_remove" | "sprite_import" | "nine_patch_set"
            | "style_set" | "font_set" | "gaze_set" | "flow_set" | "batch" => {
                self.update(id, command, request)
            }
            _ if edits::COMMANDS.contains(&command) => self.update(id, command, request),
            _ => {
                Err(format!("Unknown command {command}. Run help for the command contract.").into())
            }
        }
    }

    fn update(&self, id: &str, command: &str, request: &Value) -> Result<Value> {
        identifier(id, "Session")?;
        let directory = self.sessions();
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{id}.json"));
        let lock = lock_session(&directory.join(format!("{id}.lock")))?;
        let mut document = if command == "session_start" {
            if path.try_exists()? {
                return Err(format!("Session {id} already exists").into());
            }
            Document {
                version: 1,
                id: id.to_owned(),
                name: request["name"].as_str().unwrap_or(id).to_owned(),
                revision: 0,
                entry: None,
                screens: Vec::new(),
                sprites: Vec::new(),
                nine_patches: BTreeMap::new(),
                styles: BTreeMap::new(),
                fonts: BTreeMap::new(),
                components: BTreeMap::new(),
                preview_epoch: 0,
                preview: preview::Preview::default(),
                gaze: Vec::new(),
                flow: None,
            }
        } else {
            self.read(id)?
        };
        check_revision(&document, request)?;
        if command == "gaze_set" {
            document.preview = self.read_preview(&document)?;
        } else {
            document.preview_epoch = document
                .preview_epoch
                .checked_add(1)
                .ok_or("Preview epoch overflow")?;
        }

        self.edit(&mut document, command, request)?;
        if command == "batch" || edits::COMMANDS.contains(&command) {
            edits::validate(&document)?;
        }
        if (edits::COMMANDS.contains(&command) && command != "session_rename")
            || matches!(
                command,
                "screen_set"
                    | "screen_entry_set"
                    | "element_set"
                    | "element_remove"
                    | "sprite_import"
                    | "nine_patch_set"
                    | "style_set"
                    | "font_set"
                    | "component_set"
                    | "batch"
            )
        {
            document.gaze.clear();
        }
        document.revision = document
            .revision
            .checked_add(1)
            .ok_or("Revision overflow")?;
        let temporary = directory.join(format!("{id}.tmp"));
        let result = save_document(&document, &temporary, &path);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        drop(lock);
        Ok(json!({"session": id, "revision": document.revision}))
    }

    fn edit(&self, document: &mut Document, command: &str, request: &Value) -> Result<()> {
        if command == "session_start" {
            return Ok(());
        }
        if command == "batch" {
            let commands = request["commands"]
                .as_array()
                .filter(|items| !items.is_empty())
                .ok_or("Batch requires a nonempty commands array")?;
            for (index, request) in commands.iter().enumerate() {
                let command = text(request, "command")?;
                if request.get("expectedRevision").is_some() {
                    return Err(
                        "Set expectedRevision on the batch, not its individual commands".into(),
                    );
                }
                let allowed = [
                    "screen_set",
                    "screen_entry_set",
                    "element_set",
                    "element_remove",
                    "component_set",
                    "sprite_import",
                    "nine_patch_set",
                    "style_set",
                    "font_set",
                    "flow_set",
                ];
                if !allowed.contains(&command) && !edits::COMMANDS.contains(&command) {
                    return Err(format!("{command} is not an atomic design edit").into());
                }
                self.edit(document, command, request)
                    .map_err(|error| format!("Batch edit {} ({command}): {error}", index + 1))?;
            }
            return Ok(());
        }
        if edits::COMMANDS.contains(&command) {
            return edits::apply(document, command, request);
        }
        if command == "gaze_set" {
            return crate::gaze::set(document, request);
        }
        if command == "flow_set" {
            return crate::flow::set(document, request);
        }

        let id = identifier(text(request, "id")?, "ID")?;
        match command {
            "screen_set" => {
                let width = number(request, "width", 1.0, 32768.0)?;
                let index = document.screens.iter().position(|screen| screen.id == id);
                let previous = index.map(|index| &document.screens[index]);
                let canvas_mode = match request.get("canvasMode") {
                    Some(mode) => serde_json::from_value(mode.clone())?,
                    None => previous
                        .map(|screen| screen.canvas_mode)
                        .unwrap_or_default(),
                };
                let height = if matches!(canvas_mode, CanvasMode::ContentHeight)
                    && request.get("height").is_none()
                {
                    previous
                        .filter(|screen| matches!(screen.canvas_mode, CanvasMode::ContentHeight))
                        .map_or(1.0, |screen| screen.height)
                } else {
                    number(request, "height", 1.0, 32768.0)?
                };
                let label = request["label"]
                    .as_str()
                    .or(previous.map(|screen| screen.label.as_str()))
                    .unwrap_or(id);
                let background = request["background"]
                    .as_str()
                    .or(previous.map(|screen| screen.background.as_str()))
                    .unwrap_or("#ffffff");
                let screen = Screen {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    width,
                    height,
                    canvas_mode,
                    background: background.to_owned(),
                    elements: previous
                        .map(|screen| screen.elements.clone())
                        .unwrap_or_default(),
                };
                match index {
                    Some(index) => document.screens[index] = screen,
                    None => document.screens.push(screen),
                }
                if document.entry.is_none() {
                    document.entry = Some(id.to_owned());
                }
            }
            "screen_entry_set" => {
                document.screen(id)?;
                document.entry = Some(id.to_owned());
                document.preview = preview::Preview::default();
            }
            "element_set" | "element_remove" => {
                let screen = text(request, "screen")?;
                let index = document
                    .screens
                    .iter()
                    .position(|item| item.id == screen)
                    .ok_or("Unknown screen")?;
                if command == "element_set" {
                    let element = validate_element(document, request)?;
                    upsert(&mut document.screens[index].elements, element);
                } else {
                    let elements = &mut document.screens[index].elements;
                    let index = elements
                        .iter()
                        .position(|element| element["id"] == id)
                        .ok_or("Unknown element")?;
                    elements.remove(index);
                }
            }
            "component_set" => {
                let children = request["elements"]
                    .as_array()
                    .ok_or("Component requires an elements array")?;
                if children.is_empty() {
                    return Err("Component requires at least one element".into());
                }
                let mut ids = std::collections::BTreeSet::new();
                let mut elements = Vec::new();
                for child in children {
                    let name = identifier(text(child, "id")?, "Component child ID")?;
                    if !ids.insert(name) {
                        return Err(format!("Duplicate component child {name}").into());
                    }
                    if matches!(child["type"].as_str(), Some("component" | "hotspot"))
                        || ["action", "when", "key"]
                            .iter()
                            .any(|field| child.get(field).is_some())
                    {
                        return Err("Component children are visual elements; author interaction zones on the screen".into());
                    }
                    elements.push(validate_element(document, child)?);
                }
                let definition = Component { elements };
                for instance in document
                    .screens
                    .iter()
                    .flat_map(|screen| &screen.elements)
                    .filter(|element| element["component"] == id)
                {
                    if let Some(overrides) = instance.get("overrides") {
                        validate_component_overrides(document, &definition, overrides)?;
                    }
                }
                document.components.insert(id.to_owned(), definition);
            }
            "nine_patch_set" => {
                let patch: NinePatch = serde_json::from_value(request["patch"].clone())?;
                let sprite = document.sprite(&patch.sprite)?;
                patch.validate(sprite_size(sprite)?)?;
                document.nine_patches.insert(id.to_owned(), patch);
            }
            "style_set" => {
                let style: WorkspaceStyle = serde_json::from_value(request["style"].clone())?;
                document.validate_appearance(&style.appearance)?;
                if let Some(size) = style.default_size
                    && size.iter().any(|axis| !(1.0..=32768.0).contains(axis))
                {
                    return Err("Style default size must be between 1 and 32768 per axis".into());
                }
                document.styles.insert(id.to_owned(), style);
            }
            "font_set" => {
                let mut font: WorkspaceFont = serde_json::from_value(request["font"].clone())?;
                if font.data.is_some() {
                    return Err("Use path to import a font file".into());
                }
                if let Some(path) = request.get("path") {
                    let path = path.as_str().ok_or("Font path must be a string")?;
                    let bytes = fs::read(self.root.join(path))?;
                    crate::native::check_font(&bytes)?;
                    font.data = Some(encode_base64(&bytes));
                }
                if font.data.is_none() && font.local_names.is_empty() {
                    return Err("Provide a font path or local font names".into());
                }
                if font.local_names.iter().any(|name| {
                    name.is_empty()
                        || name.contains(['/', '\\', ':'])
                        || matches!(name.as_str(), "." | "..")
                }) {
                    return Err("Local font names must be filenames".into());
                }
                if font
                    .weight
                    .is_some_and(|weight| !(1..=1000).contains(&weight))
                {
                    return Err("Font weight must be between 1 and 1000".into());
                }
                document.fonts.insert(id.to_owned(), font);
            }
            "sprite_import" => {
                let path = self.root.join(text(request, "path")?);
                let png = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
                if !png {
                    return Err("Use a PNG sprite for native rendering".into());
                }
                let bytes = fs::read(path)?;
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err("Sprite bytes do not match the PNG format".into());
                }
                let reader = png::Decoder::new(bytes.as_slice()).read_info()?;
                let size = [reader.info().width, reader.info().height];
                for patch in document
                    .nine_patches
                    .values()
                    .filter(|patch| patch.sprite == id)
                {
                    patch.validate(size)?;
                }
                upsert(
                    &mut document.sprites,
                    json!({"id":id,"type":"png","size":size,"prompt":request["prompt"].as_str().unwrap_or_default(),"data":encode_base64(&bytes)}),
                );
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}

impl NinePatch {
    pub(crate) fn validate(&self, [width, height]: [u32; 2]) -> Result<()> {
        let [left, top, right, bottom] = self.insets;
        if u64::from(left) + u64::from(right) >= u64::from(width)
            || u64::from(top) + u64::from(bottom) >= u64::from(height)
            || !self.edge_scale.is_finite()
            || self.edge_scale <= 0.0
            || self.edge_scale > 64.0
        {
            return Err("Nine-patch insets must leave a center, and edgeScale must be greater than 0 and at most 64".into());
        }
        Ok(())
    }
}

pub(crate) fn sprite_size(sprite: &Value) -> Result<[u32; 2]> {
    if let Some(value) = sprite.get("size") {
        return Ok(serde_json::from_value(value.clone())?);
    }
    let data = sprite["data"].as_str().ok_or("Missing sprite data")?;
    crate::native::png_size(data)
}

impl Document {
    pub(crate) fn sprite(&self, id: &str) -> Result<&Value> {
        self.sprites
            .iter()
            .find(|sprite| sprite["id"] == id)
            .ok_or_else(|| format!("Unknown sprite {id}").into())
    }

    pub(crate) fn validate_appearance(&self, value: &Value) -> Result<()> {
        let appearance: Appearance = serde_json::from_value(value.clone())?;
        if let Some(id) = appearance.patch
            && !self.nine_patches.contains_key(&id)
        {
            return Err(format!("Unknown nine-patch {id}").into());
        }
        if let Some(id) = appearance.font
            && !self.fonts.contains_key(&id)
        {
            return Err(format!("Unknown workspace font {id}").into());
        }
        Ok(())
    }

    pub(crate) fn expand_elements(&self, screen: &Screen) -> Result<Vec<Value>> {
        let mut elements = Vec::new();
        for node in &screen.elements {
            if node["type"] != "component" {
                elements.push(node.clone());
                continue;
            }
            let name = text(node, "component")?;
            let instance = text(node, "id")?;
            let component = self
                .components
                .get(name)
                .ok_or_else(|| format!("Unknown component {name}"))?;
            let x = number(node, "x", -32768.0, 32768.0)?;
            let y = number(node, "y", -32768.0, 32768.0)?;
            for template in &component.elements {
                let id = text(template, "id")?;
                let mut child = template.clone();
                child["id"] = json!(format!("{instance}/{id}"));
                child["x"] = json!(x + number(template, "x", -32768.0, 32768.0)?);
                child["y"] = json!(y + number(template, "y", -32768.0, 32768.0)?);
                let overrides = &node["overrides"][id];
                if let Some(text) = overrides.get("text") {
                    child["text"] = text.clone();
                }
                if let Some(fields) = overrides["appearance"].as_object() {
                    let mut appearance = self.resolved_appearance(&child)?;
                    appearance
                        .as_object_mut()
                        .ok_or("Expected appearance object")?
                        .extend(fields.clone());
                    child["appearance"] = appearance;
                    child
                        .as_object_mut()
                        .ok_or("Expected component child")?
                        .remove("appearanceRef");
                }
                elements.push(child);
            }
        }
        Ok(elements)
    }

    pub(crate) fn resolved_appearance(&self, element: &Value) -> Result<Value> {
        let mut result = serde_json::Map::new();
        if let Some(id) = element["appearanceRef"].as_str() {
            let style = self
                .styles
                .get(id)
                .ok_or_else(|| format!("Unknown style {id}"))?;
            result.extend(
                style
                    .appearance
                    .as_object()
                    .ok_or("Style appearance must be an object")?
                    .clone(),
            );
        }
        if let Some(inline) = element.get("appearance") {
            result.extend(
                inline
                    .as_object()
                    .ok_or("Element appearance must be an object")?
                    .clone(),
            );
        }
        Ok(Value::Object(result))
    }

    pub fn screen(&self, id: &str) -> Result<&Screen> {
        self.screens
            .iter()
            .find(|screen| screen.id == id)
            .ok_or_else(|| format!("Unknown screen {id}").into())
    }
}

fn check_revision(document: &Document, request: &Value) -> Result<()> {
    if let Some(expected) = request.get("expectedRevision")
        && expected.as_u64() != Some(document.revision)
    {
        return Err(format!("Expected revision {expected}, found {}", document.revision).into());
    }
    Ok(())
}

fn save_document(document: &impl Serialize, temporary: &Path, path: &Path) -> Result<()> {
    match fs::remove_file(temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    serde_json::to_writer_pretty(&mut file, document)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, path)?;
    Ok(())
}

fn lock_session(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.try_lock()
        .map_err(|error| format!("Could not lock session: {error}"))?;
    Ok(file)
}

pub fn identifier<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let mut bytes = value.bytes();
    let valid = value.len() <= 64
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        });
    if !valid {
        return Err(format!("{label} must be a lowercase ID of up to 64 characters").into());
    }
    Ok(value)
}

pub fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{field} must be a nonempty string").into())
}

pub fn number(value: &Value, field: &str, min: f64, max: f64) -> Result<f64> {
    value[field]
        .as_f64()
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| format!("{field} must be between {min} and {max}").into())
}

fn upsert(values: &mut Vec<Value>, value: Value) {
    match values.iter().position(|item| item["id"] == value["id"]) {
        Some(index) => values[index] = value,
        None => values.push(value),
    }
}

pub(crate) fn validate_element(document: &Document, request: &Value) -> Result<Value> {
    let kind = text(request, "type")?;
    if kind == "component" {
        return validate_component_instance(document, request);
    }
    if ![
        "panel", "text", "sprite", "button", "input", "checkbox", "select", "hotspot",
    ]
    .contains(&kind)
    {
        return Err("Unknown element type".into());
    }
    for field in ["x", "y"] {
        number(request, field, -32768.0, 32768.0)?;
    }
    if request.get("buttonStyle").is_some() {
        return Err("Use appearanceRef and a workspace nine-patch".into());
    }
    let style = request
        .get("appearanceRef")
        .map(|value| {
            let id = value
                .as_str()
                .ok_or("Appearance reference must be a string")?;
            document.styles.get(id).ok_or("Unknown style")
        })
        .transpose()?;
    let mut dimensions = [0.0; 2];
    for (axis, field) in ["width", "height"].into_iter().enumerate() {
        dimensions[axis] = match style
            .and_then(|style| style.default_size)
            .filter(|_| request.get(field).is_none())
        {
            Some(size) => size[axis],
            None => number(request, field, 1.0, 32768.0)?,
        };
    }
    if let Some(sprite) = request.get("sprite") {
        if !["sprite", "panel", "button", "text"].contains(&kind)
            || !document.sprites.iter().any(|item| item["id"] == *sprite)
        {
            return Err("Unknown sprite or unsupported sprite element type".into());
        }
    } else if kind == "sprite" {
        return Err("Sprite elements require a sprite ID".into());
    }
    if let Some(key) = request.get("key") {
        if !["input", "checkbox", "select"].contains(&kind) {
            return Err("Only form controls can bind to a state key".into());
        }
        identifier(key.as_str().unwrap_or_default(), "State key")?;
    }
    if let Some(when) = request.get("when") {
        identifier(text(when, "key")?, "Visibility key")?;
        if !when.get("equals").is_some_and(scalar) {
            return Err("Visibility requires a scalar comparison value".into());
        }
    }
    if let Some(action) = request.get("action") {
        if !["button", "hotspot"].contains(&kind) {
            return Err("Only buttons and hotspots may have an action".into());
        }
        match text(action, "type")? {
            "navigate" => {
                document.screen(text(action, "screen")?)?;
            }
            "toggle" | "set" => {
                identifier(text(action, "key")?, "Action key")?;
                if action["type"] == "set" && !action.get("value").is_some_and(scalar) {
                    return Err("Set requires a scalar value".into());
                }
            }
            _ => return Err("Unknown button action".into()),
        }
    }
    if kind == "hotspot" && request["action"]["type"] != "navigate" {
        return Err("Hotspots require a navigate action".into());
    }
    if kind == "select"
        && !request["options"]
            .as_array()
            .is_some_and(|options| !options.is_empty() && options.iter().all(Value::is_string))
    {
        return Err("Select requires a nonempty array of string options".into());
    }
    if request.get("style").is_some() {
        return Err("Use typed appearance values instead of style".into());
    }
    if let Some(value) = request.get("appearance") {
        document.validate_appearance(value)?;
    }
    let mut fields = request
        .as_object()
        .ok_or("Element must be an object")?
        .clone();
    fields.retain(|key, _| {
        [
            "id",
            "type",
            "text",
            "x",
            "y",
            "width",
            "height",
            "sprite",
            "action",
            "when",
            "key",
            "options",
            "appearance",
            "appearanceRef",
        ]
        .contains(&key.as_str())
    });
    fields.insert("width".to_owned(), json!(dimensions[0]));
    fields.insert("height".to_owned(), json!(dimensions[1]));
    Ok(Value::Object(fields))
}

fn validate_component_instance(document: &Document, request: &Value) -> Result<Value> {
    let id = identifier(text(request, "id")?, "Element ID")?;
    let name = text(request, "component")?;
    let definition = document.components.get(name).ok_or("Unknown component")?;
    let x = number(request, "x", -32768.0, 32768.0)?;
    let y = number(request, "y", -32768.0, 32768.0)?;
    if request.as_object().is_some_and(|fields| {
        fields.keys().any(|key| {
            ![
                "command",
                "screen",
                "expectedRevision",
                "id",
                "type",
                "component",
                "x",
                "y",
                "overrides",
            ]
            .contains(&key.as_str())
        })
    }) {
        return Err("Component instances accept position and child overrides".into());
    }
    let mut element = json!({"id":id,"type":"component","component":name,"x":x,"y":y});
    if let Some(overrides) = request.get("overrides") {
        validate_component_overrides(document, definition, overrides)?;
        element["overrides"] = overrides.clone();
    }
    Ok(element)
}

pub(crate) fn validate_component_overrides(
    document: &Document,
    definition: &Component,
    value: &Value,
) -> Result<()> {
    let overrides = value
        .as_object()
        .ok_or("Component overrides must be keyed by child ID")?;
    for (id, value) in overrides {
        if !definition.elements.iter().any(|child| child["id"] == *id) {
            return Err(format!("Unknown component child {id}").into());
        }
        let fields = value
            .as_object()
            .ok_or("Child override must be an object")?;
        if fields
            .keys()
            .any(|field| !["text", "appearance"].contains(&field.as_str()))
            || fields.get("text").is_some_and(|text| !text.is_string())
        {
            return Err("Child overrides may set text and appearance".into());
        }
        if let Some(appearance) = fields.get("appearance") {
            document.validate_appearance(appearance)?;
        }
    }
    Ok(())
}

fn scalar(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_string() || value.is_number()
}

fn handoff(document: &Document) -> Result<Value> {
    let manifest = crate::query::manifest(document)?;
    let mut view = document.clone();
    let mut screens = BTreeMap::new();
    for screen in &document.screens {
        view.preview = preview::Preview::default();
        view.preview.screen = Some(screen.id.clone());
        screens.insert(&screen.id, handoff_reference(&view)?);
    }

    let mut frames = BTreeMap::new();
    if let Some(board) = &document.flow {
        for frame in &board.frames {
            crate::flow::apply(&mut view, &frame.id)?;
            frames.insert(&frame.id, handoff_reference(&view)?);
        }
    }

    Ok(json!({
        "format":"ui-workbench-handoff",
        "version":1,
        "manifest":manifest,
        "design":document,
        "references":{"screens":screens,"frames":frames}
    }))
}

fn handoff_reference(document: &Document) -> Result<Value> {
    let snapshot = render::snapshot(document)?;
    let scene = crate::native::scene(&snapshot)?;
    let png = crate::native::png_mode(&scene, crate::native::RenderMode::Color)?;
    Ok(json!({
        "screen":snapshot["screen"],
        "actions":snapshot["actions"],
        "image":{"format":"png","dataBase64":encode_base64(&png)}
    }))
}

pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(first >> 2) as usize] as char);
        result.push(ALPHABET[((first & 3) << 4 | second >> 4) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[((second & 15) << 2 | third >> 6) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(third & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

fn help() -> Value {
    let mut commands = json!({
        "session_start": {"name":"optional workspace name"},
        "session_inspect": {}, "project_list": {}, "session_validate": {},
        "screen_set": {"id":"screen","width":1080,"height":1920,"canvasMode":"fixed_aspect | content_height","label":"optional","background":"optional hex color"},
        "screen_entry_set": {"id":"screen"},
        "element_set": {"id":"element","screen":"screen","type":"panel | text | sprite | button | input | checkbox | select | hotspot",
            "x":0,"y":0,"width":100,"height":100,"optional":["text","sprite","action","key","when","options","appearance","appearanceRef"]},
        "appearance": {"fill":[14,20,30,255],"ink":[255,255,255,255],"fontSize":24,"radius":6,"padding":8,"cut":0,"font":"workspace-font-id","patch":"workspace-nine-patch-id","centered":false},
        "component_set":{"id":"header","elements":[{"id":"title","type":"text","x":0,"y":0,"width":140,"height":32,"text":"Title"}]},
        "component_instance":{"command":"element_set","screen":"home","id":"header","type":"component","component":"header","x":40,"y":20,"overrides":{"title":{"text":"Home"}}},
        "nine_patch_set":{"id":"patch","patch":{"sprite":"asset-id","insets":[8,8,8,8],"edgeScale":1}},
        "style_set":{"id":"style","style":{"appearance":{"patch":"patch","fontSize":24,"centered":true},"defaultSize":[200,96]}},
        "font_set":{"id":"font","font":{"localNames":["font.ttf"],"weight":400},"path":"optional font file to embed"},
        "element_remove": {"id":"element","screen":"screen"},
        "sprite_import": {"id":"sprite","path":"sprite.png","prompt":"optional"},
        "preview_inspect": {}, "preview_reset": {},
        "preview_action": {"target":"inspect session_render for available action IDs","value":"for input actions"},
        "gaze_set": {"study":{"targets":[{"id":"action","label":"Primary action","rect":[0,0,200,90],"score":85}],"path":["action"],"rationale":"Why these areas likely draw attention."}},
        "flow_set": {"board":{"frames":[{"id":"start","title":"Start","x":0,"y":0,"path":[]},{"id":"reference","title":"Reference","x":0,"y":450,"screen":"component-screen"}],"edges":[{"from":"frame-id","to":"next-frame-id","action":"action target","label":"Action label","reuseView":"optional; match the layout while allowing different data"}]}},
        "session_render": {"screen":"optional screen","frame":"optional board frame","format":"json | html | scene | png","mode":"color | grayscale (PNG and HTML)","path":"optional for json; required for other formats"},
        "session_export": {"path":"preview.html"},
        "notes":["Mutations accept expectedRevision. Geometry uses design pixels. Later elements paint above earlier elements."]
    });
    let edits = json!({
        "session_manifest":{},
        "image_compare": {
            "reference":"reference PNG path",
            "capture":"implementation PNG path",
            "path":"unused output HTML path",
            "focus":["optional regions or details to check"],
            "expectedDifferences":["optional differences to accept"]
        },
        "session_handoff":{"path":"unused output JSON path","expectedRevision":"optional source revision"},
        "asset_inspect":{"kind":"sprite | font | nine_patch","id":"workspace asset ID; omit for default font"},
        "asset_read":{"kind":"sprite | font (embedded only)","id":"asset ID","path":"optional unused output path; returns base64 when omitted"},
        "session_rename":{"name":"new display name"},
        "session_duplicate":{"to":"new session ID","name":"optional display name"},
        "batch":{"commands":[{"command":"element_patch","screen":"home","id":"button","changes":{"x":40}}]},
        "screen_duplicate":{"id":"screen","to":"new-screen","label":"optional"},
        "screen_rename":{"id":"screen","to":"new-screen","label":"optional"},
        "screen_remove":{"id":"unused-screen"},
        "element_patch":{"screen":"home","id":"element","changes":{"x":40,"appearance":{"fontSize":24}}},
        "element_duplicate":{"screen":"home","id":"element","to":"copy","toScreen":"optional destination","index":"optional paint index","changes":{}},
        "element_move":{"screen":"home","id":"element","toScreen":"optional destination","index":"optional paint index","changes":{"x":40}},
        "element_reorder":{"screen":"home","id":"element","index":0},
        "element_inspect":{"id":"element or instance/child","screen":"optional screen","frame":"optional board frame"},
        "component_element_patch":{"id":"component","child":"title","changes":{"text":"Updated"}},
        "component_remove":{"id":"unused-component"},
        "style_remove":{"id":"unused-style"},
        "font_remove":{"id":"unused-font"},
        "sprite_remove":{"id":"unused-sprite"},
        "nine_patch_remove":{"id":"unused-patch"},
        "flow_frame_set":{"id":"frame","frame":{"title":"Frame","x":0,"y":0,"screen":"optional standalone screen ID; omit to replay path"}},
        "flow_frame_patch":{"id":"frame","changes":{"x":100}},
        "flow_frame_remove":{"id":"frame"},
        "flow_edge_set":{"edge":{"from":"start","to":"end","action":"next","label":"Next"}},
        "flow_edge_patch":{"from":"start","to":"end","action":"next","changes":{"label":"Continue"}},
        "flow_edge_remove":{"from":"start","to":"end","action":"next"},
    });
    commands
        .as_object_mut()
        .unwrap()
        .extend(edits.as_object().unwrap().clone());
    commands
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn rejected_batch_preserves_design_bytes_and_revision() -> Result<()> {
        let path = env_test_path()?;
        let result = (|| {
            let project = Project::new(&path);
            let session = "fixture";
            for command in [
                json!({"command":"session_start"}),
                json!({"command":"screen_set","id":"home","width":100,"height":100}),
                json!({"command":"screen_set","id":"details","width":100,"height":100}),
                json!({"command":"element_set","screen":"home","id":"next","type":"hotspot",
                    "x":0,"y":0,"width":40,"height":40,"action":{"type":"navigate","screen":"details"}}),
            ] {
                project.execute(session, &command)?;
            }
            let design = project.sessions().join("fixture.json");
            let previous = fs::read(&design)?;
            let invalid = json!({
                "command":"batch","expectedRevision":4,"commands":[
                    {"command":"element_patch","screen":"home","id":"next","changes":{"x":25}},
                    {"command":"screen_remove","id":"details"}
                ]
            });
            assert!(project.execute(session, &invalid).is_err());
            assert_eq!(fs::read(design)?, previous);
            assert_eq!(project.read(session)?.revision, 4);
            Ok(())
        })();
        fs::remove_dir_all(path)?;
        result
    }

    fn env_test_path() -> Result<PathBuf> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("atk-ui-edit-{}-{stamp}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(path)
    }
}
