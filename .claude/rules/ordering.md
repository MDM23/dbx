# Alphabetical ordering

Applies to all source code and configuration in the repository.

- Anything whose order carries no meaning is kept in alphabetical order:
  dependency imports, array entries and object keys where the order doesn't
  matter, keys in localization files, attribute sets in Nix modules, entries
  in config lists, and similar.
- Insert new items at their correct alphabetical position; never append at
  the end. This spreads edits across the file and keeps merge conflicts rare
  and trivial.
- Where a formatter or linter already enforces an order (e.g. goimports),
  rely on that tool instead of sorting by hand.
- Never reorder things whose order is semantic: middleware stacks, migration
  steps, matcher/route precedence, CSS cascade, and the like. When unsure
  whether order matters, leave it unchanged.
