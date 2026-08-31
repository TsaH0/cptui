//! Competitive Companion compatible local HTTP server.
//!
//! The Competitive Companion browser extension sends one HTTP `POST /` per
//! problem to `http://localhost:<port>/` with `Content-Type: application/json`.
//! The body is a JSON "Task" object whose fields are (per the extension's
//! `src/models/Task.ts`):
//!
//! ```json
//! {
//!   "name": "A. Example",
//!   "group": "Codeforces - Codeforces Round 999",
//!   "url": "https://codeforces.com/contest/999/problem/A",
//!   "interactive": false,
//!   "memoryLimit": 256,
//!   "timeLimit": 2000,
//!   "testType": "single",
//!   "input": { "type": "stdin" },
//!   "output": { "type": "stdout" },
//!   "languages": { "java": { "mainClass": "Main", "taskClass": "A" } },
//!   "tests": [ { "input": "5\n", "output": "5\n" } ],
//!   "batch": { "id": "<uuid>", "size": 4 }
//! }
//! ```
//!
//! For a contest, the extension sends N separate POST requests sharing the
//! same `batch.id` with `batch.size` = N. We group arrivals by `batch.id` and
//! surface progress to the UI.

use crate::model::{ContestMeta, ProblemMeta, Testcase};
use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN,
        },
        HeaderMap, HeaderName, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// What the Competitive Companion sends for one problem.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct CompanionTask {
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub memoryLimit: Option<u64>,
    #[serde(default)]
    pub timeLimit: Option<u64>,
    #[serde(default)]
    pub testType: Option<String>,
    #[serde(default)]
    pub tests: Vec<CompanionTest>,
    #[serde(default)]
    pub batch: CompanionBatch,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CompanionTest {
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub output: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CompanionBatch {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub size: u64,
}

/// Current Codeforces problem, supplied by cptui's browser extension.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolutionProblem {
    #[serde(default = "default_solution_platform")]
    pub platform: String,
    pub contest_id: String,
    pub index: String,
    #[serde(default)]
    pub title: String,
}

/// One downloaded source file. Extension has already selected its suffix.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolutionFile {
    pub handle: String,
    pub submission_id: u64,
    pub ext: String,
    #[serde(default)]
    pub source: String,
    /// Present only for VJudge image-only source responses; OCR stays server-side.
    #[serde(default)]
    pub image_base64: Option<String>,
    #[serde(default)]
    pub image_mime: Option<String>,
    /// OCR failure is saved beside the original image instead of writing empty code.
    #[serde(default)]
    pub ocr_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolutionBatch {
    pub problem: SolutionProblem,
    pub files: Vec<SolutionFile>,
}

/// Extension stage, forwarded into cptui's visible footer.
fn default_solution_platform() -> String {
    "Codeforces".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolutionProgress {
    pub problem: SolutionProblem,
    pub stage: String,
    #[serde(default)]
    pub completed: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub message: String,
}

/// Events sent from the HTTP server to the app.
#[derive(Debug, Clone)]
pub enum CompanionEvent {
    /// A single problem arrived. `batch_size` lets the UI show progress.
    Problem {
        task: CompanionTask,
        batch_size: u64,
        index: u64,
    },
    /// Browser extension says what it is currently doing.
    SolutionProgress(SolutionProgress),
    /// Browser extension finished a source batch ready for disk.
    Solutions(SolutionBatch),
}

/// Shared server state: an mpsc sender to push events to the app and a running
/// tally of batch arrivals to compute per-batch progress.
#[derive(Clone)]
struct ServerState {
    tx: mpsc::UnboundedSender<CompanionEvent>,
    batch_counts: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
}

/// Start the Competitive Companion HTTP server in the background.
///
/// Returns immediately; the server runs on the tokio runtime until the process
/// exits. The returned channel receives one event per imported problem.
pub fn spawn(host: &str, port: u16) -> Result<mpsc::UnboundedReceiver<CompanionEvent>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let state = ServerState {
        tx,
        batch_counts: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    let app = Router::new()
        .route("/", post(handle_task))
        .route(
            "/solutions",
            post(handle_solutions).options(solution_options),
        )
        .route(
            "/solutions/progress",
            post(handle_solution_progress).options(solution_options),
        )
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state);
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind address {host}:{port}: {e}"))?;

    // Bind before returning so cptui can show a truthful listener state and
    // fail startup instead of silently running without a browser receiver.
    let listener = std::net::TcpListener::bind(addr)
        .map_err(|e| anyhow::anyhow!("cannot bind companion server {addr}: {e}"))?;
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;

    let handle = tokio::runtime::Handle::current();
    handle.spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[companion] server error: {e}");
        }
    });

    Ok(rx)
}

/// `cptui` listens only on localhost. With no pairing token, accept only the
/// installed extension or a Codeforces page; reject arbitrary web origins.
fn solution_cors(headers: &HeaderMap) -> Result<HeaderMap, StatusCode> {
    let origin = headers
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .filter(|o| {
            *o == "https://codeforces.com"
                || *o == "https://www.codeforces.com"
                || *o == "https://atcoder.jp"
                || *o == "https://vjudge.net"
                || (o.starts_with("https://")
                    && (o.ends_with(".codeforces.com")
                        || o.ends_with(".atcoder.jp")
                        || o.ends_with(".vjudge.net")))
                || o.starts_with("chrome-extension://")
        })
        .ok_or(StatusCode::FORBIDDEN)?;
    let mut out = HeaderMap::new();
    out.insert(
        ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_str(origin).unwrap(),
    );
    out.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    out.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    out.insert(
        HeaderName::from_static("access-control-allow-private-network"),
        HeaderValue::from_static("true"),
    );
    Ok(out)
}

async fn solution_options(headers: HeaderMap) -> Response {
    match solution_cors(&headers) {
        Ok(cors) => (StatusCode::NO_CONTENT, cors).into_response(),
        Err(code) => code.into_response(),
    }
}

async fn resolve_ocr_sources(batch: &mut SolutionBatch) -> usize {
    let mut failed = 0;
    for file in &mut batch.files {
        if !file.source.trim().is_empty() {
            continue;
        }
        let result = match file.image_base64.as_deref() {
            Some(image) => {
                mistral_ocr(image, file.image_mime.as_deref().unwrap_or("image/png")).await
            }
            None => Err("source has neither text nor image".to_string()),
        };
        match result {
            Ok(text) if !text.trim().is_empty() => file.source = text,
            Ok(_) => {
                file.ocr_error = Some("Mistral OCR returned no text".to_string());
                failed += 1;
            }
            Err(error) => {
                file.ocr_error = Some(error);
                failed += 1;
            }
        }
    }
    failed
}

async fn mistral_ocr(image_base64: &str, mime: &str) -> Result<String, String> {
    let key = mistral_key(std::env::var("MISTRAL_API_KEY").ok())?;
    let body = serde_json::json!({
        "model": "mistral-ocr-latest",
        "document": {
            "type": "image_url",
            "image_url": format!("data:{mime};base64,{image_base64}"),
        },
        "include_image_base64": false,
    });
    let response = reqwest::Client::new()
        .post("https://api.mistral.ai/v1/ocr")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Mistral OCR request failed: {e}"))?;
    let status = response.status();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Mistral OCR response was invalid JSON: {e}"))?;
    if !status.is_success() {
        let detail = value["message"].as_str().unwrap_or("request rejected");
        return Err(format!("Mistral OCR HTTP {status}: {detail}"));
    }
    let markdown = value["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|page| page["markdown"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(strip_ocr_fence(&markdown))
}

fn mistral_key(value: Option<String>) -> Result<String, String> {
    value
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| "MISTRAL_API_KEY is not set in the cptui process".to_string())
}

fn strip_ocr_fence(text: &str) -> String {
    let text = text.trim();
    if !text.starts_with("```") {
        return text.to_string();
    }
    let mut lines = text.lines();
    lines.next();
    let mut body: Vec<&str> = lines.collect();
    if body
        .last()
        .is_some_and(|line| line.trim_start().starts_with("```"))
    {
        body.pop();
    }
    body.join("\n")
}

async fn handle_solutions(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let cors = match solution_cors(&headers) {
        Ok(cors) => cors,
        Err(code) => return code.into_response(),
    };
    let mut batch: SolutionBatch = match serde_json::from_slice::<SolutionBatch>(&body) {
        Ok(batch) if !batch.files.is_empty() => batch,
        Ok(_) => {
            return (StatusCode::BAD_REQUEST, cors, "no source files").into_response();
        }
        Err(e) => {
            return (StatusCode::BAD_REQUEST, cors, format!("invalid JSON: {e}")).into_response();
        }
    };
    let ocr_failed = resolve_ocr_sources(&mut batch).await;
    let count = batch.files.len();
    let _ = state.tx.send(CompanionEvent::Solutions(batch));
    (
        StatusCode::ACCEPTED,
        cors,
        Json(serde_json::json!({ "accepted": count, "ocr_failed": ocr_failed })),
    )
        .into_response()
}

async fn handle_solution_progress(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let cors = match solution_cors(&headers) {
        Ok(cors) => cors,
        Err(code) => return code.into_response(),
    };
    let progress: SolutionProgress = match serde_json::from_slice(&body) {
        Ok(progress) => progress,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, cors, format!("invalid JSON: {e}")).into_response();
        }
    };
    let _ = state.tx.send(CompanionEvent::SolutionProgress(progress));
    (StatusCode::NO_CONTENT, cors).into_response()
}

async fn handle_task(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Optionally verify the Competitive Companion header (not all clients set it).
    let _ = headers.get("x-competitive-companion");

    let task: CompanionTask = match serde_json::from_slice(&body) {
        Ok(t) => t,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("invalid JSON: {e}"));
        }
    };

    let batch_id = if task.batch.id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        task.batch.id.clone()
    };
    let batch_size = if task.batch.size == 0 {
        1
    } else {
        task.batch.size
    };

    let index = {
        let mut counts = state.batch_counts.lock().unwrap();
        let c = counts.entry(batch_id.clone()).or_insert(0);
        *c += 1;
        *c
    };

    let event = CompanionEvent::Problem {
        task,
        batch_size,
        index,
    };
    let _ = state.tx.send(event);

    (StatusCode::OK, "OK".to_string())
}

/// Convert a Companion task into cptui's problem metadata + sample testcases.
pub fn task_to_problem(task: &CompanionTask) -> (ProblemMeta, Vec<Testcase>) {
    let id = derive_problem_id(&task.name);
    let meta = ProblemMeta {
        id,
        name: task.name.clone(),
        group: task.group.clone(),
        url: task.url.clone(),
        interactive: task.interactive,
        memory_limit_mb: task.memoryLimit.unwrap_or(0),
        time_limit_ms: task.timeLimit.unwrap_or(0),
        status: Default::default(),
        source: "main.cpp".to_string(),
        batch_id: task.batch.id.clone(),
    };
    let samples = task
        .tests
        .iter()
        .map(|t| Testcase::new_sample(t.input.clone(), t.output.clone()))
        .collect();
    (meta, samples)
}

/// Derive a short filesystem-safe problem id from a Companion name.
///
/// Companion names are typically like "A. Example Problem" or "Problem A. Foo".
/// We pull the leading letter/code if present, else a sanitized slug.
fn derive_problem_id(name: &str) -> String {
    let trimmed = name.trim();
    // Try to extract a leading "A." / "A " / "1." style code.
    if let Some(code) = extract_leading_code(trimmed) {
        return code;
    }
    let s = super::config::sanitize(trimmed);
    if s.len() > 24 {
        s.chars().take(24).collect()
    } else {
        s
    }
}

/// Pull a leading problem code like "A", "B", "1", "A1" from the start of a name.
fn extract_leading_code(name: &str) -> Option<String> {
    let mut chars = name.chars().peekable();
    // Skip nothing; the code is at the very start.
    let first = chars.peek()?.to_ascii_uppercase();
    if !first.is_ascii_alphanumeric() {
        return None;
    }
    let mut code = String::new();
    // Allow up to 3 alphanumeric chars forming the code (e.g. "A", "C1", "A2").
    while let Some(c) = chars.peek() {
        if c.is_ascii_alphanumeric() && code.len() < 3 {
            code.push(c.to_ascii_uppercase());
            chars.next();
        } else {
            break;
        }
    }
    // The next char must be a separator (".", " ", "-", ":") for this to count
    // as a code rather than part of a word.
    match chars.peek() {
        Some(c) if matches!(*c, '.' | ' ' | '-' | ':' | '\t') => Some(code),
        _ => None,
    }
}

/// Build a contest display name from a Companion group string, e.g.
/// "Codeforces - Codeforces Round 999" -> "Codeforces-Round-999".
pub fn contest_name_from_group(group: &str) -> String {
    if let Some(idx) = group.rfind(" - ") {
        let after = &group[idx + 3..];
        return super::config::sanitize(after);
    }
    super::config::sanitize(group)
}

/// Construct a [`ContestMeta`] with a fresh start timestamp.
pub fn contest_meta_for(name: &str, batch_id: &str) -> ContestMeta {
    ContestMeta {
        name: name.to_string(),
        batch_id: batch_id.to_string(),
        started_at: Some(chrono::Local::now()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header::ORIGIN, HeaderMap, HeaderValue};

    #[test]
    fn solutions_cors_allows_only_codeforces_or_extensions() {
        let mut headers = HeaderMap::new();
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://www.codeforces.com"),
        );
        assert!(solution_cors(&headers).is_ok());
        headers.insert(ORIGIN, HeaderValue::from_static("https://evil.example"));
        assert_eq!(solution_cors(&headers).unwrap_err(), StatusCode::FORBIDDEN);
        headers.insert(ORIGIN, HeaderValue::from_static("https://atcoder.jp"));
        assert!(solution_cors(&headers).is_ok());
        headers.insert(ORIGIN, HeaderValue::from_static("https://vjudge.net"));
        assert!(solution_cors(&headers).is_ok());
        headers.insert(ORIGIN, HeaderValue::from_static("chrome-extension://abc"));
        assert!(solution_cors(&headers).is_ok());
    }

    #[tokio::test]
    async fn solution_batch_enqueues_extension_payload() {
        use axum::{body::Bytes, extract::State};
        let (tx, mut rx) = mpsc::unbounded_channel();
        let state = ServerState {
            tx,
            batch_counts: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_static("chrome-extension://test"));
        let body = Bytes::from(
            r#"{"problem":{"platform":"Codeforces","contest_id":"4","index":"A","title":"Watermelon"},"files":[{"handle":"Benq","submission_id":1,"ext":"cpp","source":"int main(){}"}]}"#,
        );
        let response = handle_solutions(State(state), headers, body).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        match rx.recv().await.unwrap() {
            CompanionEvent::Solutions(batch) => {
                assert_eq!(batch.problem.contest_id, "4");
                assert_eq!(batch.files[0].handle, "Benq");
            }
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn ocr_key_and_fence_validation() {
        assert!(mistral_key(None).is_err());
        assert_eq!(
            strip_ocr_fence("```cpp\nint main() {}\n```"),
            "int main() {}"
        );
        assert_eq!(strip_ocr_fence("plain text"), "plain text");
    }

    #[test]
    fn parse_single_problem() {
        let payload = r#"{
            "name": "A. Example Problem",
            "group": "Codeforces - Codeforces Round 999",
            "url": "https://codeforces.com/contest/999/problem/A",
            "interactive": false,
            "memoryLimit": 256,
            "timeLimit": 2000,
            "testType": "single",
            "input": {"type": "stdin"},
            "output": {"type": "stdout"},
            "languages": {"java": {"mainClass": "Main", "taskClass": "A"}},
            "tests": [{"input": "5\n", "output": "5\n"}],
            "batch": {"id": "abc", "size": 1}
        }"#;
        let task: CompanionTask = serde_json::from_str(payload).unwrap();
        assert_eq!(task.name, "A. Example Problem");
        assert_eq!(task.memoryLimit, Some(256));
        assert_eq!(task.timeLimit, Some(2000));
        assert_eq!(task.tests.len(), 1);
        assert_eq!(task.batch.size, 1);
        let (meta, samples) = task_to_problem(&task);
        assert_eq!(meta.id, "A");
        assert_eq!(meta.time_limit_ms, 2000);
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn parse_batch() {
        let payload = r#"{
            "name": "B. Strange Permutation",
            "group": "Codeforces - Codeforces Round 999",
            "url": "https://codeforces.com/contest/999/problem/B",
            "tests": [],
            "batch": {"id": "abc", "size": 4}
        }"#;
        let task: CompanionTask = serde_json::from_str(payload).unwrap();
        assert_eq!(task.batch.size, 4);
        let (meta, _) = task_to_problem(&task);
        assert_eq!(meta.id, "B");
    }

    #[test]
    fn malformed_payload() {
        let bad = "not json at all";
        assert!(serde_json::from_str::<CompanionTask>(bad).is_err());
    }

    #[test]
    fn optional_missing_fields() {
        // Minimal payload with only required-ish fields; everything else optional.
        let payload = r#"{"name": "Problem X"}"#;
        let task: CompanionTask = serde_json::from_str(payload).unwrap();
        assert_eq!(task.name, "Problem X");
        assert_eq!(task.url, "");
        assert_eq!(task.memoryLimit, None);
        assert!(task.tests.is_empty());
        let (meta, samples) = task_to_problem(&task);
        assert_eq!(meta.id, "Problem-X");
        assert!(samples.is_empty());
        assert_eq!(meta.time_limit_ms, 0);
    }

    #[test]
    fn contest_name_from_group_codeforces() {
        assert_eq!(
            contest_name_from_group("Codeforces - Codeforces Round 999"),
            "Codeforces-Round-999"
        );
    }
}
