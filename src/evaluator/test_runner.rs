// src/evaluator/test_runner.rs — Built-in test runner
//
// Detects and runs project test suites (cargo test, npm test, pytest, go test).
// Parses output to derive pass/fail counts and failure details.

use crate::core::types::*;
use std::path::Path;
use tokio::process::Command;

/// Built-in test runner that detects and runs project test suites.
pub struct TestRunner;

/// Result from running tests.
pub struct TestResult {
    pub all_passed: bool,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub failures: Vec<TestFailure>,
}

pub struct TestFailure {
    pub name: String,
    pub message: String,
    pub location: Option<String>,
}

impl Default for TestRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl TestRunner {
    pub fn new() -> Self {
        Self
    }

    /// Run tests if a test suite is detected in the project.
    ///
    /// Returns `None` if no test runner is detected.
    /// Runs the test command and parses output to produce a `TestResult`.
    ///
    /// `project_dir` is the directory to check for project markers and to run
    /// commands in. Pass `"."` for the current working directory.
    pub async fn run_if_available(
        &self,
        _task: &TaskInput,
        project_dir: &Path,
    ) -> anyhow::Result<Option<TestResult>> {
        // Try each runner in order of preference
        if project_dir.join("Cargo.toml").exists() {
            return self.run_cargo_test(project_dir).await;
        }
        if project_dir.join("go.mod").exists() {
            return self.run_go_test(project_dir).await;
        }
        if project_dir.join("pyproject.toml").exists() || project_dir.join("pytest.ini").exists() {
            return self.run_pytest(project_dir).await;
        }
        if project_dir.join("package.json").exists() {
            return self.run_npm_test(project_dir).await;
        }

        Ok(None)
    }

    /// Run `cargo test` and parse output.
    async fn run_cargo_test(&self, project_dir: &Path) -> anyhow::Result<Option<TestResult>> {
        tracing::debug!("Running: cargo test in {:?}", project_dir);

        let output = Command::new("cargo")
            .args(["test", "--", "--format=terse"])
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        parse_cargo_test_output(&combined, output.status.success())
    }

    /// Run `go test ./...` and parse output.
    async fn run_go_test(&self, project_dir: &Path) -> anyhow::Result<Option<TestResult>> {
        tracing::debug!("Running: go test ./... in {:?}", project_dir);

        let output = Command::new("go")
            .args(["test", "-v", "./..."])
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_go_test_output(&stdout, output.status.success())
    }

    /// Run `pytest` and parse output.
    async fn run_pytest(&self, project_dir: &Path) -> anyhow::Result<Option<TestResult>> {
        tracing::debug!("Running: pytest in {:?}", project_dir);

        let output = Command::new("pytest")
            .args(["--tb=short", "-q"])
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_pytest_output(&stdout, output.status.success())
    }

    /// Run `npm test` and parse output.
    async fn run_npm_test(&self, project_dir: &Path) -> anyhow::Result<Option<TestResult>> {
        // Check if there's actually a test script defined
        let pkg_path = project_dir.join("package.json");
        let pkg = std::fs::read_to_string(&pkg_path).unwrap_or_default();
        if !pkg.contains("\"test\"") {
            tracing::debug!("package.json found but no 'test' script defined");
            return Ok(None);
        }

        tracing::debug!("Running: npm test in {:?}", project_dir);

        let output = Command::new("npm")
            .args(["test", "--", "--passWithNoTests"])
            .env("CI", "true") // Prevent interactive mode
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        parse_npm_test_output(&combined, output.status.success())
    }
}

/// Parse `cargo test` output.
///
/// Typical output line: `test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`
fn parse_cargo_test_output(output: &str, success: bool) -> anyhow::Result<Option<TestResult>> {
    let mut total_passed: u32 = 0;
    let mut total_failed: u32 = 0;
    let mut failures = Vec::new();

    // Parse summary lines like: "test result: ok. 46 passed; 0 failed; ..."
    for line in output.lines() {
        if line.starts_with("test result:") {
            // Extract "N passed" and "N failed"
            if let Some(passed) = extract_number_before(line, " passed") {
                total_passed += passed;
            }
            if let Some(failed) = extract_number_before(line, " failed") {
                total_failed += failed;
            }
        }

        // Capture individual failures: "test some::path ... FAILED"
        if line.starts_with("test ") && line.contains(" ... FAILED") {
            let name = line
                .trim_start_matches("test ")
                .split(" ... ")
                .next()
                .unwrap_or("unknown")
                .to_string();
            failures.push(TestFailure {
                name,
                message: line.to_string(),
                location: None,
            });
        }
    }

    let total = total_passed + total_failed;
    if total == 0 && !output.contains("test result:") {
        // No test output found at all
        return Ok(None);
    }

    Ok(Some(TestResult {
        all_passed: success && total_failed == 0,
        total,
        passed: total_passed,
        failed: total_failed,
        failures,
    }))
}

/// Parse `go test` output.
///
/// Lines like: `--- FAIL: TestFoo (0.00s)` or `ok      package    0.005s`
fn parse_go_test_output(output: &str, success: bool) -> anyhow::Result<Option<TestResult>> {
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut failures = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--- PASS:") {
            passed += 1;
        } else if trimmed.starts_with("--- FAIL:") {
            failed += 1;
            let name = trimmed
                .trim_start_matches("--- FAIL: ")
                .split_whitespace()
                .next()
                .unwrap_or("unknown")
                .to_string();
            failures.push(TestFailure {
                name,
                message: trimmed.to_string(),
                location: None,
            });
        }
    }

    let total = passed + failed;
    if total == 0 {
        return Ok(None);
    }

    Ok(Some(TestResult {
        all_passed: success && failed == 0,
        total,
        passed,
        failed,
        failures,
    }))
}

/// Parse `pytest` output.
///
/// Summary line: `5 passed, 2 failed in 0.12s` or `5 passed in 0.12s`
fn parse_pytest_output(output: &str, success: bool) -> anyhow::Result<Option<TestResult>> {
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut failures = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Failure line: `FAILED test_file.py::test_name - AssertionError: ...`
        if trimmed.starts_with("FAILED ") {
            let rest = trimmed.trim_start_matches("FAILED ");
            let parts: Vec<&str> = rest.splitn(2, " - ").collect();
            let name = parts.first().unwrap_or(&"unknown").to_string();
            let message = parts.get(1).unwrap_or(&"").to_string();
            failures.push(TestFailure {
                name,
                message,
                location: None,
            });
        }

        // Summary line: "5 passed" or "2 failed" or "5 passed, 2 failed in 0.12s"
        if trimmed.contains(" passed") || trimmed.contains(" failed") {
            if let Some(p) = extract_number_before(trimmed, " passed") {
                passed = p;
            }
            if let Some(f) = extract_number_before(trimmed, " failed") {
                failed = f;
            }
        }
    }

    let total = passed + failed;
    if total == 0 {
        return Ok(None);
    }

    Ok(Some(TestResult {
        all_passed: success && failed == 0,
        total,
        passed,
        failed,
        failures,
    }))
}

/// Parse `npm test` output (Jest/Vitest/Mocha style).
///
/// Jest summary: `Tests: 2 failed, 8 passed, 10 total`
/// Vitest summary: `Tests  2 failed | 8 passed (10)`
fn parse_npm_test_output(output: &str, success: bool) -> anyhow::Result<Option<TestResult>> {
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut failures = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Jest: "Tests:       2 failed, 8 passed, 10 total"
        if trimmed.starts_with("Tests:") || trimmed.starts_with("Tests ") {
            if let Some(p) = extract_number_before(trimmed, " passed") {
                passed = p;
            }
            if let Some(f) = extract_number_before(trimmed, " failed") {
                failed = f;
            }
        }

        // Jest failure: "  ● test name"
        if trimmed.starts_with("● ") || trimmed.starts_with("✕ ") || trimmed.starts_with("× ")
        {
            let name = trimmed[2..].trim().to_string();
            failures.push(TestFailure {
                name,
                message: trimmed.to_string(),
                location: None,
            });
        }

        // Vitest failure: " FAIL  src/foo.test.ts > test name"
        if trimmed.starts_with("FAIL") && trimmed.contains(" > ") {
            let name = trimmed.split(" > ").last().unwrap_or("unknown").to_string();
            failures.push(TestFailure {
                name,
                message: trimmed.to_string(),
                location: None,
            });
        }
    }

    let total = passed + failed;
    if total == 0 {
        // Fall back to exit code
        if !success {
            return Ok(Some(TestResult {
                all_passed: false,
                total: 1,
                passed: 0,
                failed: 1,
                failures: vec![TestFailure {
                    name: "npm test".into(),
                    message: "Tests failed (could not parse output)".into(),
                    location: None,
                }],
            }));
        }
        return Ok(None);
    }

    Ok(Some(TestResult {
        all_passed: success && failed == 0,
        total,
        passed,
        failed,
        failures,
    }))
}

/// Extract a number that appears immediately before `suffix` in the text.
/// E.g., `extract_number_before("46 passed; 0 failed", " passed")` → `Some(46)`
fn extract_number_before(text: &str, suffix: &str) -> Option<u32> {
    let idx = text.find(suffix)?;
    let before = &text[..idx];
    // Walk backwards to find the start of the number
    let num_str: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    num_str.parse().ok()
}

impl TestResult {
    pub fn to_dimension_score(&self) -> DimensionScore {
        let score = if self.total == 0 {
            1.0
        } else {
            self.passed as f32 / self.total as f32
        };

        DimensionScore {
            dimension: "tests".into(),
            score,
            weight: 0.3,
        }
    }

    pub fn failures_as_findings(&self) -> Vec<Finding> {
        self.failures
            .iter()
            .enumerate()
            .map(|(i, f)| Finding {
                id: format!("T{}", i + 1),
                severity: Severity::Blocker,
                dimension: "tests".into(),
                title: format!("Test failed: {}", f.name),
                description: f.message.clone(),
                location: f.location.clone(),
                fix: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_number_before() {
        assert_eq!(
            extract_number_before("46 passed; 0 failed", " passed"),
            Some(46)
        );
        assert_eq!(
            extract_number_before("46 passed; 0 failed", " failed"),
            Some(0)
        );
        assert_eq!(extract_number_before("no match here", " passed"), None);
        assert_eq!(extract_number_before("", " passed"), None);
    }

    #[test]
    fn test_parse_cargo_test_all_pass() {
        let output = "\
running 3 tests
test foo ... ok
test bar ... ok
test baz ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";
        let result = parse_cargo_test_output(output, true).unwrap().unwrap();
        assert!(result.all_passed);
        assert_eq!(result.total, 3);
        assert_eq!(result.passed, 3);
        assert_eq!(result.failed, 0);
        assert!(result.failures.is_empty());
    }

    #[test]
    fn test_parse_cargo_test_with_failures() {
        let output = "\
running 3 tests
test foo ... ok
test bar ... FAILED
test baz ... ok

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";
        let result = parse_cargo_test_output(output, false).unwrap().unwrap();
        assert!(!result.all_passed);
        assert_eq!(result.total, 3);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].name, "bar");
    }

    #[test]
    fn test_parse_cargo_test_multiple_suites() {
        let output = "\
running 2 tests
test a ... ok
test b ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 3 tests
test c ... ok
test d ... FAILED
test e ... ok

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";
        let result = parse_cargo_test_output(output, false).unwrap().unwrap();
        assert!(!result.all_passed);
        // Suite 1: 2 passed + 0 failed, Suite 2: 2 passed + 1 failed = 4+1=5
        assert_eq!(result.passed, 4);
        assert_eq!(result.failed, 1);
        assert_eq!(result.total, 5);
    }

    #[test]
    fn test_parse_cargo_test_no_tests() {
        let output = "no test output at all\n";
        let result = parse_cargo_test_output(output, true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_go_test_pass() {
        let output = "\
=== RUN   TestFoo
--- PASS: TestFoo (0.00s)
=== RUN   TestBar
--- PASS: TestBar (0.01s)
ok  \tpackage\t0.011s
";
        let result = parse_go_test_output(output, true).unwrap().unwrap();
        assert!(result.all_passed);
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_parse_go_test_failure() {
        let output = "\
=== RUN   TestFoo
--- PASS: TestFoo (0.00s)
=== RUN   TestBar
--- FAIL: TestBar (0.01s)
FAIL\tpackage\t0.011s
";
        let result = parse_go_test_output(output, false).unwrap().unwrap();
        assert!(!result.all_passed);
        assert_eq!(result.total, 2);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures[0].name, "TestBar");
    }

    #[test]
    fn test_parse_pytest_pass() {
        let output = "5 passed in 0.12s\n";
        let result = parse_pytest_output(output, true).unwrap().unwrap();
        assert!(result.all_passed);
        assert_eq!(result.passed, 5);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_parse_pytest_failure() {
        let output = "\
FAILED test_math.py::test_add - AssertionError: 1 != 2
2 passed, 1 failed in 0.45s
";
        let result = parse_pytest_output(output, false).unwrap().unwrap();
        assert!(!result.all_passed);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].name, "test_math.py::test_add");
    }

    #[test]
    fn test_parse_jest_output() {
        let output = "\
Tests:       1 failed, 4 passed, 5 total
";
        let result = parse_npm_test_output(output, false).unwrap().unwrap();
        assert!(!result.all_passed);
        assert_eq!(result.passed, 4);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn test_dimension_score_all_pass() {
        let result = TestResult {
            all_passed: true,
            total: 10,
            passed: 10,
            failed: 0,
            failures: vec![],
        };
        let dim = result.to_dimension_score();
        assert_eq!(dim.score, 1.0);
        assert_eq!(dim.weight, 0.3);
    }

    #[test]
    fn test_dimension_score_partial() {
        let result = TestResult {
            all_passed: false,
            total: 10,
            passed: 8,
            failed: 2,
            failures: vec![],
        };
        let dim = result.to_dimension_score();
        assert!((dim.score - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_failures_as_findings() {
        let result = TestResult {
            all_passed: false,
            total: 2,
            passed: 1,
            failed: 1,
            failures: vec![TestFailure {
                name: "test_foo".into(),
                message: "assertion failed".into(),
                location: Some("src/lib.rs:42".into()),
            }],
        };
        let findings = result.failures_as_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Blocker);
        assert!(findings[0].title.contains("test_foo"));
    }
}
