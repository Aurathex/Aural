# Licensing decision

> Not legal advice. This is an engineering analysis to support a decision; have a
> lawyer review it before relying on it commercially.

## What the owner wants

Decided by the owner on 2026-09-26:

- The source is public on GitHub.
- Personal and other noncommercial use is free.
- The license grants no right to modify Aural and no right to redistribute it.
- Commercial use needs a permission or license from Aurathex.
- The Aural and Aurathex names and branding are reserved separately; the software
  license does not cover them.
- Third-party dependency and model licenses are preserved.

## Why MIT / Apache-2.0 cannot stay

Both licenses expressly allow anyone to use, copy, modify, sell and sublicense the
software, commercially or not. You cannot keep either license and also say
"no commercial redistribution"; the license text would contradict the README. Anyone
who received a copy under MIT/Apache keeps those rights for that copy forever.

**Current position:** the repository has never been pushed or shared (no remote, no
GitHub repository, one author), so nobody holds an MIT/Apache copy yet. Changing the
license now is clean. After the first public push it would only apply to later versions.

## Options

| Option | Personal use | Commercial use / resale | Free redistribution & forks | Fit |
|---|---|---|---|---|
| MIT or Apache-2.0 (current) | yes | allowed | allowed | fails the requirement |
| PolyForm Noncommercial 1.0.0 | yes | not allowed (licensor can grant separately) | allowed for noncommercial purposes, with notices | first recommendation; lets others share and modify |
| **PolyForm Strict 1.0.0** | yes | **not allowed** (licensor can grant separately) | **not allowed** (no distribution, no changes) | **chosen** |
| Business Source License 1.1 | configurable | restricted until a "change date", then becomes open source | allowed | more moving parts; time-limited |
| Creative Commons BY-NC 4.0 | yes | not allowed | allowed | Creative Commons advises against it for software (no patent terms) |
| Custom EULA / "all rights reserved" + public source | as written | as written | as written | most control; needs a lawyer to write |

**Decision: PolyForm Strict 1.0.0.** PolyForm Noncommercial was recommended first, but it
lets anyone share and modify Aural for free, which the owner does not want. PolyForm
Strict is the same standard, lawyer-drafted family with the same noncommercial and
personal-use grant, but it grants no right to distribute or change the software. It has
an SPDX identifier (`PolyForm-Strict-1.0.0`), so tooling and WinGet understand it, and it
leaves Aurathex free to sell commercial licenses separately.

## Consequences — these need your approval

1. **Aural stops being "open source".** The OSI definition forbids restricting
   commercial use. The accurate term is **source-available**. The original brief said
   "open source"; that wording has to change everywhere (README, repo description).
2. **"Noncommercial" is broader than "personal only".** PolyForm Strict, like
   Noncommercial, also lets charities, schools, public research bodies and government
   use Aural for free. Limiting it to individuals would need a custom license.
3. **No redistribution or changes, by anyone but Aurathex.** Nobody may share the
   installer, publish a modified build, or even change it for their own use under the
   license. Fair-use rights are unaffected. The official download (GitHub Releases and a
   WinGet manifest that points there) is Aurathex distributing, so it is not affected.
4. **GitHub's own terms still apply.** Publishing a public repository gives other GitHub
   users a license under GitHub's Terms of Service to view it and fork it within GitHub,
   whatever `LICENSE.md` says. That permission is limited to GitHub's own features;
   outside GitHub, only the Aural license applies.
5. **Contributions.** The license does not let anyone change the code, and there are
   no contributor terms (for example a CLA), so CONTRIBUTING.md says pull requests
   aren't accepted.
6. **Code signing.** The SignPath Foundation's free signing is for OSI-licensed projects
   only, so it is no longer available. The alternatives are Azure Artifact Signing
   (about US$10 a month; individuals were eligible only in the US and Canada when
   checked), a commercial OV certificate, or staying unsigned (SmartScreen warnings).
7. **Who is the licensor?** The copyright line reads "Aurathex". That only works if
   Aurathex is you (a trading name) or a legal entity you control; otherwise use your
   legal name. The line in `LICENSE.md` still starts with `Required Notice:`, a label
   carried over from PolyForm Noncommercial; PolyForm Strict has no notice clause, so it
   now reads as a plain copyright notice. The WinGet publisher "Aurathex" and
   identifier `Aurathex.Aural` should match whatever you choose, because they are hard
   to change later. Neither "Aural" nor "Aurathex" has been checked for trademark
   conflicts.
8. **AI-assisted code.** Much of the code was written with an AI assistant under your
   direction. In some jurisdictions purely AI-generated material may not be
   copyrightable, which could weaken enforcement of any license on those parts. Your
   review, selection and changes strengthen your claim; a lawyer can advise.

## What the license does not cover

Aural's license applies only to Aural's own code and assets (including the logo).
Everything else keeps its own terms, and those terms are respected as follows.

- **Rust and npm dependencies** (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0,
  MPL-2.0 and similar). These permit use in a noncommercial product; their notices ship
  in `THIRD_PARTY_NOTICES.md`. `cargo deny` blocks copyleft and unknown licenses.
- **Speech models.** Parakeet is CC BY 4.0 (NVIDIA) and Whisper is MIT (OpenAI).
  Users download these directly from Hugging Face under those licenses; Aural does not
  redistribute them and cannot restrict them. The required attribution is shown in the
  app (About → Credits) and in `THIRD_PARTY_NOTICES.md`.
- **Bundled Microsoft files:**
  - `DirectML.dll` is the DirectML redistributable, which ONNX Runtime requires.
  - `msvcp140*.dll` and `vcruntime140*.dll` are the Visual C++ runtime.

  Microsoft permits redistributing both as part of an application, unmodified and not
  as a standalone product. We ship them only inside the installer.
- **Mozilla CA certificates** are compiled in for HTTPS downloads (CDLA-Permissive-2.0),
  with attribution in the notices.
- **WebView2** is installed by Microsoft's own bootstrapper; it is not redistributed by
  Aural.

## What was changed

The license switch is kept in isolated commits so it can be reviewed or reverted on
its own:

- `LICENSE.md`: the PolyForm Strict 1.0.0 text, unmodified (from
  github.com/polyformproject/polyform-licenses), under the existing copyright line.
  It replaced PolyForm Noncommercial 1.0.0, which had replaced MIT / Apache-2.0.
- `Cargo.toml`, `app/package.json`, `release.yml` (WinGet `License`) and the WinGet
  packaging docs use `PolyForm-Strict-1.0.0`.
- README, CONTRIBUTING and the About page describe the Strict terms.