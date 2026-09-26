# /reimagine-it exploration — Aural UI (2026-09-26)

- **Source:** `design/aural-ui-source.html`, the real pill states, settings sections, models,
  hotkey and delete phrase.
- **Command:** `npx reimagine-it variations -i design/aural-ui-source.html -n 3 -o design/reimagined/2026-09-26-aural-ui/ --brief "monochrome speed clarity flowing water; native desktop utility, not a web dashboard"`
  (seed 1071685301).
- **Audit:** 01 clean (19/19). 02 and 03 each have 1 advisory warning and no failures.
- **Open:** `npx serve design`, then `/reimagined/2026-09-26-aural-ui/index.html`.

## Critique and decision
All three came out as marketing webpages with serif display type. That is the wrong
register for a small Windows utility, so none is used as-is.
- **01 gradient:** rejected. Gradients and glow are on the spec's kill list (§11a).
- **02 infographic:** kept one idea. Its row of equal unit bars independently lands on the
  pill's 12-bar motif as the strongest monochrome element.
- **03 folio:** kept one idea. A thin line ending in a solid dot becomes the logo: sound
  (a wave) flows and resolves into a point (clarity, the text cursor).

**Implemented direction: "Monolith"** (spec §11a) with those two refinements.
- Near-black capsule pill with a white mark and 12 mirrored hairline bars.
- The settings window is a compact utility sheet in Segoe UI Variable, using grayscale
  plus black/white inversion as the only accent.
