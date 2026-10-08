# True pixel art with integer nearest-neighbour scaling

Characters are authored as true low-resolution pixel art on a fixed grid and
rendered with nearest-neighbour filtering at integer scale factors only
(`image-rendering: pixelated`). Assets are PNGs plus a Character Manifest
declaring fps and loop mode per Animation.

The manifest's format is [ADR-0015](./0015-character-manifest-is-toml.md)'s.
This decision is only that the frame grid is fixed and the scale factor is an
integer.

A future reader on a Retina display will see a deliberately blocky sprite and
wonder why it was not drawn at native resolution. This is the reason.

## Considered Options

- **High-resolution sprites in a retro style**: Smooth at any DPI, and it looks
  like a modern app cosplaying as Windows 95. Larger assets, more expensive to
  author, and the pixel grid — which is what made these characters readable at
  32px in the first place — is gone.
- **Per-Character render mode**: Each package declares whether it wants pixelated
  or smooth rendering. Kept possible via a `render_mode` field in the Character
  Manifest, but not implemented now.

## Consequences

Constrains display scaling to integer factors (2×, 3×, 4×), which is exactly what
keeps the art crisp rather than blurred on Retina. Cheapest format to author and
to render, smallest assets, and the constraint is a feature.

The real cost lands on asset *generation*, and it is the top risk to the character
library: image models produce pixel-art-*styled* output at high resolution with
anti-aliased edges and drifting palettes, not grid-aligned sprites — and holding
one character consistent across the 6–8 frames of a walk cycle is harder than any
individual frame. Mitigation to prove on one Character before authoring ten:
generate a high-res reference sheet, then downscale through a scripted
nearest-neighbour plus fixed-palette quantisation pass so every frame lands on
the same grid and palette.

Reversing this means re-authoring every shipped Character.
