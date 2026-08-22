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
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
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

/// Events sent from the HTTP server to the app.
#[derive(Debug, Clone)]
pub enum CompanionEvent {
    /// A single problem arrived. `batch_size` lets the UI show progress.
    Problem {
        task: CompanionTask,
        batch_size: u64,
        index: u64,
    },
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
        .with_state(state);
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind address {host}:{port}: {e}"))?;

    let handle = tokio::runtime::Handle::current();
    handle.spawn(async move {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[companion] failed to bind {addr}: {e}");
                return;
            }
        };
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[companion] server error: {e}");
        }
    });

    Ok(rx)
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
