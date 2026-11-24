use crate::{Result, project::encode_base64};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, fs, path::Path};

const STEPS: [&str; 4] = [
    "Check that the images show the same screen and interaction state. Note viewport, crop, and safe-area differences.",
    "Review each area in paint order. Check for missing elements, then compare bounds, spacing, borders, colors, icons, and text alignment.",
    "Account for expected differences and dynamic content. Mark anything the images cannot establish as uncertain.",
    "Report each actionable mismatch with its location, what the reference shows, and what the capture shows. Prioritize by user impact.",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Comparison {
    reference: String,
    capture: String,
    #[serde(default)]
    focus: Vec<String>,
    #[serde(default)]
    expected_differences: Vec<String>,
}

pub(crate) fn prepare(root: &Path, request: &Value) -> Result<(Value, String)> {
    let comparison: Comparison = serde_json::from_value(request.clone())?;
    let reference = root.join(&comparison.reference);
    let capture = root.join(&comparison.capture);
    let mut html = String::from(
        r#"<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:">
<title>UI comparison</title>
<style>
body { color-scheme: light dark; font: 16px/1.5 system-ui, sans-serif; margin: 2rem auto; padding: 0 1rem; max-width: 100rem; }
.images { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1rem; }
figure { margin: 0; min-width: 0; }
figcaption { margin-bottom: .5rem; font-weight: 600; }
img { display: block; width: 100%; height: auto; }
</style>
<main><h1>UI comparison</h1><div class="images">
"#,
    );
    let reference_size = append_image(&mut html, "Reference", &reference)?;
    let capture_size = append_image(&mut html, "Implementation capture", &capture)?;
    html.push_str("</div>");
    append_list(&mut html, "Focus", &comparison.focus);
    append_list(&mut html, "Expected differences", &comparison.expected_differences);

    html.push_str("<h2>Review steps</h2><ol>");
    for step in STEPS {
        write!(html, "<li>{step}</li>")?;
    }
    html.push_str("</ol></main></html>");

    let guide = json!({
        "reference":{"path":reference,"size":reference_size},
        "capture":{"path":capture,"size":capture_size},
        "focus":comparison.focus,
        "expectedDifferences":comparison.expected_differences,
        "reviewSteps":STEPS
    });
    Ok((guide, html))
}

fn append_image(html: &mut String, label: &str, path: &Path) -> Result<[u32; 2]> {
    let bytes = fs::read(path).map_err(|error| format!("{label} {}: {error}", path.display()))?;
    let reader = png::Decoder::new(bytes.as_slice())
        .read_info()
        .map_err(|error| format!("{label} {} must be a readable PNG: {error}", path.display()))?;
    let width = reader.info().width;
    let height = reader.info().height;
    let data = encode_base64(&bytes);
    write!(
        html,
        r#"<figure><figcaption>{label} · {width} × {height}</figcaption><img alt="{label}" width="{width}" height="{height}" src="data:image/png;base64,{data}"></figure>"#
    )?;
    Ok([width, height])
}

fn append_list(html: &mut String, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }

    html.push_str("<h2>");
    html.push_str(label);
    html.push_str("</h2><ul>");
    for item in items {
        html.push_str("<li>");
        for character in item.chars() {
            match character {
                '&' => html.push_str("&amp;"),
                '<' => html.push_str("&lt;"),
                '>' => html.push_str("&gt;"),
                _ => html.push(character),
            }
        }
        html.push_str("</li>");
    }
    html.push_str("</ul>");
}
