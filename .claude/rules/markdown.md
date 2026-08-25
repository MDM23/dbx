---
paths:
  - "**/*.md"
---

# Markdown writing rules

Applies to every markdown file in the repository (ADRs, docs, rules).

## Style

- Wrap **paragraphs** at 80 columns. Lists, headlines or tables are allowed to be longer.
- One `#` H1 per file; heading levels never skip (`##` → `###`, not `##` → `####`).
- Fenced code blocks always declare a language (` ```bash `, ` ```nix `, ` ```rs `, …).

## Content

- Reference other documents by relative link, not by bare title.
