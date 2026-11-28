use serde_json::{Value, json};

const GUIDE: &str = include_str!("../docs/usage.md");

pub fn execute(request: &Value, mut catalog: Value) -> Result<Value, String> {
    let topics = json!({
        "workflow": "Quickstart",
        "command-api": "Usage and command reference",
        "batch-commands": "CLI command conventions",
        "recording-and-replay": "Debug recordings and replay"
    });

    let Some(topic) = request.get("topic") else {
        let all = request.get("all").map_or(Ok(false), |value| {
            value.as_bool().ok_or("all must be a boolean")
        })?;
        if all {
            return Ok(json!({"topics": topics}));
        }

        catalog["tasks"] = json!({
            "get_started": ["workflow", "command-api"],
            "run_commands": ["batch-commands"],
            "record_and_replay": ["recording-and-replay"]
        });
        catalog["read_topic"] = json!({"command": "help", "topic": "workflow"});
        catalog["all_topics"] = json!({"command": "help", "all": true});
        catalog["pagination"] = json!("Continue with next_offset until null.");
        return Ok(catalog);
    };

    let topic = topic.as_str().ok_or("topic must be a string")?;
    let Some(title) = topics[topic].as_str() else {
        if let Some(fields) = catalog.get(topic).filter(|entry| entry.is_object())
            && !matches!(topic, "appearance" | "component_instance")
        {
            return Ok(json!({"command":topic,"fields":fields}));
        }
        return Err("unknown help topic or command; use help with all:true for topics or help without arguments for commands".into());
    };
    let text = if topic == "command-api" {
        GUIDE
    } else {
        let heading = format!("## {title}");
        let (_, section) = GUIDE.split_once(&heading).ok_or("Help section is missing")?;
        section.split("\n## ").next().unwrap_or_default().trim()
    };
    let offset = request.get("offset").map_or(Ok(0), |value| {
        value.as_u64().ok_or("offset must be an unsigned integer")
    })?;
    let offset = usize::try_from(offset).map_err(|_| "offset is too large")?;
    let limit = request.get("limit").map_or(Ok(32), |value| {
        value.as_u64().filter(|value| (1..=256).contains(value)).ok_or("limit must be 1..256")
    })? as usize;
    let lines: Vec<_> = text.lines().collect();
    let page: Vec<_> = lines.iter().skip(offset).take(limit).copied().collect();
    let next = offset.saturating_add(limit);

    Ok(json!({
        "topic": topic,
        "title": title,
        "text": page.join("\n"),
        "total": lines.len(),
        "offset": offset,
        "next_offset": (next < lines.len()).then_some(next)
    }))
}
