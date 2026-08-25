---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/clippy.toml"
---

# Rust

Applies to all Rust code and crate manifests in the repository.

The baseline is the
[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and the
[official style guide](https://doc.rust-lang.org/style-guide/). Where those
are silent, the taste is borrowed from the best-regarded async server and
reactive UI crates: a small core trait with a single required method,
combinators as default methods on an extension trait, builders that consume
`self` and return `Self`, composition that is free at runtime, macros whose
input reads like their output, and no surprises at the call site.

## Tooling

- `rustfmt` with default settings decides all formatting. Never hand-format,
  never add `#[rustfmt::skip]` without a comment saying why.
- Code must be clean under `cargo clippy --all-targets --all-features`.
  Warnings fail CI.
- Lint levels live in `[workspace.lints]`; every crate opts in with
  `lints.workspace = true`. `clippy.toml` holds only thresholds and MSRV.
- The toolchain is pinned by the flake, not by ad-hoc `rustup` installs; see
  [nix.md](nix.md).

The workspace lint table below is the starting point. Lint groups need
`priority = -1` so that individual entries can still override them.

```toml
[workspace.lints.rust]
missing_debug_implementations = "warn"
missing_docs = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }
unreachable_pub = "warn"
unsafe_code = "forbid"
unused_must_use = "deny"

[workspace.lints.clippy]
await_holding_lock = "warn"
dbg_macro = "warn"
doc_markdown = "warn"
enum_glob_use = "warn"
exit = "warn"
filter_map_next = "warn"
fn_params_excessive_bools = "warn"
implicit_clone = "warn"
inefficient_to_string = "warn"
macro_use_imports = "warn"
manual_let_else = "warn"
match_same_arms = "warn"
match_wildcard_for_single_variants = "warn"
mem_forget = "warn"
missing_errors_doc = "warn"
missing_panics_doc = "warn"
must_use_candidate = "warn"
needless_continue = "warn"
needless_pass_by_value = "warn"
option_option = "warn"
redundant_clone = "warn"
return_self_not_must_use = "warn"
semicolon_if_nothing_returned = "warn"
str_to_string = "warn"
todo = "warn"
trivially_copy_pass_by_ref = "warn"
unnested_or_patterns = "warn"
unused_self = "warn"
unwrap_used = "warn"
use_self = "warn"
```

Relax a lint locally with a narrowly scoped `#[expect(..., reason = "...")]`
on the smallest item that needs it, never with a crate-wide `#![allow]`.
Tests may allow `clippy::unwrap_used` and `clippy::expect_used` on the `mod
tests` item itself.

## Crate layout

- `lib.rs` contains crate docs, module declarations and re-exports. No logic.
- Use `foo.rs` plus `foo/` for submodules. No `mod.rs` files.
- Modules are private by default; the crate root curates the public API with
  explicit `pub use`. Never `pub use module::*` in the public surface.
- Internals are `pub(crate)`. `unreachable_pub` keeps that honest.
- Public paths are part of the API: moving a type between private modules must
  not change how callers name it.
- Order inside a file: crate docs, attributes, imports, `main` in a binary
  crate, public types, public impls, private helpers, `mod tests`. Whoever
  opens a binary lands on the entry point instead of scrolling for it.
- Constants and statics sit directly above the item that uses them, or in the
  preamble right after the imports when the whole file uses them.
- Files long enough to navigate rather than read get section comments; see
  [section-comments.md](section-comments.md).
- Cargo features are additive and never remove items. Name them for what they
  add (`json`, not `with-json` or `use-json`), and annotate gated items with
  `#[cfg_attr(docsrs, doc(cfg(...)))]`.

## Naming

- Follow RFC 430 casing. Conversions use the `as_` (borrow), `to_` (expensive
  or owned) and `into_` (consume) prefixes exactly as std does.
- Getters are `field()`, setters are `set_field()`. No `get_` prefix.
- Iterator methods are `iter`, `iter_mut`, `into_iter`, and the returned types
  are named after them (`Iter`, `IterMut`, `IntoIter`).
- Extension traits end in `Ext`. Trait names are verbs or capabilities, type
  names are nouns.
- Keep word order consistent across the crate: pick `verb_object` or
  `object_verb` once and never mix.
- Lifetimes get meaningful names (`'req`, `'buf`), not `'a`, once more than one
  is in scope.

## API design

- Make illegal states unrepresentable. Newtypes over bare `String`, `u64` and
  `bool`; enums over stringly-typed parameters.
- Arguments convey meaning through types. A call site reading `f(true, false)`
  is a bug in the signature, not in the caller.
- Structs keep private fields and expose constructors. Public enums and
  field-public structs that may grow are `#[non_exhaustive]` from day one.
- Traits that exist to be used, not implemented, are sealed. That keeps adding
  methods a non-breaking change.
- Be generic in what you accept and concrete in what you return: take
  `impl Into<String>`, `impl AsRef<Path>`, `impl IntoIterator<Item = T>`, and
  return a named type the caller can name too.
- The caller decides where data lives. Do not allocate or clone on their
  behalf; hand back borrows or iterators and let them collect.
- Derive eagerly: `Clone`, `Copy`, `Debug`, `Default`, `Eq`, `Hash`, `Ord`,
  `PartialEq`, `PartialOrd`, wherever the semantics hold. Every public type
  implements `Debug`.
- Put bounds on the `impl`, not on the struct definition.
- `#[must_use]` where dropping the value loses work that cannot be got back,
  with a message saying what to do about it. Put it on the type, not on every
  function returning one: `Effect`, `Js` and `Markup` carry it, so nothing that
  builds one has to. A pure getter does not get one; discarding it is dead code
  a reader sees, and the attribute costs a line on every accessor in the crate.

## Errors and panics

- Libraries define their own error types with `thiserror`. `anyhow` is allowed
  in binaries, tests and build scripts only.
- One error enum per crate boundary, `#[non_exhaustive]`, with `#[source]` or
  `#[from]` on every wrapped cause so the chain survives.
- `Display` messages are lowercase, without trailing punctuation, and describe
  what failed, not that something failed. The caller adds context.
- Export `pub type Result<T, E = Error> = core::result::Result<T, E>` when the
  crate has one dominant error type.
- No `unwrap`, `expect`, `panic!`, `todo!` or `unimplemented!` on any path a
  user can reach. Where an invariant truly cannot fail, `expect` with a message
  stating the invariant, not the symptom.
- Document `# Errors` for every fallible function and `# Panics` for every
  function that can panic. If neither section is needed, say nothing.
- Validate arguments at the boundary. Destructors never fail and never block.

## Async

- Never block inside `async fn`. Move blocking work to `spawn_blocking`.
- Never hold a `std` lock across an `.await`.
- Prefer returning `impl Future` or a named future over boxing. Box only at
  API boundaries that need object safety.
- Document cancellation safety for anything meant to be used in `select!`.
- Stay runtime-agnostic where it is free; where it is not, put the runtime
  behind a feature.

## Unsafe

- `unsafe_code = "forbid"` is the default and stays that way unless there is a
  measured reason.
- If a crate needs it, downgrade the lint in that crate alone, keep `unsafe`
  blocks minimal, and give every one a `// SAFETY:` comment justifying each
  precondition. Public `unsafe fn` needs a `# Safety` doc section.
- Anything with `unsafe` runs under Miri in CI.

## Macros

- A macro's input reads like its output. If the invocation does not look like
  the thing it builds, write a builder instead.
- Internal helper macros are `#[doc(hidden)]` and prefixed `__internal_`.
- Refer to everything through `$crate::` so the macro works from any scope.
- Item macros accept attributes and visibility specifiers, and work anywhere an
  item is allowed.

## Documentation

- `#![warn(missing_docs)]` on every published crate. Every public item has a
  doc comment; every non-trivial one has an example.
- Crate-level docs open with what the crate is for and a complete, compiling
  example.
- Examples use `?` and return `Result`. Never `unwrap()` in a doc example.
- Name other items with intra-doc links, not with bare backticks, so rustdoc
  can resolve them.
- First line of a doc comment is a single sentence summary, then a blank line.
- Hide noise from rustdoc with `#[doc(hidden)]`, and surface re-exported types
  with `#[doc(inline)]`.

## Dependencies and versioning

- Every dependency is a decision. Prefer std, then a small well-maintained
  crate, then writing it. Pull dependencies with `default-features = false` and
  enable only what is used.
- Versions and features are declared once in `[workspace.dependencies]`; member
  crates use `dep.workspace = true`.
- `Cargo.toml` carries full metadata: categories, description, documentation,
  homepage, keywords, license, repository, rust-version.
- MSRV is declared with `rust-version` and treated as a breaking change.
- Public dependencies (types that appear in your signatures) must be at 1.0 or
  wrapped. Bumping one is a major version bump.
- Check API breakage with `cargo semver-checks` before releasing.

## Testing

- Unit tests go in `mod tests` at the bottom of the file they test; integration
  tests in `tests/` exercise only the public API.
- Doc examples are tests. Keep them compiling.
- Test names say what holds: `rejects_empty_path`, not `test_path_2`.
- No sleeps for synchronization, no reliance on test execution order, no shared
  mutable global state between tests.
