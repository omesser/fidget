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

`marked@18.1.3`, `lib/marked.esm.js` — 84 lines, 49,668 bytes, no imports, no
runtime dependencies.

```sh
npm pack marked@18.1.3
tar xzOf marked-18.1.3.tgz package/lib/marked.esm.js > src/vendor/marked.esm.js
```

| | |
| --- | --- |
| sha256 (file) | `e76f3b37046a5d294159872002e5f6fcda8ed232e1be45ff76b3863f2919dae3` |
| sha256 (tarball) | `5ca94887c584f7a67b27acd6b08e0a19da651418b627295d82d57fe63a56c82b` |
| same file on a CDN | <https://cdn.jsdelivr.net/npm/marked@18.1.3/lib/marked.esm.js> |
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
(`npm view marked@18.1.3 dist.attestations`). The readable original is in
`lib/marked.esm.js.map` in the same tarball, which is not vendored because
nothing but a devtools session would read it.
