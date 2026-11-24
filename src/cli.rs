mod recording;

use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use ui_workbench::{Result, project::Project};

fn main() -> ExitCode {
    if let Err(error) = run() {
        println!("{}", json!({"ok":false,"error":error.to_string()}));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let mut project = PathBuf::from(".");
    let mut session = String::from("default");
    let mut source = None;
    let mut output = None;
    let mut batch = false;
    let mut debug = false;
    let mut args = env::args().skip(1).peekable();
    if args.peek().is_some_and(|arg| arg == "--replay") {
        args.next();
        let path = args.next().ok_or("--replay requires a recording file")?;
        let frame = match args.next() {
            None => None,
            Some(option) if option == "--frame" => {
                let value = args.next().ok_or("--frame requires a nonnegative number")?;
                Some(value.parse().ok().ok_or("--frame requires a nonnegative number")?)
            }
            _ => return Err("Use --replay FILE [--frame NUMBER]".into()),
        };
        if args.next().is_some() {
            return Err("Use --replay FILE [--frame NUMBER]".into());
        }
        return recording::replay(Path::new(&path), frame);
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "atk-ui-cli [--project DIR] [--session ID] [--batch] [--file FILE | --input FILE | --command JSON] [--output FILE]\nSend one JSON command or an array via stdin. Use --batch to require an array. Use --output for a single export command. Send {{\"command\":\"help\"}} for command discovery."
                );
                println!("Use --debug to record requests, replies, and saved session state. Use --replay FILE to read a recording.");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("atk-ui-cli {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--project" => {
                project = PathBuf::from(args.next().ok_or("--project requires a directory")?)
            }
            "--session" => session = args.next().ok_or("--session requires an ID")?,
            "--batch" if !batch => batch = true,
            "--debug" if !debug => debug = true,
            "--output" | "-o" if output.is_none() => {
                output = Some(args.next().ok_or("--output requires a path")?);
            }
            "--file" | "--input" | "-i" | "--command" if source.is_none() => {
                let input = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                source = Some(if arg == "--command" {
                    input
                } else {
                    fs::read_to_string(input)?
                });
            }
            _ => return Err(format!("Unexpected or repeated argument {arg}").into()),
        }
    }
    let source = match source {
        Some(source) => source,
        None => {
            if io::stdin().is_terminal() {
                return Err("Provide JSON commands, use --help, or run atk-ui for the viewer".into());
            }
            let mut source = String::new();
            io::stdin().read_to_string(&mut source)?;
            source
        }
    };
    let mut value: Value = serde_json::from_str(source.trim_start_matches('\u{feff}'))?;
    if let Some(output) = output {
        if batch || value.is_array() {
            return Err("--output requires a single export command. Set path on individual batch commands.".into());
        }
        if !writes_artifact(&value) {
            return Err("--output requires an export command".into());
        }
        if value.get("path").is_some() {
            return Err("Specify either --output or the JSON path field".into());
        }
        let path = env::current_dir()?.join(output);
        if value["command"] == "session_render" && value.get("format").is_none() {
            let extension = path.extension().and_then(|extension| extension.to_str());
            if let Some(format @ ("png" | "html" | "json")) = extension {
                value["format"] = json!(format);
            }
        }
        value["path"] = json!(path);
    }
    let requests = match value {
        Value::Array(requests) => {
            if !(1..=1024).contains(&requests.len()) {
                return Err("Batch input must be a JSON array of 1..1024 commands".into());
            }
            for (index, request) in requests.iter().enumerate() {
                if !request["command"].as_str().is_some_and(|name| !name.is_empty()) {
                    return Err(format!("Command {}: expected a JSON object with a nonempty command string", index + 1).into());
                }
            }
            requests
        }
        _ if batch => return Err("Batch input must be a JSON array of 1..1024 commands".into()),
        request => vec![request],
    };
    let directory = project.join(".ui").join("recordings");
    let project = Project::new(project);
    let mut recording = if debug {
        let state = recorded_state(&project, &session, &Value::Null);
        Some(recording::Recording::create(&directory, &session, state)?)
    } else {
        None
    };
    let mut stdout = io::stdout().lock();
    for request in requests {
        if let Some(recording) = &mut recording {
            recording.begin(&request)?;
        }
        let result = execute_with_receipt(&project, &session, &request);
        if let Some(recording) = &mut recording {
            let state = recorded_state(&project, &session, &request);
            let reply = match &result {
                Ok(reply) => reply.clone(),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            };
            recording.finish(&reply, state)?;
        }
        let reply = result?;
        writeln!(stdout, "{reply}")?;
    }
    Ok(())
}


fn recorded_state(project: &Project, session: &str, request: &Value) -> Value {
    let document = project.read(session).ok();
    let preview = project.execute(session, &json!({"command": "preview_inspect"})).ok();
    let duplicate = if request["command"] == "session_duplicate" {
        request["to"].as_str().and_then(|id| project.read(id).ok())
    } else {
        None
    };
    json!({"document": document, "preview": preview, "duplicate": duplicate})
}

fn execute_with_receipt(project: &Project, session: &str, request: &Value) -> Result<Value> {
    let result = project.execute(session, request)?;
    let mut reply = json!({"ok":true,"command":request["command"],"result":result});
    if writes_artifact(request) && let Some(path) = result["path"].as_str() {
        reply["written"] = json!({"path": fs::canonicalize(path)?, "bytes": fs::metadata(path)?.len()});
    }
    Ok(reply)
}

fn writes_artifact(request: &Value) -> bool {
    matches!(
        request["command"].as_str(),
        Some("image_compare" | "session_handoff" | "asset_read" | "session_render" | "session_export")
    )
}
