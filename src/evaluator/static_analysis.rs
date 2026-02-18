// src/evaluator/static_analysis.rs — Built-in static analysis
//
// Detects and runs linters/type checkers (cargo clippy, eslint, ruff, mypy, etc.).
// Parses output to derive lint issues with severity and location.

use crate::core::types::*;
use std::path::Path;
use tokio::process::Command;

/// Built-in static analyzer that runs lint and type checks.
pub struct StaticAnalyzer;

/// Result from running static analysis.
pub struct LintResult {
    pub all_clean: bool,
    pub issues: Vec<LintIssue>,
}

pub struct LintIssue {
    pub message: String,
    pub severity: LintSeverity,
    pub location: Option<String>,
}

pub enum LintSeverity {
    Error,
    Warning,
}

impl Default for StaticAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Run static analysis if applicable to the project.
    ///
    /// Returns `None` if no analysis tool is detected.
    ///
    /// `project_dir` is the directory to check for project markers and to run
    /// commands in. Pass `"."` for the current working directory.
    pub async fn run_if_applicable(
        &self,
        _task: &TaskInput,
        project_dir: &Path,
    ) -> anyhow::Result<Option<LintResult>> {
        // Try each analyzer in order of preference
        if project_dir.join("Cargo.toml").exists() {
            return self.run_cargo_clippy(project_dir).await;
        }
        if project_dir.join("pyproject.toml").exists() {
            return self.run_ruff_or_flake8(project_dir).await;
        }
        if project_dir.join(".eslintrc.json").exists()
            || project_dir.join(".eslintrc.js").exists()
            || project_dir.join(".eslintrc.yml").exists()
            || project_dir.join("eslint.config.js").exists()
            || project_dir.join("eslint.config.mjs").exists()
        {
            return self.run_eslint(project_dir).await;
        }

        Ok(None)
    }

    /// Run `cargo clippy` and parse output.
    async fn run_cargo_clippy(&self, project_dir: &Path) -> anyhow::Result<Option<LintResult>> {
        // Check if clippy is available
        let check = Command::new("cargo")
            .args(["clippy", "--version"])
            .output()
            .await;

        if check.is_err() || !check.unwrap().status.success() {
            tracing::debug!("cargo clippy not available, skipping");
            return Ok(None);
        }

        tracing::debug!("Running: cargo clippy in {:?}", project_dir);

        let output = Command::new("cargo")
            .args([
                "clippy",
                "--all-targets",
                "--message-format=short",
                "--",
                "-D",
                "warnings",
            ])
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(project_dir)
            .output()
            .await?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        parse_clippy_output(&stderr, output.status.success())
    }

    /// Run `ruff check` or `flake8` and parse output.
    async fn run_ruff_or_flake8(&self, project_dir: &Path) -> anyhow::Result<Option<LintResult>> {
        // Try ruff first (faster)
        let ruff_check = Command::new("ruff").args(["--version"]).output().await;
        if ruff_check.is_ok() && ruff_check.unwrap().status.success() {
            return self.run_ruff(project_dir).await;
        }

        // Fall back to flake8
        let flake8_check = Command::new("flake8").args(["--version"]).output().await;
        if flake8_check.is_ok() && flake8_check.unwrap().status.success() {
            return self.run_flake8(project_dir).await;
        }

        tracing::debug!("No Python linter available (ruff/flake8), skipping");
        Ok(None)
    }

    async fn run_ruff(&self, project_dir: &Path) -> anyhow::Result<Option<LintResult>> {
        tracing::debug!("Running: ruff check in {:?}", project_dir);

        let output = Command::new("ruff")
            .args(["check", "."])
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_ruff_output(&stdout, output.status.success())
    }

    async fn run_flake8(&self, project_dir: &Path) -> anyhow::Result<Option<LintResult>> {
        tracing::debug!("Running: flake8 in {:?}", project_dir);

        let output = Command::new("flake8")
            .args([".", "--format=default"])
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        // flake8 output format is similar to ruff
        parse_ruff_output(&stdout, output.status.success())
    }

    /// Run `eslint` and parse output.
    async fn run_eslint(&self, project_dir: &Path) -> anyhow::Result<Option<LintResult>> {
        // Check npx availability
        let check = Command::new("npx").args(["--version"]).output().await;
        if check.is_err() || !check.unwrap().status.success() {
            tracing::debug!("npx not available, skipping eslint");
            return Ok(None);
        }

        tracing::debug!("Running: npx eslint in {:?}", project_dir);

        let output = Command::new("npx")
            .args(["eslint", ".", "--format=compact"])
            .current_dir(project_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_eslint_output(&stdout, output.status.success())
    }
}

/// Parse `cargo clippy` output (short format).
///
/// Lines like: `src/main.rs:12:5: warning: unused variable`
fn parse_clippy_output(output: &str, success: bool) -> anyhow::Result<Option<LintResult>> {
    let mut issues = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Skip compilation progress and non-diagnostic lines
        if trimmed.is_empty()
            || trimmed.starts_with("Compiling")
            || trimmed.starts_with("Checking")
            || trimmed.starts_with("Finished")
            || trimmed.starts_with("Downloading")
            || trimmed.starts_with("warning: build failed")
        {
            continue;
        }

        // Match lines like: `warning: unused variable` or `error[E0308]: mismatched types`
        // or `src/foo.rs:12:5: warning: ...`
        if trimmed.starts_with("warning:") || trimmed.starts_with("error") {
            let severity = if trimmed.starts_with("error") {
                LintSeverity::Error
            } else {
                LintSeverity::Warning
            };

            issues.push(LintIssue {
                message: trimmed.to_string(),
                severity,
                location: None,
            });
        } else if trimmed.contains(": warning:") || trimmed.contains(": error") {
            // Format: "src/foo.rs:12:5: warning: message"
            let severity = if trimmed.contains(": error") {
                LintSeverity::Error
            } else {
                LintSeverity::Warning
            };

            // Extract location (everything before the first ": warning:" or ": error")
            let location = if let Some(idx) = trimmed.find(": warning:") {
                Some(trimmed[..idx].to_string())
            } else {
                trimmed.find(": error").map(|idx| trimmed[..idx].to_string())
            };

            issues.push(LintIssue {
                message: trimmed.to_string(),
                severity,
                location,
            });
        }
    }

    if issues.is_empty() && success {
        // Clippy ran successfully with no issues
        return Ok(Some(LintResult {
            all_clean: true,
            issues: vec![],
        }));
    }

    if issues.is_empty() {
        // Failed but couldn't parse issues — might be a compilation error
        return Ok(None);
    }

    Ok(Some(LintResult {
        all_clean: false,
        issues,
    }))
}

/// Parse ruff/flake8 output.
///
/// Lines like: `src/foo.py:12:5: E302 expected 2 blank lines, got 1`
fn parse_ruff_output(output: &str, success: bool) -> anyhow::Result<Option<LintResult>> {
    let mut issues = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Found") {
            continue;
        }

        // Format: "file.py:line:col: CODE message"
        // The code determines severity: E = error, W = warning, F = error
        let parts: Vec<&str> = trimmed.splitn(4, ':').collect();
        if parts.len() >= 4 {
            let location = format!(
                "{}:{}:{}",
                parts[0].trim(),
                parts[1].trim(),
                parts[2].trim()
            );
            let msg_raw = parts[3];
            let msg = msg_raw.trim();

            // Check the lint code prefix to determine severity
            let severity = if msg.starts_with("E") || msg.starts_with("F") {
                LintSeverity::Error
            } else {
                LintSeverity::Warning
            };

            issues.push(LintIssue {
                message: msg.trim().to_string(),
                severity,
                location: Some(location),
            });
        }
    }

    if issues.is_empty() && success {
        return Ok(Some(LintResult {
            all_clean: true,
            issues: vec![],
        }));
    }

    if issues.is_empty() {
        return Ok(None);
    }

    Ok(Some(LintResult {
        all_clean: false,
        issues,
    }))
}

/// Parse `eslint` compact format output.
///
/// Lines like: `/path/file.js: line 12, col 5, Warning - message (rule)`
fn parse_eslint_output(output: &str, success: bool) -> anyhow::Result<Option<LintResult>> {
    let mut issues = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Compact format: "/path/file.js: line 12, col 5, Warning - message (rule)"
        if trimmed.contains(", Warning -") || trimmed.contains(", Error -") {
            let severity = if trimmed.contains(", Error -") {
                LintSeverity::Error
            } else {
                LintSeverity::Warning
            };

            // Extract location: everything before ": line"
            let location = trimmed.find(": line ").map(|idx| {
                // Build "file:line:col"
                let file = &trimmed[..idx];
                let after = &trimmed[idx + 7..]; // skip ": line "
                if let Some(col_idx) = after.find(", col ") {
                    let line_num = &after[..col_idx];
                    let rest = &after[col_idx + 6..]; // skip ", col "
                    let col = rest.split(',').next().unwrap_or("0");
                    format!("{}:{}:{}", file, line_num, col)
                } else {
                    file.to_string()
                }
            });

            // Extract message
            let message = if let Some(dash_idx) = trimmed.find(" - ") {
                trimmed[dash_idx + 3..].to_string()
            } else {
                trimmed.to_string()
            };

            issues.push(LintIssue {
                message,
                severity,
                location,
            });
        }
    }

    if issues.is_empty() && success {
        return Ok(Some(LintResult {
            all_clean: true,
            issues: vec![],
        }));
    }

    if issues.is_empty() {
        return Ok(None);
    }

    Ok(Some(LintResult {
        all_clean: false,
        issues,
    }))
}

impl LintResult {
    pub fn to_dimension_score(&self) -> DimensionScore {
        if self.all_clean {
            return DimensionScore {
                dimension: "static_analysis".into(),
                score: 1.0,
                weight: 0.2,
            };
        }

        // Score decreases based on number and severity of issues
        let error_count = self
            .issues
            .iter()
            .filter(|i| matches!(i.severity, LintSeverity::Error))
            .count();
        let warning_count = self
            .issues
            .iter()
            .filter(|i| matches!(i.severity, LintSeverity::Warning))
            .count();

        // Errors are worth 0.1 deduction each, warnings 0.03
        let deduction = (error_count as f32 * 0.1 + warning_count as f32 * 0.03).min(0.8);
        let score = (1.0 - deduction).max(0.2);

        DimensionScore {
            dimension: "static_analysis".into(),
            score,
            weight: 0.2,
        }
    }

    pub fn issues_as_findings(&self) -> Vec<Finding> {
        self.issues
            .iter()
            .enumerate()
            .map(|(i, issue)| Finding {
                id: format!("L{}", i + 1),
                severity: match issue.severity {
                    LintSeverity::Error => Severity::Important,
                    LintSeverity::Warning => Severity::Suggestion,
                },
                dimension: "static_analysis".into(),
                title: issue.message.clone(),
                description: issue.message.clone(),
                location: issue.location.clone(),
                fix: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clippy_clean() {
        let output = "\
    Checking openkoi v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.0s
";
        let result = parse_clippy_output(output, true).unwrap().unwrap();
        assert!(result.all_clean);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_parse_clippy_warnings() {
        let output = "\
    Checking openkoi v0.1.0
warning: unused variable: `x`
  --> src/main.rs:5:9
src/lib.rs:10:5: warning: this could be simplified
warning: 2 warnings emitted
";
        let result = parse_clippy_output(output, false).unwrap().unwrap();
        assert!(!result.all_clean);
        assert!(result.issues.len() >= 2);
    }

    #[test]
    fn test_parse_clippy_errors() {
        let output = "error[E0308]: mismatched types\n";
        let result = parse_clippy_output(output, false).unwrap().unwrap();
        assert!(!result.all_clean);
        assert_eq!(result.issues.len(), 1);
        assert!(matches!(result.issues[0].severity, LintSeverity::Error));
    }

    #[test]
    fn test_parse_ruff_clean() {
        let result = parse_ruff_output("", true).unwrap().unwrap();
        assert!(result.all_clean);
    }

    #[test]
    fn test_parse_ruff_issues() {
        let output = "\
src/foo.py:12:5: E302 expected 2 blank lines, got 1
src/bar.py:5:1: W291 trailing whitespace
Found 2 errors.
";
        let result = parse_ruff_output(output, false).unwrap().unwrap();
        assert!(!result.all_clean);
        assert_eq!(result.issues.len(), 2);
        assert!(matches!(result.issues[0].severity, LintSeverity::Error));
        assert!(matches!(result.issues[1].severity, LintSeverity::Warning));
        assert_eq!(
            result.issues[0].location.as_deref(),
            Some("src/foo.py:12:5")
        );
    }

    #[test]
    fn test_parse_eslint_clean() {
        let result = parse_eslint_output("", true).unwrap().unwrap();
        assert!(result.all_clean);
    }

    #[test]
    fn test_parse_eslint_issues() {
        let output =
            "/src/App.js: line 5, col 3, Warning - Unexpected console statement (no-console)\n\
                       /src/App.js: line 10, col 1, Error - Missing semicolon (semi)\n";
        let result = parse_eslint_output(output, false).unwrap().unwrap();
        assert!(!result.all_clean);
        assert_eq!(result.issues.len(), 2);
        assert!(matches!(result.issues[0].severity, LintSeverity::Warning));
        assert!(matches!(result.issues[1].severity, LintSeverity::Error));
    }

    #[test]
    fn test_dimension_score_clean() {
        let result = LintResult {
            all_clean: true,
            issues: vec![],
        };
        let dim = result.to_dimension_score();
        assert_eq!(dim.score, 1.0);
        assert_eq!(dim.weight, 0.2);
    }

    #[test]
    fn test_dimension_score_with_errors() {
        let result = LintResult {
            all_clean: false,
            issues: vec![
                LintIssue {
                    message: "error".into(),
                    severity: LintSeverity::Error,
                    location: None,
                },
                LintIssue {
                    message: "warning".into(),
                    severity: LintSeverity::Warning,
                    location: None,
                },
            ],
        };
        let dim = result.to_dimension_score();
        // 1.0 - (1*0.1 + 1*0.03) = 0.87
        assert!((dim.score - 0.87).abs() < 0.01);
    }

    #[test]
    fn test_issues_as_findings() {
        let result = LintResult {
            all_clean: false,
            issues: vec![LintIssue {
                message: "unused variable".into(),
                severity: LintSeverity::Warning,
                location: Some("src/lib.rs:5:3".into()),
            }],
        };
        let findings = result.issues_as_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Suggestion);
        assert_eq!(findings[0].location.as_deref(), Some("src/lib.rs:5:3"));
    }
}
