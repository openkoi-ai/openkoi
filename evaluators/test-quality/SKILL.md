---
name: test-quality
kind: evaluator
description: Evaluates test code for coverage, assertion quality, isolation, and readability.
metadata:
  categories: ["test", "testing", "spec", "unit-test", "integration-test"]
  dimensions:
    - name: coverage
      weight: 0.3
      description: Are the important behaviors and edge cases tested?
    - name: assertions
      weight: 0.3
      description: Are assertions specific, meaningful, and testing the right thing?
    - name: isolation
      weight: 0.2
      description: Are tests independent, deterministic, and properly mocked?
    - name: readability
      weight: 0.2
      description: Are tests clear, well-named, and maintainable?
---

# Test Quality Evaluator

Evaluate test code against these criteria.

## Coverage (30%)

- Are the happy path scenarios tested?
- Are error paths and failure modes tested?
- Are edge cases covered (empty input, None/null, boundary values, unicode)?
- Are important state transitions tested?
- Is the coverage proportional to risk (critical paths have more tests)?
- Are both positive and negative cases present (testing what should AND should not happen)?
- For bug fixes: is there a regression test that would catch the original bug?

## Assertions (30%)

- Are assertions specific (checking exact values, not just "no error")?
- Do assertions test behavior, not implementation details?
- Are error messages in assertions descriptive (what was expected vs what happened)?
- Is each test asserting one logical concept (not testing 5 unrelated things)?
- Are snapshot/golden tests used appropriately (not as a lazy substitute for specific assertions)?
- Do assertions verify side effects where relevant (database state, file writes, API calls)?
- Are floating point comparisons using approximate equality?

## Isolation (20%)

- Does each test run independently (no shared mutable state between tests)?
- Are external dependencies mocked or stubbed (network, filesystem, time, randomness)?
- Can tests run in any order without affecting each other?
- Are test fixtures and setup/teardown properly scoped?
- Is time-dependent logic tested with controlled clocks (not real time)?
- Are flaky patterns avoided (sleep-based waits, race conditions)?
- For integration tests: is cleanup reliable (no leaked resources)?

## Readability (20%)

- Do test names describe the scenario and expected outcome?
- Is the Arrange/Act/Assert (or Given/When/Then) pattern clear?
- Are test helpers and builders used to reduce boilerplate?
- Is setup code extracted when repeated across multiple tests?
- Are magic numbers and strings explained or named?
- Can a new developer understand what is being tested by reading the test name and body?
- Are tests grouped logically (by feature, by method, by scenario)?

## Severity Guide

- **Blocker**: Tests that always pass (meaningless assertions), tests that depend on external state, tests that mask bugs
- **Important**: Missing edge case coverage, implementation-coupled tests that break on refactor, flaky tests
- **Suggestion**: Naming improvements, better test organization, additional helper extraction
