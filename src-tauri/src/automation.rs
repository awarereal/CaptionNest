//! Test automation bridge — compiled only with `--features automation`, never in
//! release packages.
//!
//! When the app runs with CAPTIONNEST_AUTOMATION_PORT=<port>, it listens on
//! 127.0.0.1:<port> for:
//!   POST /eval      body = JavaScript function body (may `await`, use `return`);
//!                   runs inside the real app's web view, responds {"ok","value"}
//!   POST /snapshot  body = output .png path; captures the web view (macOS)
//! This lets tests drive the real interface and take screenshots without the
//! Screen Recording permission.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Listener, Manager};

type Pending = Arc<Mutex<HashMap<String, mpsc::Sender<(bool, String)>>>>;

pub fn start(app: &AppHandle) {
    let Ok(port) = std::env::var("CAPTIONNEST_AUTOMATION_PORT") else { return };
    let pending: Pending = Default::default();
    let p = pending.clone();
    app.listen("automation-result", move |ev| {
        let v: serde_json::Value = serde_json::from_str(ev.payload()).unwrap_or_default();
        let id = v["id"].as_str().unwrap_or_default().to_string();
        if let Some(tx) = p.lock().unwrap().remove(&id) {
            let _ = tx.send((v["ok"].as_bool().unwrap_or(false), v["value"].as_str().unwrap_or("null").to_string()));
        }
    });
    let app = app.clone();
    std::thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{port}")).expect("automation port");
        log::info!("automation bridge listening on 127.0.0.1:{port}");
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            let pending = pending.clone();
            std::thread::spawn(move || {
                let _ = handle(&app, &pending, stream);
            });
        }
    });
}

/// Largest request body (a test script or a path); bigger ones are refused.
const MAX_BODY: usize = 1024 * 1024;

fn handle(app: &AppHandle, pending: &Pending, mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut len = 0usize;
    let mut from_browser = false;
    let mut host_ok = false;
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        if h == "\r\n" || h.is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        // Browsers always send Origin (or Sec-Fetch-*) with cross-site requests; the test
        // scripts never do. A web page must not be able to drive the app.
        if lower.starts_with("origin:") || lower.starts_with("sec-fetch-") || lower.starts_with("referer:") {
            from_browser = true;
        }
        // DNS rebinding: only a loopback Host is accepted.
        if let Some(v) = lower.strip_prefix("host:") {
            let host = v.trim().rsplit_once(':').map(|(h, _)| h).unwrap_or(v.trim());
            host_ok = matches!(host, "127.0.0.1" | "localhost" | "[::1]");
        }
    }
    if from_browser || !host_ok || len > MAX_BODY {
        let _ = write!(stream, "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Ok(());
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let body = String::from_utf8_lossy(&body).into_owned();

    let (status, json) = match (method.as_str(), path.as_str()) {
        ("POST", "/eval") => match eval(app, pending, &body) {
            Ok((ok, value)) => (200, serde_json::json!({ "ok": ok, "value": value }).to_string()),
            Err(e) => (500, serde_json::json!({ "ok": false, "value": e }).to_string()),
        },
        ("POST", "/snapshot") => match snapshot(app, body.trim()) {
            Ok(()) => (200, r#"{"ok":true}"#.to_string()),
            Err(e) => (500, serde_json::json!({ "ok": false, "value": e }).to_string()),
        },
        // Body "1360x860": resize the window (logical pixels), to check layouts at any size.
        ("POST", "/resize") => match resize(app, body.trim()) {
            Ok(()) => (200, r#"{"ok":true}"#.to_string()),
            Err(e) => (500, serde_json::json!({ "ok": false, "value": e }).to_string()),
        },
        _ => (404, r#"{"ok":false,"value":"not found"}"#.to_string()),
    };
    write!(
        stream,
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
        json.len()
    )
}

fn resize(app: &AppHandle, spec: &str) -> Result<(), String> {
    let (w, h) = spec.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let (w, h): (f64, f64) = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
    let wv = app.get_webview_window("main").ok_or("no main window")?;
    wv.set_size(tauri::LogicalSize::new(w, h)).map_err(|e| e.to_string())
}

fn eval(app: &AppHandle, pending: &Pending, js: &str) -> Result<(bool, String), String> {
    let wv = app.get_webview_window("main").ok_or("no main window")?;
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel();
    pending.lock().unwrap().insert(id.clone(), tx);
    let script = format!(
        r#"(async () => {{
  const __id = {id:?};
  let ok = true, value;
  try {{ value = await (async () => {{ {js}
  }})(); }} catch (e) {{ ok = false; value = String(e && e.message ? e.message + "\n" + (e.stack || "") : e); }}
  let s; try {{ s = JSON.stringify(value === undefined ? null : value); }} catch {{ s = String(value); }}
  await window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {{ event: "automation-result", payload: {{ id: __id, ok, value: s }} }});
}})();"#
    );
    wv.eval(&script).map_err(|e| e.to_string())?;
    let result = rx.recv_timeout(Duration::from_secs(300)).map_err(|_| "timed out".to_string());
    pending.lock().unwrap().remove(&id);
    result
}

#[cfg(target_os = "macos")]
fn snapshot(app: &AppHandle, out: &str) -> Result<(), String> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;

    let wv = app.get_webview_window("main").ok_or("no main window")?;
    let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
    wv.with_webview(move |pv| unsafe {
        let wk: &WKWebView = &*(pv.inner() as *const WKWebView);
        let tx = tx.clone();
        let block = RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
            let png = (|| {
                let img = img.as_ref()?;
                let tiff = img.TIFFRepresentation()?;
                let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
                let props = NSDictionary::new();
                let data = rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props)?;
                Some(data.to_vec())
            })();
            let _ = tx.send(png);
        });
        wk.takeSnapshotWithConfiguration_completionHandler(None, &block);
    })
    .map_err(|e| e.to_string())?;
    let bytes = rx.recv_timeout(Duration::from_secs(15)).map_err(|_| "snapshot timed out")?.ok_or("snapshot failed")?;
    std::fs::write(out, bytes).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn snapshot(_app: &AppHandle, _out: &str) -> Result<(), String> {
    Err("snapshots are only implemented on macOS".into())
}
