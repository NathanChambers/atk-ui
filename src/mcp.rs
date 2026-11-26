use serde_json::{Map, Value, json};
use std::io::{self, BufRead, Write};

pub fn tool(name: &str, schema: Value) -> Value {
    let description = name.replace('_', " ");
    json!({"name":name,"description":description,"inputSchema":schema})
}

pub fn example_tool(name: &str, example: &Value) -> Value {
    let mut properties = Map::new();
    if let Some(example) = example.as_object() {
        for (name, value) in example {
            let schema = match value {
                Value::Bool(_) => json!({"type":"boolean"}),
                Value::Number(_) => json!({"type":"number"}),
                Value::Array(_) => json!({"type":"array"}),
                Value::Object(_) => json!({"type":"object"}),
                Value::String(hint) => json!({"description":hint}),
                Value::Null => json!({}),
            };
            if name != "command" && name != "optional" {
                properties.insert(name.clone(), schema);
            }
        }
    }
    for field in example["optional"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        properties.entry(field.to_owned()).or_insert(json!({}));
    }
    tool(name, json!({"type":"object","properties":properties}))
}

pub fn batch_tool(name: &str) -> Value {
    json!({
        "name": name,
        "description": "Run an ordered batch of native commands. Split batches when a later step needs a previously unknown result.",
        "inputSchema": {
            "type":"object",
            "properties":{"commands":{"type":"array","minItems":1,"maxItems":1024,
                "items":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}},
            "required":["commands"]
        }
    })
}

pub fn tools_call(request: Value, batch: &str) -> Result<Value, String> {
    let (name, arguments) = if request.is_array() {
        (batch.to_owned(), json!({"commands":request}))
    } else {
        let mut arguments = request.as_object().cloned().ok_or("Expected a JSON command object")?;
        let command = arguments.remove("command").ok_or("command is required")?;
        let name = command.as_str().ok_or("command must be a string")?.to_owned();
        (name, Value::Object(arguments))
    };
    Ok(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
}

pub fn native_command(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut request = arguments.as_object().cloned().ok_or("Expected a tool arguments object")?;
    if request.contains_key("command") {
        return Err("Use the MCP tool name to select the command".into());
    }
    request.insert("command".into(), json!(name));
    Ok(Value::Object(request))
}

pub fn call_local(
    request: Value,
    batch: &str,
    dispatch: impl FnOnce(&str, &Value) -> Result<Value, String>,
) -> Result<Value, String> {
    let envelope = tools_call(request, batch)?;
    let params = &envelope["params"];
    dispatch(params["name"].as_str().unwrap(), &params["arguments"])
}

pub fn batch_commands(arguments: &Value) -> Result<&[Value], String> {
    let commands = arguments["commands"].as_array().ok_or("Expected commands array")?;
    if !(1..=1024).contains(&commands.len()) || commands.iter().any(|command| !command["command"].as_str().is_some_and(|name| !name.is_empty())) {
        return Err("Expected 1 to 1024 native command objects".into());
    }
    Ok(commands)
}

pub fn run_batch(commands: &[Value], mut execute: impl FnMut(&Value) -> Result<Value, String>) -> Value {
    let mut replies = Vec::with_capacity(commands.len());
    for command in commands {
        match execute(command) {
            Ok(reply) => replies.push(reply),
            Err(error) => return json!({"ok":false,"replies":replies,"failed_command":replies.len()+1,"error":error}),
        }
    }
    json!({"ok":true,"replies":replies})
}

pub fn dispatch_request(
    request: &Value,
    server: &str,
    catalog: &[Value],
    batch_guidance: bool,
    dispatch: &mut impl FnMut(&str, &Value) -> Result<Value, String>,
) -> Option<Value> {
    let id = &request["id"];
    let error = |code, message: &str| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}});
    if request["jsonrpc"] != "2.0" || !request["method"].is_string() {
        return Some(error(-32600, "Invalid Request"));
    }
    if request.get("id").is_none() {
        return None;
    }
    if !id.is_string() && !id.is_i64() && !id.is_u64() {
        return Some(error(-32600, "Invalid request ID"));
    }

    let params = &request["params"];
    let result = match request["method"].as_str().unwrap() {
        "initialize" => {
            let requested = params["protocolVersion"].as_str();
            let version = match requested {
                Some(version @ ("2025-11-25" | "2025-06-18")) => version,
                Some(_) => "2025-11-25",
                None => return Some(error(-32602, "Expected protocolVersion")),
            };
            let mut result = json!({"protocolVersion":version,"capabilities":{"tools":{}},
                "serverInfo":{"name":server,"version":"0.1.0"}});
            if batch_guidance {
                result["instructions"] = json!("Use batches when command inputs are known. Split at steps that depend on returned IDs, revisions or evidence.");
            }
            result
        }
        "ping" => json!({}),
        "tools/list" if params.get("cursor").is_none() => json!({"tools":catalog}),
        "tools/list" => return Some(error(-32602, "Invalid cursor")),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(error(-32602, "Expected tool name"));
            };
            if !catalog.iter().any(|tool| tool["name"] == name) {
                return Some(error(-32602, "Unknown tool"));
            }
            let empty = json!({});
            let arguments = params.get("arguments").unwrap_or(&empty);
            if !arguments.is_object() {
                return Some(error(-32602, "Expected an arguments object"));
            }
            let reply = dispatch(name, arguments).unwrap_or_else(|error| json!({"ok":false,"error":error}));
            json!({"content":[{"type":"text","text":reply.to_string()}],
                "structuredContent":reply,"isError":reply["ok"]==false})
        }
        _ => return Some(error(-32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

pub fn serve(
    server: &str,
    catalog: &[Value],
    batch_guidance: bool,
    mut dispatch: impl FnMut(&str, &Value) -> Result<Value, String>,
) -> io::Result<()> {
    let mut output = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let response = match serde_json::from_str(&line) {
            Ok(request) => dispatch_request(&request, server, catalog, batch_guidance, &mut dispatch),
            Err(_) => Some(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}
