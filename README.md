# rssite

A static site generator designed from day one to be **transparent to downstream build systems**.

## The core idea

Unlike mkdocs, Hugo, Sphinx, Jekyll — which are opaque black boxes to build systems ("I'll produce something in `_site/`, ask me later") — rssite will:

**Enumerate every file it will produce *before* it runs, as a pure function of configuration and sources.**

This one design choice gives build systems (rsconstruct, Ninja, Bazel, custom Make) everything they need to:

- Cache each generated page individually (precise incremental builds)
- Compose with other processors that also write into the same output directory
- Model true cross-processor dependencies (e.g., a linter can depend on `_site/about.html`)
- Run per-page work in parallel (when applicable)
- Detect output conflicts at graph-build time instead of silently corrupting caches

## Status

Not yet implemented. This README is the design spec that guides the implementation.

## The contract

rssite exposes three modes:

```
rssite plan           # reads config, scans sources, prints JSON manifest to stdout, exits 0
rssite build          # runs plan internally, then builds; errors if outputs diverge from plan
rssite build --plan   # prints manifest AND builds in one invocation (single-pass)
```

### Manifest schema

```json
{
  "version": 1,
  "outputs": [
    {
      "path": "_site/index.html",
      "sources": ["docs/index.md", "templates/default.html", "rssite.toml"]
    },
    {
      "path": "_site/about/index.html",
      "sources": ["docs/about.md", "templates/default.html", "rssite.toml"]
    },
    {
      "path": "_site/assets/style.css",
      "sources": ["assets/style.scss", "assets/_vars.scss"]
    }
  ]
}
```

- **`version`** — integer schema version. Incremented on breaking changes. Consumers pin the major version they support.
- **`outputs`** — array, one entry per file rssite will produce. Order is deterministic (sorted by path).
- **`outputs[].path`** — output file path relative to the project root.
- **`outputs[].sources`** — every input file whose change would affect this output's content: markdown source, template, stylesheet partial, config file, plugin data, etc.

The `sources` array is the gold nugget — it lets downstream build tools wire per-file dependencies precisely. Without it, the best any build tool can do is "rebuild everything when any source changes."

## Design invariants

These are **contracts** rssite must uphold. The feature is only valuable if these hold.

### 1. Plan is a pure function of inputs

Given the same configuration file and the same source tree contents, `rssite plan` must produce the same manifest — bit for bit. No:

- Network I/O
- Timestamps in the output
- Non-deterministic iteration
- Environment variable reads (unless declared as sources)

Why: cache keys must be stable. Plan output is fed into content-addressed caches.

### 2. Plan is cheap (or at least cached)

Computing the plan may need to parse every markdown file (for front matter, tag aggregation, redirects). At scale that is slow. Two escape hatches:

- **Internal cache**: rssite caches its own plan keyed on `hash(config) + hash(source tree)`. Re-running `plan` is fast when nothing changed.
- **Single-pass mode**: `rssite build --plan` produces both manifest and outputs in one invocation, amortizing the plan cost with the build.

### 3. Built outputs must match the plan

When `rssite build` runs:

- Every file in the plan's `outputs[]` MUST be produced.
- No file outside the plan may be produced.

Violations are hard errors (`--strict-manifest`, default on in CI). Silent divergence here corrupts downstream caches.

Two concrete modes:

- `--strict-manifest` (default): divergence → exit non-zero, leave partial output where it is for debugging.
- `--loose-manifest`: divergence → warning to stderr, continue. Use only for local development while iterating on plugins.

### 4. Plan and build share the same code path

The single most dangerous failure mode in this design is plan/build divergence — the plan says one thing, the build does another.

Guard against it structurally: **the plan function IS the first phase of the build function.** Internally, rssite should have a single `generate_file_plan(config, sources) -> FilePlan`, and both `plan` and `build` commands invoke it. The build pass then iterates the plan and produces each file.

Plugins follow the same rule (see below).

### 5. Variable outputs are deterministic

Some output types depend on content:

- Tag index pages (one per tag found in front matter)
- Archive pages (one per year/month found in dates)
- RSS/Atom feeds (contain hashes of included content)
- Search index (content-derived)

These are fine — they're pure functions of the source tree. The plan pass must parse enough of each source to enumerate them.

If an output can only be known after generating other outputs (e.g., a sitemap referencing pages), that's allowed but should be modeled explicitly: the sitemap's `sources` is the set of all its contributing pages' sources. The plan still enumerates it.

## Plugins

If rssite supports plugins (likely yes), the plugin API must be **plan-aware from day one**.

A plugin registers:

```rust
trait Plugin {
    /// Contribute to the plan. Called during `plan` and during `build`'s
    /// first phase. Must be deterministic given the same inputs.
    fn contribute_outputs(&self, ctx: &PlanContext) -> Vec<PlannedOutput>;

    /// Actually produce the files declared in contribute_outputs.
    /// Called only during `build`. MUST produce exactly the paths
    /// it declared in contribute_outputs, no more and no less.
    fn build_outputs(&self, ctx: &BuildContext) -> Result<()>;
}
```

This dual requirement is what mkdocs-style plugin ecosystems get wrong: plugins can override `on_files` but the output-listing logic is separate from the file-writing logic. In rssite's design, both MUST come from the same plugin, and the framework verifies consistency.

## Non-goals

- **Drop-in replacement for mkdocs/Hugo/Jekyll.** This is a new tool with different guarantees. Existing theme/plugin ecosystems do not transfer.
- **Support for all possible outputs.** If a plugin fundamentally cannot enumerate its outputs without running the full generation, that plugin is incompatible with rssite's design. (Most can; some genuinely can't.)
- **Dynamic/runtime content.** rssite generates static files. No server-side rendering, no runtime template evaluation, no per-request generation.

## Integration example: rsconstruct

Once implemented, rsconstruct can integrate like:

```toml
[processor.rssite]
command         = "rssite build"
predict_command = "rssite plan"
output_dirs     = ["_site"]
```

rsconstruct runs `rssite plan` at graph-build time, promotes each planned output to a declared product output with its own `sources` dependencies, caches per-file, and plays nicely with other processors writing to `_site/`.

Other tools (Ninja generator, Bazel macros, Make meta-rules) can follow the same pattern.

## Open design questions

- **Config format**: TOML? YAML? Something bespoke? TOML is likely, to match rsconstruct's config language.
- **Template language**: Tera? MiniJinja? Hand-written? Tera is Rust-native and well-maintained.
- **Markdown engine**: `pulldown-cmark` (fast, CommonMark) is the obvious choice for Rust.
- **Should `sources` include the rssite binary itself?** Arguably yes — a different rssite version may produce different outputs. Probably handled by the downstream build tool rather than inside the manifest.
- **Incremental plan caching**: how fine-grained? Whole-tree hash vs. per-file hashes with smart re-planning?

## Prior art / what we're learning from

- **mkdocs**: excellent developer experience, opaque outputs, plugin APIs that couple rendering and file listing incorrectly.
- **Sphinx**: configurable, `conf.py` is literal Python (hard to analyze statically), genuinely incremental builds but project-scoped.
- **Hugo**: very fast, but rebuilds everything on each invocation; no partial build.
- **Jekyll**: Liquid permalinks + front matter — manifest could in principle be computed, but the tool doesn't expose one.
- **Bazel's `skylib`**: rules declare outputs up front via `attrs.output()` and family. This is exactly the pattern rssite adopts at the tool level.

## Contributing

This is a solo design effort for now. Once the core plan/build split is implemented, contributions welcome.

## License

MIT (to be committed with the license file).
