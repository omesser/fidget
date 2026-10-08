# The Character Manifest is TOML, not an invented format

The Character Manifest is TOML: `name` at the root, a table per Animation, a
table per Behavior. No proprietary configuration formats — the `key = value`
dialect invented in #7 read well but had no editor support, no syntax
highlighting, and no answer an author could look up; every rule about it
existed only in the loader.

TOML because it is the Rust ecosystem's house format (Cargo speaks it), the
tooling exists everywhere, and comments survive — every shipped manifest opens
with a doc header. The container is all that changes: the loader still walks
the parsed document itself, rejects every declaration it does not know, and
reports every mistake in one pass in Character-Manifest terms, and the `when`
condition stays this project's own small trigger language inside a TOML
string. No serde: derive-generated errors would replace the author-facing ones.

The `x y then z` Behavior micro-syntax is absorbed: `play` is a list of
Primitives and `then` a key of its own, because a list is the natural TOML
spelling and `then` no longer needs to be a reserved word.

This supersedes the container-format note ADR-0006 makes in passing; its
decision — integer scaling of true pixel art — stands as written.

## Considered Options

- **YAML:** Rejected: its flagship Rust crate is archived, and implicit typing
  is wrong for untrusted hobbyist input.
- **JSON:** Rejected: no comments, and every shipped manifest opens with a doc
  header.
- **Keep the invented format:** Zero dependencies, and the parser was small
  enough to trust. But a format only one program can read is a tax on every
  author, and v2 makes the manifest public.

## Consequences

Manifests gain editor support and a specification someone else maintains, and
duplicate declarations become the parser's problem rather than this module's.
Rejections name the offending declaration by name rather
than by line; syntax errors still carry the line, because a file that is not
TOML has no declarations to name.
