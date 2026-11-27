mod build_identity;
use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Condvar, Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use atk_ui::{
    Result, native, preview,
    project::{Document, Project},
    render,
};

#[derive(PartialEq, Eq)]
struct FrameKey {
    session: String,
    revision: u64,
    mode: native::RenderMode,
    path: String,
    thumbnail: bool,
}

#[derive(Default)]
struct FrameCache {
    frames: VecDeque<(FrameKey, Arc<[u8]>)>,
    bytes: usize,
}

#[derive(Default)]
struct SessionEvents {
    clients: Mutex<usize>,
    generation: Mutex<u64>,
    changed: Condvar,
}

impl FrameCache {
    fn get(&mut self, key: &FrameKey) -> Option<Arc<[u8]>> {
        let index = self
            .frames
            .iter()
            .position(|(candidate, _)| candidate == key)?;
        let entry = self.frames.remove(index)?;
        let result = Arc::clone(&entry.1);
        self.frames.push_back(entry);
        Some(result)
    }

    fn insert(&mut self, key: FrameKey, pixels: Arc<[u8]>) {
        const LIMIT: usize = 8 * 1024 * 1024;
        const FRAMES: usize = 16;
        if pixels.len() > LIMIT {
            return;
        }
        if let Some(index) = self
            .frames
            .iter()
            .position(|(candidate, _)| candidate == &key)
            && let Some((_, previous)) = self.frames.remove(index)
        {
            self.bytes -= previous.len();
        }

        while self.bytes + pixels.len() > LIMIT || self.frames.len() >= FRAMES {
            if let Some((_, entry)) = self.frames.pop_front() {
                self.bytes -= entry.len();
            }
        }
        self.bytes += pixels.len();
        self.frames.push_back((key, pixels));
    }
}

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let mut root = None;
    let mut port = 0;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "atk-ui [--project DIR | DIR] [--port PORT] [--headless]\nThe viewer prints its URL on startup. --headless is accepted for compatibility.\nServes saved UI render snapshots in a read-only browser viewer.\nThe directory defaults to the current working directory. Omit --port or pass 0 for an available port. Use --version or -V for build identity."
                );
                return Ok(());
            }
            "--version" | "-V" => {
                build_identity::print_version()?;
                return Ok(());
            }
            "--project" if root.is_none() => {
                root = Some(PathBuf::from(
                    args.next().ok_or("--project requires a directory")?,
                ))
            }
            "--headless" => {},
            "--port" => port = args.next().ok_or("--port requires a number")?.parse()?,
            path if !path.starts_with('-') && root.is_none() => root = Some(PathBuf::from(path)),
            _ => return Err(format!("Unexpected or repeated argument {arg}").into()),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from(".")).canonicalize()?;
    if !root.is_dir() {
        return Err("Project root must be a directory".into());
    }
    let events = Arc::new(SessionEvents::default());
    let directory = root.join(".ui").join("sessions");
    let watcher = Arc::clone(&events);
    thread::spawn(move || watch_sessions(&directory, &watcher));
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let address = listener.local_addr()?;
    let url = format!("http://{address}");
    eprintln!("atk-ui viewer at {url}");
    eprintln!("Watching project: {}", root.display());
    let project = Arc::new(Project::new(root));
    let frames = Arc::new(Mutex::new(FrameCache::default()));
    let (send, receive) = mpsc::sync_channel(16);
    let receive = Arc::new(Mutex::new(receive));
    for _ in 0..4 {
        let project = Arc::clone(&project);
        let frames = Arc::clone(&frames);
        let events = Arc::clone(&events);
        let receive = Arc::clone(&receive);
        thread::spawn(move || {
            loop {
                let stream = receive.lock().unwrap().recv();
                let Ok(stream) = stream else { break };
                let _ = handle(stream, &project, address.port(), &frames, &events);
            }
        });
    }
    drop(receive);
    for stream in listener.incoming().flatten() {
        if send.send(stream).is_err() {
            break;
        }
    }
    Ok(())
}

fn watch_sessions(directory: &Path, events: &SessionEvents) {
    let mut previous = None;
    loop {
        let current = session_files(directory);
        if previous.as_ref() != Some(&current) {
            previous = Some(current);
            *events.generation.lock().unwrap() += 1;
            events.changed.notify_all();
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn session_files(directory: &Path) -> BTreeMap<PathBuf, (Option<SystemTime>, u64)> {
    let Ok(entries) = fs::read_dir(directory) else {
        return BTreeMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            metadata
                .is_file()
                .then(|| (path, (metadata.modified().ok(), metadata.len())))
        })
        .collect()
}

fn start_event_stream(mut stream: TcpStream, events: &Arc<SessionEvents>) -> Result<()> {
    const MAX_CLIENTS: usize = 16;
    let mut clients = events.clients.lock().unwrap();
    if *clients >= MAX_CLIENTS {
        drop(clients);
        return respond(
            &mut stream,
            "503 Service Unavailable",
            "text/plain",
            "Too many viewer connections",
            None,
        );
    }
    *clients += 1;

    let listener = Arc::clone(events);
    let worker = thread::Builder::new().spawn(move || {
        let _ = stream_events(stream, &listener);
        *listener.clients.lock().unwrap() -= 1;
    });
    if worker.is_err() {
        *clients -= 1;
    }
    worker?;
    Ok(())
}

fn stream_events(mut stream: TcpStream, events: &SessionEvents) -> std::io::Result<()> {
    let mut last = *events.generation.lock().unwrap();
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n: connected\n\n")?;

    loop {
        let generation = events.generation.lock().unwrap();
        let (generation, _) = events
            .changed
            .wait_timeout_while(generation, Duration::from_secs(10), |value| *value == last)
            .unwrap();
        let current = *generation;
        drop(generation);

        if current == last {
            stream.write_all(b": heartbeat\n\n")?;
        } else {
            stream.write_all(b"event: change\ndata: sessions\n\n")?;
            last = current;
        }
    }
}

fn handle(
    mut stream: TcpStream,
    project: &Project,
    port: u16,
    frames: &Mutex<FrameCache>,
    events: &Arc<SessionEvents>,
) -> Result<()> {
    let timeout = Some(Duration::from_secs(2));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    let mut reader = BufReader::new((&mut stream).take(16 * 1024));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut request = line.split_whitespace();
    let method = request.next().unwrap_or_default();
    let path = request.next().unwrap_or("/");
    let mut revision = None;
    let mut host = None;
    for header in reader.lines() {
        let header = header?;
        if header.is_empty() {
            break;
        }
        if let Some((key, value)) = header.split_once(':') {
            if key.eq_ignore_ascii_case("host") {
                host = Some(value.trim().to_owned());
            } else if key.eq_ignore_ascii_case("if-none-match") {
                revision = Some(value.trim().to_owned());
            }
        }
    }
    if !host.is_some_and(|host| {
        host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
    }) {
        return respond(
            &mut stream,
            "403 Forbidden",
            "text/plain",
            "Use the local viewer address",
            None,
        );
    }
    if method != "GET" {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            "Viewer serves GET requests only",
            None,
        );
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    if path == "/api/events" {
        return start_event_stream(stream, events);
    }
    if path == "/" {
        let page = render::INDEX.replace("__VIEWER_GENERATION__", viewer_generation());
        return respond(&mut stream, "200 OK", "text/html", &page, None);
    }
    let asset = match path {
        "/viewer.css" => Some(("text/css", render::CSS)),
        "/viewer.js" => Some(("text/javascript", render::VIEWER)),
        "/flow.js" => Some(("text/javascript", render::FLOW)),
        "/render.js" => Some(("text/javascript", render::RENDERER)),
        _ => None,
    };
    if let Some((kind, value)) = asset {
        return respond(&mut stream, "200 OK", kind, value, None);
    }
    if path == "/api/sessions" {
        if query_value(query, "details") == Some("1") {
            let generation = *events.generation.lock().unwrap();
            let tag = format!("\"workspaces-{}-{generation}\"", viewer_generation());
            if revision.as_ref() == Some(&tag) {
                return respond(
                    &mut stream,
                    "304 Not Modified",
                    "application/json",
                    "",
                    Some(&tag),
                );
            }

            let workspaces: Vec<_> = project
                .list()?
                .into_iter()
                .map(|id| {
                    let name = project.read(&id).map_or_else(|_| id.clone(), |doc| doc.name);
                    serde_json::json!({ "id": id, "name": name })
                })
                .collect();
            let list = serde_json::to_string(&workspaces)?;
            return respond(&mut stream, "200 OK", "application/json", &list, Some(&tag));
        }

        let list = serde_json::to_string(&project.list()?)?;
        return respond(&mut stream, "200 OK", "application/json", &list, None);
    }
    if let Some(id) = path.strip_prefix("/api/sessions/") {
        if let Some((id, image)) = id.split_once("/frame/") {
            let mode = query
                .split('&')
                .find_map(|value| value.strip_prefix("mode="))
                .unwrap_or("color");
            let mode = match native::RenderMode::parse(mode) {
                Ok(mode) => mode,
                Err(error) => {
                    return respond(
                        &mut stream,
                        "400 Bad Request",
                        "text/plain",
                        &error.to_string(),
                        None,
                    );
                }
            };
            let prepare_start = Instant::now();
            let frame = project.read(id).and_then(|mut document| {
                let requested = image
                    .strip_suffix(".png")
                    .and_then(|value| value.parse::<u64>().ok());
                if requested != Some(document.revision) {
                    return Err("This frame revision is no longer current".into());
                }
                apply_preview(&mut document, query)?;
                let key = FrameKey {
                    session: document.id.clone(),
                    revision: document.revision,
                    mode,
                    path: if let Some(flow) = query_value(query, "flow") {
                        format!("flow:{flow}")
                    } else {
                        query_value(query, "preview")
                            .map_or_else(String::new, |path| format!("preview:{path}"))
                    },
                    thumbnail: query_value(query, "flow").is_some()
                        && query_value(query, "thumb") == Some("1"),
                };
                if let Some(frame) = frames.lock().unwrap().get(&key) {
                    return Ok((frame, "cache;desc=\"hit\"".to_owned()));
                }

                let snapshot = render::snapshot(&document)?;
                let mut scene = native::scene(&snapshot)?;
                if key.thumbnail {
                    scene.limit_resolution(768);
                }
                let prepare = prepare_start.elapsed();
                let (png, raster, encode) = native::profiled_png(&scene, mode)?;
                let frame: Arc<[u8]> = png.into();
                frames.lock().unwrap().insert(key, Arc::clone(&frame));
                let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
                let timing = format!(
                    "cache;desc=\"miss\", prepare;dur={:.2}, raster;dur={:.2}, encode;dur={:.2}",
                    milliseconds(prepare),
                    milliseconds(raster),
                    milliseconds(encode)
                );
                Ok((frame, timing))
            });
            return match frame {
                Ok((frame, timing)) => respond_bytes(
                    &mut stream,
                    "200 OK",
                    "image/png",
                    &frame,
                    None,
                    Some(&timing),
                ),
                Err(error) => respond(
                    &mut stream,
                    "404 Not Found",
                    "text/plain",
                    &error.to_string(),
                    None,
                ),
            };
        }
        match project.read(id).and_then(|mut document| {
            let tag = format!("\"{}-{}\"", document.id, document.revision);
            let transient =
                query_value(query, "preview").is_some() || query_value(query, "flow").is_some();
            if !transient && revision.as_ref() == Some(&tag) {
                return Ok((tag, None));
            }
            apply_preview(&mut document, query)?;
            let snapshot = render::snapshot(&document)?;
            Ok((tag, Some(serde_json::to_string(&snapshot)?)))
        }) {
            Ok((tag, Some(snapshot))) => {
                return respond(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    &snapshot,
                    Some(&tag),
                );
            }
            Ok((tag, None)) => {
                return respond(
                    &mut stream,
                    "304 Not Modified",
                    "application/json",
                    "",
                    Some(&tag),
                );
            }
            Err(error) => {
                return respond(
                    &mut stream,
                    "404 Not Found",
                    "text/plain",
                    &error.to_string(),
                    None,
                );
            }
        }
    }
    respond(
        &mut stream,
        "404 Not Found",
        "text/plain",
        "Page not found",
        None,
    )
}

fn query_value<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn apply_preview(document: &mut Document, query: &str) -> Result<()> {
    if let Some(frame) = query_value(query, "flow") {
        if query_value(query, "preview").is_some() {
            return Err("Flow frames and local preview paths are mutually exclusive".into());
        }
        let revision = query_value(query, "revision").and_then(|value| value.parse::<u64>().ok());
        if revision != Some(document.revision) {
            return Err("The saved workspace changed; reload the flow board".into());
        }
        return atk_ui::flow::apply(document, frame);
    }
    let Some(path) = query_value(query, "preview") else {
        return Ok(());
    };
    let revision = query_value(query, "revision").and_then(|value| value.parse::<u64>().ok());
    if revision != Some(document.revision) {
        return Err("The saved workspace changed; start the preview again".into());
    }
    for (index, target) in path.split(',').enumerate() {
        let valid = !target.is_empty()
            && target.len() <= 150
            && target.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b":_-".contains(&byte)
            });
        if index >= 32 || !valid {
            return Err("Invalid viewer preview path".into());
        }
        preview::apply(document, &serde_json::json!({"target":target}))?;
    }
    Ok(())
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    kind: &str,
    value: &str,
    tag: Option<&str>,
) -> Result<()> {
    respond_bytes(
        stream,
        status,
        &format!("{kind}; charset=utf-8"),
        value.as_bytes(),
        tag,
        None,
    )
}

fn viewer_generation() -> &'static str {
    static GENERATION: OnceLock<String> = OnceLock::new();
    GENERATION.get_or_init(|| {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}-{}", std::process::id(), started.as_nanos())
    })
}

fn respond_bytes(
    stream: &mut TcpStream,
    status: &str,
    kind: &str,
    value: &[u8],
    tag: Option<&str>,
    timing: Option<&str>,
) -> Result<()> {
    let length = value.len();
    let policy = render::POLICY;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {length}\r\nConnection: close\r\nCache-Control: no-cache\r\nContent-Security-Policy: {policy} frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\n"
    )?;
    writeln!(stream, "X-UI-Viewer-Generation: {}\r", viewer_generation())?;
    if let Some(tag) = tag {
        write!(stream, "ETag: {tag}\r\n")?;
    }
    if let Some(timing) = timing {
        write!(stream, "Server-Timing: {timing}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(value)?;
    Ok(())
}
