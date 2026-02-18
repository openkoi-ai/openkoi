---
name: sql-safety
kind: evaluator
description: Evaluates SQL queries and database migrations for correctness, safety, performance, and reversibility.
metadata:
  categories: ["database", "migration", "sql", "schema"]
  dimensions:
    - name: correctness
      weight: 0.35
      description: Does the SQL produce the intended result?
    - name: safety
      weight: 0.3
      description: Is the query safe from injection, data loss, and locking issues?
    - name: performance
      weight: 0.2
      description: Will the query perform well at scale?
    - name: reversibility
      weight: 0.15
      description: Can the change be rolled back without data loss?
---

# SQL Safety Evaluator

Evaluate SQL output against these criteria.

## Correctness (35%)

- Does the query produce the intended result set or mutation?
- Are JOINs correct (inner vs outer, join conditions)?
- Are WHERE clauses filtering the right rows?
- Are GROUP BY and aggregate functions used correctly?
- For migrations: does the schema change match the requirements?
- Are data types appropriate for the column contents?
- Are NULL semantics handled correctly (NULL != NULL, COALESCE where needed)?
- Are transactions used where atomicity is required?

## Safety (30%)

- Is the query parameterized (no string interpolation of user input)?
- For UPDATE/DELETE: is there a WHERE clause (no accidental full-table mutation)?
- For migrations: is there a risk of data loss (dropping columns, changing types)?
- Are there locking concerns (long-running transactions on hot tables)?
- Is there a risk of deadlocks from the access pattern?
- Are foreign key constraints maintained?
- For ALTER TABLE: will this lock the table? For how long? Is this acceptable?
- Are permissions appropriate (no unnecessary GRANT escalation)?

## Performance (20%)

- Are there indexes to support the WHERE and JOIN conditions?
- Will this query scan the full table when it does not need to?
- Are there N+1 query patterns?
- For large tables: is the operation batched (not one massive transaction)?
- Are there unnecessary subqueries that could be JOINs?
- Is SELECT * avoided in production queries?
- For migrations: is the migration online-safe (no long locks on large tables)?

## Reversibility (15%)

- Is there a corresponding DOWN migration?
- Can the change be rolled back without data loss?
- For destructive changes (DROP, column removal): is there a backup step?
- Are non-reversible changes flagged explicitly?
- For data migrations: is the old format preserved during transition?

## Severity Guide

- **Blocker**: SQL injection risk, accidental data deletion, wrong query results
- **Important**: Missing index on hot path, no DOWN migration, full-table lock on large table
- **Suggestion**: Style improvements, naming conventions, minor optimization opportunities
