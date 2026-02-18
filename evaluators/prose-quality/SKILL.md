---
name: prose-quality
kind: evaluator
description: Evaluates written content for clarity, accuracy, tone, and structure.
metadata:
  categories: ["writing", "summary", "docs", "documentation", "readme"]
  dimensions:
    - name: clarity
      weight: 0.3
      description: Is the writing clear, concise, and easy to understand?
    - name: accuracy
      weight: 0.3
      description: Are facts, claims, and technical details correct?
    - name: tone
      weight: 0.15
      description: Is the tone appropriate for the audience and purpose?
    - name: structure
      weight: 0.25
      description: Is the content well-organized with logical flow?
---

# Prose Quality Evaluator

Evaluate the written output against these criteria.

## Clarity (30%)

- Is each sentence easy to understand on first read?
- Are technical terms explained or used consistently?
- Is jargon avoided where plain language would work?
- Are sentences concise without sacrificing meaning?
- Is the writing free of ambiguity (could a reader misinterpret any part)?
- Are pronouns used clearly (no dangling references)?

## Accuracy (30%)

- Are all factual claims correct and verifiable?
- Are technical details accurate (commands, file paths, API names)?
- Are code examples syntactically correct and runnable?
- Are version numbers, dates, and links correct?
- Is anything stated that contradicts the source material or task context?
- Are caveats and limitations mentioned where appropriate?

## Tone (15%)

- Is the tone appropriate for the target audience?
- Is the writing professional without being stuffy?
- Is it consistent throughout (no jarring shifts from formal to casual)?
- Does it avoid condescension ("simply", "just", "obviously")?
- For docs: is it task-oriented and helpful rather than abstract?
- For summaries: is it objective and balanced?

## Structure (25%)

- Is there a logical progression from introduction to details?
- Are headings and sections used effectively?
- Does each paragraph focus on one idea?
- Are lists used where appropriate (not buried in prose)?
- Is the length appropriate (not padded, not truncated)?
- Is there a clear beginning, middle, and end?

## Severity Guide

- **Blocker**: Factually wrong, misleading, or incomprehensible
- **Important**: Confusing structure, missing key information, wrong audience tone
- **Suggestion**: Minor wording improvements, formatting tweaks, optional additions
