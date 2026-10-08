# The Harness working directory is its project scope

## Context

ADR-0018 makes the attached Harness headless, and ADR-0022 makes fidget
responsible for launching it. Neither decision says which project the
Harness belongs to. This record does.

For a Harness, the working directory is not launch bookkeeping. Claude looks
up local-scope MCP servers and project-scope configuration against it. That
lookup was measured. By their own documentation, Grok, opencode, and Hermes
do the same kind of project lookup from the directory the process starts in.
User-scope servers and a vendor's own connectors still load wherever the
Harness starts, which is why an attach looks configured while the project
slice is missing. Because fidget starts the process, fidget chooses the
project scope whose configuration the Harness sees.

fidget currently uses its application data folder for that directory.
An earlier decision chose a non-project folder so the character is not flavoured
by a repository it was never asked about. The same choice is why local and
project MCP servers and per-project opt-ins miss under attach. ACP does not
suppress that configuration. The MCP entry fidget supplies (ADR-0023,
ADR-0026) merges with what the directory brings in. It does not replace it.

The Harness session file and the Action Log also live in the application
data folder today. That is coincidence, not a dependency. #782 needs to
change project scope without turning that shared location into shared
ownership.

## Decision

fidget chooses one working directory whenever it starts an attached
Harness. That directory is the Harness's project scope for vendor
configuration. Changing it is a change to Harness behavior, not
process-launch cleanup.

Today, `data_dir()` supplies that working directory. The miss under attach
is this value, not the meaning of the directory. Its replacement belongs to
#782. This ADR does not choose it.

The Harness session file and the Action Log are app persistence, not
project scope. fidget must choose their paths independently of the
Harness working directory. The paths may resolve to the same directory, but
neither may be derived from the other. Changing project scope must not, by
itself, move either record.

## Consequences

A change to the Harness working directory now names the vendor
configuration it makes visible. A reviewer can distinguish a deliberate
project-scope change from an incidental spawn edit.

Keeping the application data folder keeps the Harness repository-neutral
and excludes project configuration. Choosing another scope can admit
repository configuration, tools, and opt-ins into the character's conversation.
#782 owns that product choice.

#782 must separate the persistence paths from project scope before
changing the working directory. Their values can still coincide, but a
project-scope change cannot alter them.

The loss is silent. A Harness does not report configuration it did not
find. User-scope configuration is independent of this choice.

Vendor filenames and lookup rules remain research facts rather than
architecture. Reversing this decision requires a way to choose Harness
project configuration that no longer follows its working directory.

## Considered Options

- **Keep the application data folder as the permanent scope:** This keeps
  the character from being flavoured by an arbitrary repository. It also makes
  local and project MCP servers and per-project opt-ins unavailable, so it
  is not a neutral default worth making permanent.
- **Fold the rule into ADR-0018 or ADR-0022:** Those records own the Chat
  surface and the ACP client. Project scope has its own reversal and
  persistence invariant.
- **Treat the working directory as launch mechanism:** That description
  hides the behavior Harnesses assign to it and invites future changes to
  move project scope or app persistence without review.
