# TOFIX

Findings from a code scan on 2026-10-04.

## Low

- `README.md:195` - says the license is "MIT (to be committed with the license file)", but `LICENSE` is already committed; drop the parenthetical.
- `README.md:148` - states rsconstruct "defines a dedicated processor type, MassGenerator", but rsconstruct's own `docs/src/processors/mass_generator.md` says "Designed, not yet implemented" and no rsconstruct source references it; say it is planned.
- `docs/src/features.md:132` - claims the plugin design is "already locked per README §Plugins", but the README section opens with "If rssite supports plugins (likely yes)" (`README.md:121`), and that README section is not part of the mdbook at all; reconcile the wording and link to where the spec actually lives.
