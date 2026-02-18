---
name: code-review
kind: evaluator
description: Evaluates code changes for correctness, style, and safety.
metadata:
  categories: ["code", "refactor", "bugfix"]
  dimensions:
    - name: correctness
      weight: 0.4
      description: Does the code do what the task asked?
    - name: safety
      weight: 0.25
      description: Error handling, input validation, no panics
    - name: style
      weight: 0.15
      description: Idiomatic, readable, consistent naming
    - name: completeness
      weight: 0.2
      description: Edge cases, tests, documentation
---

# Code Review Evaluator

Evaluate the code output against these criteria.

## Correctness (40%)

- Does the implementation match the task requirements?
- Are all specified behaviors implemented?
- Would this code produce correct results for normal inputs?
- Are there logic errors, off-by-one mistakes, or incorrect assumptions?
- Does the code handle the data types and structures correctly?
- If modifying existing code, does it preserve existing behavior where expected?

## Safety (25%)

- Are errors handled properly (no unwrap on user input, no silent failures)?
- Is user input validated and sanitized?
- Are there potential panics, integer overflows, or buffer issues?
- Are there resource leaks (unclosed files, connections, channels)?
- Are credentials, secrets, or sensitive data handled securely?
- Is there proper bounds checking on arrays, slices, and indices?
- Are concurrent access patterns safe (no data races, proper locking)?

## Style (15%)

- Is the code idiomatic for the language?
- Are variable and function names descriptive and consistent?
- Is the code DRY without being over-abstracted?
- Are functions at a reasonable size (not too long, not trivially small)?
- Is the code formatted consistently?
- Are comments present where logic is non-obvious?

## Completeness (20%)

- Are edge cases handled (empty inputs, None/null, max values, unicode)?
- Are tests included or updated (if applicable)?
- Is the change documented where needed (comments, docstrings, README)?
- Are error messages helpful and actionable?
- If adding a public API, is it documented?

## Severity Guide

- **Blocker**: Crashes, data loss, security vulnerability, incorrect behavior for normal inputs
- **Important**: Missing error handling, poor performance on common paths, missing tests for core logic
- **Suggestion**: Style nits, naming improvements, minor refactoring opportunities
