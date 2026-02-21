// src/cli/progress.rs — Terminal progress renderer for real-time task feedback

use crate::core::types::ProgressEvent;

/// Build a progress callback that writes formatted output to stderr.
///
/// All progress output goes to stderr so stdout remains clean for task output.
/// Returns a closure suitable for `Orchestrator::with_progress()`.
pub fn terminal_progress() -> impl Fn(ProgressEvent) + Send + 'static {
    move |event| match event {
        ProgressEvent::PlanReady {
            steps,
            estimated_iterations,
        } => {
            eprintln!(
                "[plan] {} step(s), estimated {} iteration(s)",
                steps, estimated_iterations,
            );
        }
        ProgressEvent::IterationStart {
            iteration,
            max_iterations,
        } => {
            eprintln!("[iter {}/{}] executing...", iteration, max_iterations);
        }
        ProgressEvent::ToolCall { name, iteration } => {
            eprintln!("[iter {}]   tool: {}", iteration, name);
        }
        ProgressEvent::IterationEnd {
            iteration,
            score,
            decision,
            cost_so_far,
        } => {
            eprintln!(
                "[iter {}] score={:.2} -> {:<12} (${:.2})",
                iteration, score, decision, cost_so_far,
            );
        }
        ProgressEvent::SafetyWarning { message } => {
            eprintln!("[safety] {}", message);
        }
        ProgressEvent::Complete {
            iterations,
            total_tokens,
            cost,
            final_score,
        } => {
            eprintln!(
                "[done] score={:.2} iterations={} tokens={} cost=${:.2}",
                final_score, iterations, total_tokens, cost,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::IterationDecision;
    use std::sync::{Arc, Mutex};

    /// Helper that captures progress output into a Vec instead of stderr.
    fn capturing_progress() -> (
        impl Fn(ProgressEvent) + Send + 'static,
        Arc<Mutex<Vec<String>>>,
    ) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_clone = log.clone();
        let cb = move |event: ProgressEvent| {
            let msg = match event {
                ProgressEvent::PlanReady {
                    steps,
                    estimated_iterations,
                } => format!(
                    "[plan] {} step(s), estimated {} iteration(s)",
                    steps, estimated_iterations
                ),
                ProgressEvent::IterationStart {
                    iteration,
                    max_iterations,
                } => format!("[iter {}/{}] executing...", iteration, max_iterations),
                ProgressEvent::ToolCall { name, iteration } => {
                    format!("[iter {}]   tool: {}", iteration, name)
                }
                ProgressEvent::IterationEnd {
                    iteration,
                    score,
                    decision,
                    cost_so_far,
                } => format!(
                    "[iter {}] score={:.2} -> {:<12} (${:.2})",
                    iteration, score, decision, cost_so_far,
                ),
                ProgressEvent::SafetyWarning { message } => format!("[safety] {}", message),
                ProgressEvent::Complete {
                    iterations,
                    total_tokens,
                    cost,
                    final_score,
                } => format!(
                    "[done] score={:.2} iterations={} tokens={} cost=${:.2}",
                    final_score, iterations, total_tokens, cost,
                ),
            };
            log_clone.lock().unwrap().push(msg);
        };
        (cb, log)
    }

    #[test]
    fn test_plan_ready_format() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::PlanReady {
            steps: 3,
            estimated_iterations: 2,
        });
        let msgs = log.lock().unwrap();
        assert_eq!(msgs[0], "[plan] 3 step(s), estimated 2 iteration(s)");
    }

    #[test]
    fn test_iteration_start_format() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::IterationStart {
            iteration: 1,
            max_iterations: 3,
        });
        let msgs = log.lock().unwrap();
        assert_eq!(msgs[0], "[iter 1/3] executing...");
    }

    #[test]
    fn test_tool_call_format() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::ToolCall {
            name: "edit_file(\"src/main.rs\")".into(),
            iteration: 2,
        });
        let msgs = log.lock().unwrap();
        assert_eq!(msgs[0], "[iter 2]   tool: edit_file(\"src/main.rs\")");
    }

    #[test]
    fn test_iteration_end_format() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::IterationEnd {
            iteration: 1,
            score: 0.78,
            decision: IterationDecision::Continue,
            cost_so_far: 0.04,
        });
        let msgs = log.lock().unwrap();
        assert!(msgs[0].contains("score=0.78"));
        assert!(msgs[0].contains("continue"));
        assert!(msgs[0].contains("$0.04"));
    }

    #[test]
    fn test_safety_warning_format() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::SafetyWarning {
            message: "Safety abort: abort_budget".into(),
        });
        let msgs = log.lock().unwrap();
        assert_eq!(msgs[0], "[safety] Safety abort: abort_budget");
    }

    #[test]
    fn test_complete_format() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::Complete {
            iterations: 2,
            total_tokens: 32104,
            cost: 0.11,
            final_score: 0.91,
        });
        let msgs = log.lock().unwrap();
        assert!(msgs[0].contains("score=0.91"));
        assert!(msgs[0].contains("iterations=2"));
        assert!(msgs[0].contains("tokens=32104"));
        assert!(msgs[0].contains("cost=$0.11"));
    }

    #[test]
    fn test_full_lifecycle_sequence() {
        let (cb, log) = capturing_progress();
        cb(ProgressEvent::PlanReady {
            steps: 2,
            estimated_iterations: 3,
        });
        cb(ProgressEvent::IterationStart {
            iteration: 1,
            max_iterations: 3,
        });
        cb(ProgressEvent::ToolCall {
            name: "read_file(\"src/lib.rs\")".into(),
            iteration: 1,
        });
        cb(ProgressEvent::IterationEnd {
            iteration: 1,
            score: 0.72,
            decision: IterationDecision::Continue,
            cost_so_far: 0.06,
        });
        cb(ProgressEvent::IterationStart {
            iteration: 2,
            max_iterations: 3,
        });
        cb(ProgressEvent::IterationEnd {
            iteration: 2,
            score: 0.91,
            decision: IterationDecision::Accept,
            cost_so_far: 0.14,
        });
        cb(ProgressEvent::Complete {
            iterations: 2,
            total_tokens: 38201,
            cost: 0.14,
            final_score: 0.91,
        });

        let msgs = log.lock().unwrap();
        assert_eq!(msgs.len(), 7);
        assert!(msgs[0].starts_with("[plan]"));
        assert!(msgs[1].starts_with("[iter 1/3]"));
        assert!(msgs[2].contains("tool:"));
        assert!(msgs[3].contains("continue"));
        assert!(msgs[4].starts_with("[iter 2/3]"));
        assert!(msgs[5].contains("accept"));
        assert!(msgs[6].starts_with("[done]"));
    }
}
