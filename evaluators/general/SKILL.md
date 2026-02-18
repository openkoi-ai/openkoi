---
name: general
kind: evaluator
description: General-purpose evaluator for any task type. Used as fallback when no domain-specific evaluator matches.
metadata:
  categories: []
  dimensions:
    - name: relevance
      weight: 0.4
      description: Does the output address what the task actually asked for?
    - name: quality
      weight: 0.35
      description: Is the output well-crafted, accurate, and free of errors?
    - name: completeness
      weight: 0.25
      description: Are all parts of the task addressed? Any gaps?
---

# General Evaluator

Evaluate the output against the task requirements using these criteria.

## Relevance (40%)

- Does the output directly address the task description?
- Is the response on-topic and focused?
- Are there tangential or irrelevant sections that dilute the output?
- Would a reasonable person reading this say "yes, that answers what was asked"?

## Quality (35%)

- Is the output accurate and factually correct?
- Is it well-structured and easy to follow?
- Is the language clear and unambiguous?
- Are there logical errors, contradictions, or unsupported claims?
- Is the level of detail appropriate (not too shallow, not unnecessarily verbose)?

## Completeness (25%)

- Are all parts of the task addressed?
- Are there obvious gaps or missing elements?
- If the task had multiple requirements, are they all covered?
- Are edge cases or caveats mentioned where appropriate?

## Severity Guide

- **Blocker**: Output is wrong, misleading, or answers the wrong question entirely
- **Important**: Significant gap in coverage, notable inaccuracy, or confusing structure
- **Suggestion**: Minor improvements to clarity, depth, or organization
