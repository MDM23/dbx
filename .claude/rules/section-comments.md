# Section comments in code

Applies to all source code and configuration in the repository.

A file that has grown into several distinct areas may divide them with a banner
comment:

```rs
// -----------------------------------------------------------------------------
//                                SECTION HEADER
// -----------------------------------------------------------------------------
```

## Shape

- The banner ends at the 80th column. The rule lines are dashes filling
  everything between the comment marker and that column, so a banner indented by
  four spaces carries four dashes fewer and still ends at column 80.
- The header is uppercase and centered over the rule lines. Where it cannot be
  centered exactly, the left padding rounds down and the odd column falls to the
  right.
- The header names the area with a noun or a short noun phrase (`REACTIVITY`,
  `PUBLIC SURFACE`), not a sentence, and takes no trailing punctuation. A header
  too long to center is a section that is not one thing.
- One blank line above the banner and one below it.
- Three lines and nothing else. A banner is a signpost to be scanned past, and
  prose boxed inside one competes with the code for attention.
- Where a section needs explaining, the explanation goes below the banner as an
  ordinary comment, wrapped the way every other comment is, with a blank line
  after it. That blank line is the whole difference between explaining the
  section and commenting whichever item happens to come first:

```js
    // -------------------------------------------------------------------------
    //                                REACTIVITY
    // -------------------------------------------------------------------------

    // Push-based: reading a signal inside an effect subscribes that effect,
    // and writing re-runs subscribers on a microtask so a handler that writes
    // several signals causes one DOM pass rather than several.

    const store = new Map();
```

## Comment markers

A banner uses the line comment of its language, or the block comment where the
language has none. In the block form the closing delimiter counts towards the
80 columns.

| Language   | Marker                                      |
| ---------- | ------------------------------------------- |
| CSS        | `/*` and `*/` on each of the three lines    |
| JavaScript | `//`                                        |
| Markdown   | none, headings already divide the document  |
| Nix        | `#`                                         |
| Rust       | `//`                                        |
| Shell      | `#`                                         |
| TOML       | `#`                                         |

```css
/* -------------------------------------------------------------------------- */
/*                               THE FILE LIST                                */
/* -------------------------------------------------------------------------- */
```

## When to divide a file

- Banners are for files a reader navigates rather than reads front to back. A
  file with one job needs none, and a file that needs them needs one per area:
  a lone banner is decoration.
- Sections divide top-level items. Never put one between statements inside a
  function or between declarations inside a rule.
- A banner is not permission for a file to grow. Where a section could move to
  its own module or file without dragging the rest along, move it instead; see
  [rust.md](rust.md).
- Section order is reading order and therefore carries meaning, so sections are
  exempt from [ordering.md](ordering.md). What sits inside a section is not.
- Whatever order the language's rule gives items applies within a section, not
  across the file: a section keeps the private helpers it owns rather than
  sending them to the bottom of the file.
- Everything above the first banner is preamble: file docs, attributes,
  imports. An item that ends up there belongs to no section, so give it one or
  move it down.

## Rust

- Write banners with `//`. Written with `///` or `//!` they become
  documentation, attached to the next item or to the enclosing module, and end
  up in rustdoc.
- A banner goes between items, never between an item and the doc comment or
  attributes that belong to it.
- `rustfmt` leaves comments alone, so a banner never needs `#[rustfmt::skip]`.
- What a section is for belongs in the module docs or on the items themselves,
  where rustdoc publishes it, rather than in a comment under the banner. A
  comment there is for decisions about the code's shape that no reader of the
  documentation needs.
- The trailing `#[cfg(test)] mod tests` is a section like any other and gets a
  `TESTS` banner, so a file that uses banners is divided all the way down. The
  banner sits above `#[cfg(test)]`, which belongs to the module.
