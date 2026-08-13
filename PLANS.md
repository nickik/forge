# Execution Plans

Use an execution plan for changes that cross major compiler phases, alter normative semantics, or are too large to safely complete as one local edit.

A plan is a living document. Keep it updated as implementation reveals facts.

## Required sections

### Goal
What user-visible or compiler-visible outcome will exist when complete?

### Normative basis
List exact spec sections affected. If semantics are intentionally changing, state the proposed spec change first.

### Current state
Describe the files, data structures, and compiler phases involved.

### Invariants
List behavior that must remain true. Include source-compatibility constraints.

### Milestones
Break work into independently testable increments.

### Tests
List positive tests, negative diagnostics, regression tests, and any target-specific tests.

### Risks / decisions
Record ambiguity, implementation tradeoffs, and rejected alternatives.

### Completion record
Summarize files changed, tests run, remaining follow-ups, and any spec documentation updated.

## Rule

Do not let an implementation plan become a hidden specification. Normative language semantics belong in `docs/forge-v1-spec.md` and `docs/fdn-v1-spec.md`.
