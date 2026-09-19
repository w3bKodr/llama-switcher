//! Versioned, locally graded professional benchmark cases.
//!
//! The catalog is intentionally kept in Rust.  The UI receives only the safe
//! summary metadata; test inputs and expected answers stay on the backend so
//! the benchmark can grade submissions without leaking its held-out cases.

use boa_engine::{Context, Source};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SUITE_ID: &str = "professional-js";
pub const SUITE_VERSION: u32 = 2;

#[path = "benchmark_cases_v2.rs"]
mod benchmark_cases_v2;

#[derive(Clone)]
pub struct TestCase {
    pub id: String,
    pub label: String,
    pub input: Value,
    pub expected: Value,
}

#[derive(Clone)]
pub struct BenchmarkCase {
    pub id: String,
    pub title: String,
    pub description: String,
    pub difficulty: String,
    pub weight: f64,
    pub function_name: String,
    pub prompt: String,
    #[allow(dead_code)]
    pub reference_solution: String,
    pub tests: Vec<TestCase>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfessionalBenchmarkSummary {
    pub id: String,
    pub suite_id: String,
    pub suite_version: u32,
    pub title: String,
    pub description: String,
    pub difficulty: String,
    pub weight: f64,
    pub prompt: String,
    pub function_signature: String,
    pub public_test_count: usize,
    pub total_test_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GradeResult {
    pub benchmark_id: String,
    pub suite_version: u32,
    pub status: String,
    pub score: f64,
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
    pub feedback: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct GraderRequest {
    benchmark_id: String,
    markdown: String,
    timeout_seconds: u64,
}

/// Entry point used by a short-lived copy of llama-switcher.exe. Keeping Boa
/// in a separate process means hostile or pathological generated JavaScript
/// cannot abort the desktop application.
pub fn run_grader_mode() -> Option<i32> {
    let mut args = std::env::args();
    let _ = args.next();
    if args.next().as_deref() != Some("--benchmark-grader") {
        return None;
    }
    let Some(request_path) = args.next() else {
        return Some(2);
    };
    let Some(result_path) = args.next() else {
        return Some(2);
    };
    let request: GraderRequest = match std::fs::File::open(request_path)
        .ok()
        .and_then(|file| serde_json::from_reader(file).ok())
    {
        Some(value) => value,
        None => return Some(2),
    };
    let Some(case) = catalog()
        .into_iter()
        .find(|case| case.id == request.benchmark_id)
    else {
        return Some(3);
    };
    let result = grade_submission_with_timeout(&case, &request.markdown, request.timeout_seconds);
    if std::fs::File::create(result_path)
        .ok()
        .and_then(|file| serde_json::to_writer(file, &result).ok())
        .is_none()
    {
        return Some(4);
    }
    Some(0)
}

pub fn grade_submission_isolated(
    case: &BenchmarkCase,
    markdown: &str,
    grading_timeout_seconds: u64,
) -> GradeResult {
    let failure = |status: &str, feedback: String| GradeResult {
        benchmark_id: case.id.clone(),
        suite_version: SUITE_VERSION,
        status: status.into(),
        score: 0.0,
        passed: 0,
        failed: case.tests.len(),
        total: case.tests.len(),
        feedback: vec![feedback],
    };
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return failure(
                "grader_error",
                format!("Could not locate isolated grader: {error}"),
            )
        }
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "llama-switcher-grader-{}-{nonce}",
        std::process::id()
    ));
    let request_path = base.with_extension("request.json");
    let result_path = base.with_extension("result.json");
    let request = GraderRequest {
        benchmark_id: case.id.clone(),
        markdown: markdown.into(),
        timeout_seconds: grading_timeout_seconds,
    };
    if std::fs::File::create(&request_path)
        .ok()
        .and_then(|file| serde_json::to_writer(file, &request).ok())
        .is_none()
    {
        return failure(
            "grader_error",
            "Could not create isolated grader request.".into(),
        );
    }
    let mut command = Command::new(exe);
    command
        .arg("--benchmark-grader")
        .arg(&request_path)
        .arg(&result_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&request_path);
            return failure(
                "grader_error",
                format!("Could not start isolated grader: {error}"),
            );
        }
    };
    let deadline = Instant::now() + Duration::from_secs(grading_timeout_seconds.max(1) + 5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let _ = std::fs::remove_file(&request_path);
                    let _ = std::fs::remove_file(&result_path);
                    return failure(
                        "grader_crash",
                        format!("The isolated JavaScript grader exited unexpectedly ({status})."),
                    );
                }
                let result = std::fs::File::open(&result_path)
                    .map_err(|error| error.to_string())
                    .and_then(|file| {
                        serde_json::from_reader(file).map_err(|error| error.to_string())
                    })
                    .unwrap_or_else(|error| {
                        failure(
                            "grader_error",
                            format!("Invalid isolated grader result: {error}"),
                        )
                    });
                let _ = std::fs::remove_file(&request_path);
                let _ = std::fs::remove_file(&result_path);
                return result;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&request_path);
                let _ = std::fs::remove_file(&result_path);
                return failure(
                    "grader_timeout",
                    format!(
                        "JavaScript grading exceeded {} seconds.",
                        grading_timeout_seconds.max(1)
                    ),
                );
            }
            Err(error) => {
                let _ = std::fs::remove_file(&request_path);
                let _ = std::fs::remove_file(&result_path);
                return failure(
                    "grader_error",
                    format!("Could not monitor isolated grader: {error}"),
                );
            }
        }
    }
}

pub fn catalog() -> Vec<BenchmarkCase> {
    let mut cases = vec![compact_ranges_case()];
    cases.extend(benchmark_cases_v2::easy_cases());
    cases.push(merge_intervals_case());
    cases.extend(benchmark_cases_v2::medium_cases());
    cases.push(build_batches_case());
    cases.extend(benchmark_cases_v2::hard_cases());
    cases
}

pub fn summaries() -> Vec<ProfessionalBenchmarkSummary> {
    catalog()
        .into_iter()
        .map(|case| ProfessionalBenchmarkSummary {
            id: case.id,
            suite_id: SUITE_ID.into(),
            suite_version: SUITE_VERSION,
            title: case.title,
            description: case.description,
            difficulty: case.difficulty,
            weight: case.weight,
            prompt: case.prompt,
            function_signature: format!("{}(input)", case.function_name),
            public_test_count: 2,
            total_test_count: case.tests.len(),
        })
        .collect()
}

/// Grade exactly one fenced JavaScript submission against a catalog case.
/// Boa has no host bindings in this configuration, and runtime limits protect
/// the evaluator from runaway loops/recursion while keeping grading in-process.
/// Grade with a bounded VM loop budget derived from the user-visible grading
/// limit. Boa has no host bindings and enforces this budget inside every loop.
pub fn grade_submission_with_timeout(
    case: &BenchmarkCase,
    markdown: &str,
    grading_timeout_seconds: u64,
) -> GradeResult {
    let mut result = GradeResult {
        benchmark_id: case.id.clone(),
        suite_version: SUITE_VERSION,
        total: case.tests.len(),
        ..GradeResult::default()
    };

    let blocks = extract_code_blocks(markdown);
    if blocks.len() != 1 {
        result.status = "invalid_response".into();
        result.feedback.push(format!(
            "Expected exactly one fenced JavaScript block; received {}.",
            blocks.len()
        ));
        result.failed = result.total;
        return result;
    }
    let (language, code) = &blocks[0];
    let language = language.trim().to_ascii_lowercase();
    if language != "javascript" && language != "js" {
        result.status = "invalid_response".into();
        result
            .feedback
            .push("The submission must use a ```javascript code block.".into());
        result.failed = result.total;
        return result;
    }
    if code.len() > 128 * 1024 {
        result.status = "invalid_response".into();
        result
            .feedback
            .push("Submission exceeds the 128 KiB limit.".into());
        result.failed = result.total;
        return result;
    }
    if let Some(forbidden) = forbidden_construct(code) {
        result.status = "invalid_response".into();
        result.feedback.push(format!(
            "The submission uses a disallowed host or dynamic-code construct: {}.",
            forbidden
        ));
        result.failed = result.total;
        return result;
    }

    // Parse once before running tests so syntax errors are reported distinctly.
    let mut syntax_context = Context::default();
    set_runtime_limits(&mut syntax_context, grading_timeout_seconds);
    if let Err(error) = syntax_context.eval(Source::from_bytes(code.as_bytes())) {
        result.status = "syntax_error".into();
        result.feedback.push(format!(
            "The submission could not be parsed: {}",
            format_js_error(&error, &mut syntax_context)
        ));
        result.failed = result.total;
        return result;
    }

    for test in &case.tests {
        match run_test(case, code, test, grading_timeout_seconds) {
            Ok(()) => result.passed += 1,
            Err(message) => {
                result.failed += 1;
                if result.feedback.len() < 6 {
                    result
                        .feedback
                        .push(format!("{} ({}): {}", test.id, test.label, message));
                }
            }
        }
    }
    result.status = if result.failed == 0 {
        "passed"
    } else {
        "failed"
    }
    .into();
    result.score = if result.total == 0 {
        0.0
    } else {
        (result.passed as f64 / result.total as f64) * 100.0
    };
    result
}

fn set_runtime_limits(context: &mut Context, grading_timeout_seconds: u64) {
    let limits = context.runtime_limits_mut();
    let loop_budget = grading_timeout_seconds.clamp(1, 300).saturating_mul(8_333);
    limits.set_loop_iteration_limit(loop_budget.max(8_333));
    limits.set_recursion_limit(256);
    limits.set_stack_size_limit(4096);
}

fn run_test(
    case: &BenchmarkCase,
    code: &str,
    test: &TestCase,
    grading_timeout_seconds: u64,
) -> Result<(), String> {
    let mut context = Context::default();
    set_runtime_limits(&mut context, grading_timeout_seconds);
    let input = serde_json::to_string(&test.input).map_err(|e| e.to_string())?;
    let wrapper = format!(
        "{}\nconst __benchmarkInput = {};\nconst __benchmarkBefore = JSON.stringify(__benchmarkInput);\nconst __benchmarkResult = {}(__benchmarkInput);\nJSON.stringify({{result: __benchmarkResult, before: __benchmarkBefore, after: JSON.stringify(__benchmarkInput)}})",
        code, input, case.function_name
    );
    let value = context
        .eval(Source::from_bytes(wrapper.as_bytes()))
        .map_err(|error| format_js_error(&error, &mut context))?;
    let encoded = value
        .to_string(&mut context)
        .map_err(|error| format_js_error(&error, &mut context))?
        .to_std_string_escaped();
    let envelope: Value = serde_json::from_str(&encoded)
        .map_err(|_| "the function did not return JSON-serializable data".to_string())?;
    if envelope["before"] != envelope["after"] {
        return Err("the function mutated its input".into());
    }
    let actual = envelope
        .get("result")
        .ok_or_else(|| "the function returned undefined".to_string())?;
    if actual != &test.expected {
        return Err("returned value did not match the expected result".into());
    }
    Ok(())
}

fn format_js_error(error: &boa_engine::JsError, context: &mut Context) -> String {
    error
        .to_opaque(context)
        .to_string(context)
        .map(|value| value.to_std_string_escaped())
        .unwrap_or_else(|_| "JavaScript evaluation failed".into())
}

fn forbidden_construct(code: &str) -> Option<&'static str> {
    let lowered = code.to_ascii_lowercase();
    let identifiers = [
        ("import", "import"),
        ("require", "require"),
        ("process", "process"),
        ("deno", "Deno"),
        ("fetch", "fetch"),
        ("xmlhttprequest", "XMLHttpRequest"),
        ("websocket", "WebSocket"),
        ("__dirname", "__dirname"),
        ("__filename", "__filename"),
        ("readfile", "readFile"),
        ("writefile", "writeFile"),
        ("eval", "eval"),
    ];
    if let Some((_, name)) = identifiers
        .into_iter()
        .find(|(identifier, _)| contains_identifier(&lowered, identifier))
    {
        return Some(name);
    }

    contains_identifier_sequence(&lowered, "new", "function").then_some("Function constructor")
}

fn contains_identifier(code: &str, identifier: &str) -> bool {
    code.match_indices(identifier).any(|(index, _)| {
        let before = code[..index].chars().next_back();
        let after = code[index + identifier.len()..].chars().next();
        !before.is_some_and(is_identifier_continue) && !after.is_some_and(is_identifier_continue)
    })
}

fn contains_identifier_sequence(code: &str, first: &str, second: &str) -> bool {
    code.match_indices(first).any(|(index, _)| {
        let before = code[..index].chars().next_back();
        if before.is_some_and(is_identifier_continue) {
            return false;
        }
        let remainder = &code[index + first.len()..];
        let trimmed = remainder.trim_start();
        let Some(after_second) = trimmed.strip_prefix(second) else {
            return false;
        };
        !after_second
            .chars()
            .next()
            .is_some_and(is_identifier_continue)
    })
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '$'
}

fn extract_code_blocks(markdown: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut language = String::new();
    let mut body = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                blocks.push((language.clone(), body.clone()));
                language.clear();
                body.clear();
                in_block = false;
            } else {
                in_block = true;
                language = trimmed.trim_start_matches('`').trim().to_string();
            }
        } else if in_block {
            body.push_str(line);
            body.push('\n');
        }
    }
    blocks
}

fn test(id: &str, label: &str, input: Value, expected: Value) -> TestCase {
    TestCase {
        id: id.into(),
        label: label.into(),
        input,
        expected,
    }
}

fn compact_ranges_case() -> BenchmarkCase {
    BenchmarkCase {
        id: "professional-js-v2-easy-01-compact-ranges".into(),
        title: "Compact integer ranges".into(),
        description: "Normalize unsorted integers into sorted, compact consecutive ranges.".into(),
        difficulty: "easy".into(),
        weight: 0.0333333333,
        function_name: "compactRanges".into(),
        prompt: r#"Implement compactRanges(values), which accepts an array of signed integers that may be unsorted or duplicated. Return a new array sorted numerically. Consecutive runs of two or more integers should be represented as "start-end"; a run of one integer should be represented as that integer's decimal string. Do not mutate the input. Return exactly one fenced javascript code block containing the function."#.into(),
        reference_solution: r#"__BT__javascript
function compactRanges(values) {
  const sorted = [...new Set(values)].sort((a, b) => a - b);
  const result = [];
  for (let i = 0; i < sorted.length; i++) {
    let start = sorted[i], end = start;
    while (i + 1 < sorted.length && sorted[i + 1] === end + 1) {
      end = sorted[++i];
    }
    result.push(start === end ? String(start) : String(start) + "-" + String(end));
  }
  return result;
}
__BT__"#.replace("__BT__", "\u{60}\u{60}\u{60}").into(),
        tests: vec![
            test("easy-1", "mixed unsorted values", json!([3, 2, 1, 7, 5, 8, 8]), json!(["1-3", "5", "7-8"])),
            test("easy-2", "empty input", json!([]), json!([])),
            test("easy-3", "negative values", json!([-3, -2, -1, 1, 3, 4]), json!(["-3--1", "1", "3-4"])),
            test("easy-4", "duplicates and gaps", json!([1, 1, 2, 4, 6, 7, 8, 10]), json!(["1-2", "4", "6-8", "10"])),
            test("easy-5", "single value", json!([42]), json!(["42"])),
            test("easy-6", "negative gap", json!([-2, 0, 1, 2, 4]), json!(["-2", "0-2", "4"])),
        ],
    }
}

fn merge_intervals_case() -> BenchmarkCase {
    BenchmarkCase {
        id: "professional-js-v2-medium-01-merge-intervals".into(),
        title: "Merge time intervals".into(),
        description: "Normalize, sort, and merge overlapping or touching half-open intervals.".into(),
        difficulty: "medium".into(),
        weight: 0.05,
        function_name: "mergeIntervals".into(),
        prompt: r#"Implement mergeIntervals(intervals). Treat each pair as a half-open interval [start, end). Normalize reversed endpoints, discard zero-length intervals, sort by start then end, and merge overlapping or touching intervals. Return a new array and do not mutate the input. Return exactly one fenced javascript code block containing the function."#.into(),
        reference_solution: r#"__BT__javascript
function mergeIntervals(intervals) {
  const sorted = intervals
    .map(([a, b]) => (a <= b ? [a, b] : [b, a]))
    .filter(([a, b]) => a !== b)
    .sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const result = [];
  for (const [start, end] of sorted) {
    const last = result[result.length - 1];
    if (last && start <= last[1]) last[1] = Math.max(last[1], end);
    else result.push([start, end]);
  }
  return result;
}
__BT__"#.replace("__BT__", "\u{60}\u{60}\u{60}").into(),
        tests: vec![
            test("medium-1", "overlapping and unsorted", json!([[5, 7], [1, 3], [2, 4]]), json!([[1, 4], [5, 7]])),
            test("medium-2", "touching intervals", json!([[1, 2], [2, 3], [4, 5], [5, 5], [6, 8]]), json!([[1, 3], [4, 5], [6, 8]])),
            test("medium-3", "reversed endpoints", json!([[5, 1], [10, 12]]), json!([[1, 5], [10, 12]])),
            test("medium-4", "nested ranges", json!([[1, 10], [2, 3], [4, 8]]), json!([[1, 10]])),
            test("medium-5", "negative coordinates", json!([[-5, -2], [-3, 1], [4, 4]]), json!([[-5, 1]])),
            test("medium-6", "empty input", json!([]), json!([])),
        ],
    }
}

fn build_batches_case() -> BenchmarkCase {
    BenchmarkCase {
        id: "professional-js-v2-hard-01-build-batches".into(),
        title: "Plan dependency build batches".into(),
        description: "Produce deterministic parallel build batches and identify blocked packages.".into(),
        difficulty: "hard".into(),
        weight: 0.0833333333,
        function_name: "planBuilds".into(),
        prompt: r#"Implement planBuilds(packages). Each package has a unique name and an array of dependency names. Return { batches, blocked }. A batch contains every package whose dependencies have already completed, sorted lexically. Continue until no more packages can be scheduled. A package with a missing dependency is blocked, as is every package that depends directly or indirectly on a blocked package or a dependency cycle. Return blocked sorted lexically. Treat duplicate dependency entries as one dependency and do not mutate the input. Return exactly one fenced javascript code block containing the function."#.into(),
        reference_solution: r#"__BT__javascript
function planBuilds(packages) {
  const map = new Map(packages.map((pkg) => [pkg.name, { name: pkg.name, dependencies: [...new Set(pkg.dependencies)] }]));
  const names = [...map.keys()].sort();
  const state = new Map(names.map((name) => [name, 0]));
  const blocked = new Set();
  function visit(name, stack) {
    if (blocked.has(name)) return;
    if (state.get(name) === 1) {
      const index = stack.indexOf(name);
      for (const cycleName of stack.slice(index)) blocked.add(cycleName);
      return;
    }
    if (state.get(name) === 2) return;
    state.set(name, 1);
    stack.push(name);
    for (const dependency of map.get(name).dependencies) {
      if (!map.has(dependency)) blocked.add(name);
      else visit(dependency, stack);
    }
    stack.pop();
    state.set(name, 2);
  }
  names.forEach((name) => visit(name, []));
  let changed = true;
  while (changed) {
    changed = false;
    for (const name of names) {
      if (!blocked.has(name) && map.get(name).dependencies.some((dep) => blocked.has(dep))) {
        blocked.add(name);
        changed = true;
      }
    }
  }
  const remaining = new Set(names.filter((name) => !blocked.has(name)));
  const batches = [];
  while (remaining.size) {
    const batch = [...remaining].filter((name) => map.get(name).dependencies.every((dep) => !remaining.has(dep))).sort();
    if (!batch.length) break;
    batches.push(batch);
    batch.forEach((name) => remaining.delete(name));
  }
  remaining.forEach((name) => blocked.add(name));
  return { batches, blocked: [...blocked].sort() };
}
__BT__"#.replace("__BT__", "\u{60}\u{60}\u{60}").into(),
        tests: vec![
            test("hard-1", "linear and independent graph", json!([{"name":"app","dependencies":["lib"]},{"name":"lib","dependencies":[]},{"name":"cli","dependencies":["app"]},{"name":"docs","dependencies":[]}]), json!({"batches":[["docs","lib"],["app"],["cli"]],"blocked":[]})),
            test("hard-2", "diamond graph", json!([{"name":"top","dependencies":["left","right"]},{"name":"left","dependencies":["base"]},{"name":"right","dependencies":["base"]},{"name":"base","dependencies":[]}]), json!({"batches":[["base"],["left","right"],["top"]],"blocked":[]})),
            test("hard-3", "missing dependency", json!([{"name":"app","dependencies":["missing"]},{"name":"ok","dependencies":[]}]), json!({"batches":[["ok"]],"blocked":["app"]})),
            test("hard-4", "cycle and dependent", json!([{"name":"a","dependencies":["b"]},{"name":"b","dependencies":["a"]},{"name":"c","dependencies":["a"]}]), json!({"batches":[],"blocked":["a","b","c"]})),
            test("hard-5", "duplicate dependencies", json!([{"name":"a","dependencies":[]},{"name":"b","dependencies":["a","a"]}]), json!({"batches":[["a"],["b"]],"blocked":[]})),
            test("hard-6", "disconnected roots", json!([{"name":"z","dependencies":[]},{"name":"a","dependencies":[]},{"name":"m","dependencies":[]}]), json!({"batches":[["a","m","z"]],"blocked":[]})),
            test("hard-7", "transitive missing dependency", json!([{"name":"base","dependencies":["external"]},{"name":"middle","dependencies":["base"]},{"name":"top","dependencies":["middle"]},{"name":"free","dependencies":[]}]), json!({"batches":[["free"]],"blocked":["base","middle","top"]})),
            test("hard-8", "cycle with downstream and independent chain", json!([{"name":"a","dependencies":["b"]},{"name":"b","dependencies":["c"]},{"name":"c","dependencies":["a"]},{"name":"downstream","dependencies":["a"]},{"name":"root","dependencies":[]},{"name":"leaf","dependencies":["root"]}]), json!({"batches":[["root"],["leaf"]],"blocked":["a","b","c","downstream"]})),
            test("hard-9", "self dependency", json!([{"name":"self","dependencies":["self"]},{"name":"consumer","dependencies":["self"]},{"name":"ready","dependencies":[]}]), json!({"batches":[["ready"]],"blocked":["consumer","self"]})),
            test("hard-10", "several lexical levels", json!([{"name":"release","dependencies":["api","web"]},{"name":"web","dependencies":["core"]},{"name":"api","dependencies":["core","db"]},{"name":"db","dependencies":[]},{"name":"core","dependencies":[]},{"name":"docs","dependencies":[]}]), json!({"batches":[["core","db","docs"],["api","web"],["release"]],"blocked":[]})),
            test("hard-11", "mixed missing cycle and valid graph", json!([{"name":"missing-root","dependencies":["outside"]},{"name":"missing-child","dependencies":["missing-root"]},{"name":"cycle-a","dependencies":["cycle-b"]},{"name":"cycle-b","dependencies":["cycle-a"]},{"name":"cycle-child","dependencies":["cycle-b"]},{"name":"valid-a","dependencies":[]},{"name":"valid-b","dependencies":["valid-a"]}]), json!({"batches":[["valid-a"],["valid-b"]],"blocked":["cycle-a","cycle-b","cycle-child","missing-child","missing-root"]})),
            test("hard-12", "empty package list", json!([]), json!({"batches":[],"blocked":[]})),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_stable_weighted_cases() {
        let cases = catalog();
        assert_eq!(cases.len(), 18);
        assert!((cases.iter().map(|case| case.weight).sum::<f64>() - 1.0).abs() < 0.0001);
        assert!(cases.iter().all(|case| case.tests.len() >= 6));
        for difficulty in ["easy", "medium", "hard"] {
            assert_eq!(
                cases
                    .iter()
                    .filter(|case| case.difficulty == difficulty)
                    .count(),
                6
            );
        }
    }

    #[test]
    fn correct_easy_submission_passes() {
        let case = catalog()
            .into_iter()
            .find(|case| case.id == "professional-js-v2-easy-01-compact-ranges")
            .unwrap();
        let code = r#"```javascript
function compactRanges(values) {
  const sorted = [...new Set(values)].sort((a,b) => a-b);
  const out = [];
  for (let i = 0; i < sorted.length; i++) {
    let start = sorted[i], end = start;
    while (i + 1 < sorted.length && sorted[i + 1] === end + 1) { i++; end = sorted[i]; }
    out.push(start === end ? String(start) : `${start}-${end}`);
  }
  return out;
}
```"#;
        let grade = grade_submission_with_timeout(&case, code, 30);
        assert_eq!(grade.status, "passed");
        assert_eq!(grade.passed, grade.total);
    }

    #[test]
    fn capacity_allocation_contract_uses_only_documented_fields() {
        let case = catalog()
            .into_iter()
            .find(|case| case.id == "professional-js-v2-medium-06-capacity-allocation")
            .unwrap();
        let code = r#"```javascript
function allocateCapacity(input) {
  const remaining = {...input.capacities}, allocations = [], rejected = [];
  const requests = [...input.requests].sort((a, b) => b.priority - a.priority || a.id.localeCompare(b.id));
  for (const request of requests) {
    const pool = request.pools.filter(name => remaining[name] >= request.amount)
      .sort((a, b) => remaining[a] - remaining[b] || a.localeCompare(b))[0];
    if (pool === undefined) rejected.push(request.id);
    else {
      remaining[pool] -= request.amount;
      allocations.push({id: request.id, pool});
    }
  }
  return {allocations, rejected, remaining: Object.fromEntries(Object.entries(remaining).sort())};
}
```"#;
        let grade = grade_submission_with_timeout(&case, code, 30);
        assert_eq!(grade.status, "passed", "{:?}", grade.feedback);
        assert_eq!(grade.passed, grade.total);
    }

    #[test]
    fn safety_scan_matches_identifiers_without_rejecting_prefixes() {
        assert_eq!(forbidden_construct("const processed = new Set();"), None);
        assert_eq!(forbidden_construct("const denormalized = true;"), None);
        assert_eq!(forbidden_construct("process.exit(1);"), Some("process"));
        assert_eq!(
            forbidden_construct("new Function('return 1')"),
            Some("Function constructor")
        );
    }

    #[test]
    fn bundled_reference_solutions_pass_every_case() {
        for case in catalog() {
            let grade = grade_submission_with_timeout(&case, &case.reference_solution, 30);
            assert_eq!(
                grade.passed, grade.total,
                "{} reference failed: {:?}",
                case.id, grade.feedback
            );
        }
    }
}
