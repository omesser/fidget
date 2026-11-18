# Vendored libraries

`src/` is the frontend dist (`src-tauri/tauri.conf.json`'s `frontendDist`), so it
is served raw: there is no build step, no bundler and no `node_modules` at
runtime. The CSP is `default-src 'self'`, which also rules out a CDN import. A
library therefore has to arrive here as one self-contained ES module file,
imported by a relative path.

Every file in this directory is byte-for-byte what npm publishes, so its hash
is checkable against the registry. Do not edit one — re-vendor instead, and
update the line below.

## `marked.esm.js`

`marked@18.2.0`, `lib/marked.esm.js` — 85 lines, 50,508 bytes, no imports, no
runtime dependencies.

```sh
npm pack marked@18.2.0
tar xzOf marked-18.2.0.tgz package/lib/marked.esm.js > src/vendor/marked.esm.js
```

| | |
| --- | --- |
| sha256 (file) | `162194ea902a9d6eca6011bb42b5732991ca650e017d0375b76f4204810c6c14` |
| sha256 (tarball) | `f8f191f70f642128653254bf4b511e7fd7d77962b4c2a186d5f192e47f93d800` |
| same file on a CDN | <https://cdn.jsdelivr.net/npm/marked@18.2.0/lib/marked.esm.js> |
| licence | MIT |

Used by `src/markdown.js`, and only through its token API — `Lexer.lex`. The
HTML-string renderer is never called; see that file for why. Semgrep may flag
ReDoS in the minified lexer; patterns are literals, not user-controlled regex
input.

**It is minified.** marked has published no readable ESM since v16, so the
readable alternative is not the same library at a different price — it is
pinning the 15.x line (2,189 lines, 71,905 bytes) and giving up three majors.
What stands in for reading it: the version is pinned, the hash above matches
the registry byte for byte, and marked publishes to npm with SLSA provenance
(`npm view marked@18.2.0 dist.attestations`). The readable original is in
`lib/marked.esm.js.map` in the same tarball, which is not vendored because
nothing but a devtools session would read it.
