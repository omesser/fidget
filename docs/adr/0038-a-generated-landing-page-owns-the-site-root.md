# A Generated landing page owns the site root

**Amends:** [ADR-0011](./0011-generated-or-dated-site-pages.md). Its two page
classes, its ban on the Described page and its allowlist stand. This ADR
reverses one of its rejected options, the landing page, and moves the design
directory off the root.

## Context

ADR-0011 rejected "a real landing page" as two front doors that drift. The
root was the design-page directory instead, and the README was the only front
door. Since then the site URL is what people share in posts, in search results
and in the repository's About field (#1210). The people who arrive that way
have never opened the repository. They land on a list of design proposals that
calls itself "not a landing page".

The drift objection was against a hand-written page. That is the Described
class ADR-0011 already forbids, so a landing page fails under ADR-0011 only if
someone writes it by hand.

## Decision

The site root is a **Generated** landing page. A script builds it at deploy
from the same sources a contributor edits:

- The headline, the opening paragraph, the feature list and the Harness names
  come from `README.md`.
- The cast and its count come from the Character Manifests. Every sprite on
  the page is a frame a manifest declares.

A source the generator cannot find fails the build, as it does for the
gallery. The README stays the canonical text. The landing page is a rendering
of it, so the two front doors cannot drift.

The page's fixed copy is limited to layout, section headings and their
one-line subtitles, the demo line in the hero bubble, and short cards that
link to the document owning each claim. A product
claim the page makes and the README does not is the Described class, and it
does not belong in the shell.

The landing page names its class in its footer, not in a header line. It is
the visitor's page, and contributor chrome at the top costs every visitor a
line. It is indexed and carries a canonical URL, because it is meant to be the
search result for Fidget.

The design-page directory moves to `/design.html`. Every design page stays
published and reachable from it, and the landing page links it from the footer
only.

## Considered Options

- **Keep the directory at the root and point the About field at the README:**
  A visitor from a post lands on GitHub's file view, with the pitch below the
  file list on a phone.
- **A hand-written landing page:** The Described class. It drifts from the
  README the first time either changes.
- **Ship the Dated mockup at the root:** It is frozen by definition, so it
  would be wrong after the next character or Harness lands.

## Consequences

The README headline, its feature bullets and its Harness table are now inputs
to a public page. Changing their shape can fail the Pages build, and the build
says which source it could not read.

Old links to the root that expected the directory now reach the landing page.
The directory is one click away in the footer. Pre-v1 there is no redirect.
