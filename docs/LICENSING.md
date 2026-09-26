# Licensing decision

> Not legal advice. This is an engineering analysis to support a decision; have a
> lawyer review it before relying on it commercially.

## What the owner wants

Aural is meant for **personal use**. People should not **sell, repackage or
redistribute it commercially** without permission.

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
| **PolyForm Noncommercial 1.0.0** | yes | **not allowed** (licensor can grant separately) | allowed for noncommercial purposes, with notices | **recommended** |
| PolyForm Strict 1.0.0 | yes | not allowed | **not allowed** (no distribution, no changes) | stricter; blocks forks and community builds |
| Business Source License 1.1 | configurable | restricted until a "change date", then becomes open source | allowed | more moving parts; time-limited |
| Creative Commons BY-NC 4.0 | yes | not allowed | allowed | Creative Commons advises against it for software (no patent terms) |
| Custom EULA / "all rights reserved" + public source | as written | as written | as written | most control; needs a lawyer to write |

**Recommendation: PolyForm Noncommercial 1.0.0.** It is a standard, lawyer-drafted,
short license built for exactly this ("use it freely, but not commercially"). It has an
SPDX identifier (`PolyForm-Noncommercial-1.0.0`), so tooling and WinGet understand it,
and it leaves you free to sell commercial licenses separately.

## Consequences — these need your approval

1. **Aural stops being "open source".** The OSI definition forbids restricting
   commercial use. The accurate term is **source-available**. The original brief said
   "open source"; that wording has to change everywhere (README, repo description).
2. **"Noncommercial" is broader than "personal only".** PolyForm Noncommercial also lets
   charities, schools, public research bodies and government use Aural for free. If you
   want *personal use only*, choose PolyForm Strict (which also bans redistribution and
   modification) or have a custom license written.
3. **Free redistribution stays allowed.** Under PolyForm Noncommercial someone may share
   the installer or a modified build for free, as long as they keep the notices and
   make no money from it. PolyForm Strict would forbid that too.
4. **Contributions.** Contributors would license their changes to you under the same
   noncommercial terms, so you could not sell commercial licenses that include their
   code without a contributor license agreement (CLA). Until you decide, CONTRIBUTING.md
   says pull requests aren't accepted.
5. **Code signing.** The SignPath Foundation's free signing is for OSI-licensed projects
   only, so it is no longer available. The alternatives are Azure Artifact Signing
   (about US$10 a month; individuals were eligible only in the US and Canada when
   checked), a commercial OV certificate, or staying unsigned (SmartScreen warnings).
6. **Who is the licensor?** The copyright line reads "Aurathex". That only works if
   Aurathex is you (a trading name) or a legal entity you control; otherwise use your
   legal name. The WinGet publisher "Aurathex" and identifier `Aurathex.Aural` should
   match whatever you choose, because they are hard to change later. Neither "Aural"
   nor "Aurathex" has been checked for trademark conflicts.
7. **AI-assisted code.** Much of the code was written with an AI assistant under your
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

## What was changed in this commit

Everything below is in a single commit, so it can be dropped or swapped (for example
to PolyForm Strict) before anything is published:

- `LICENSE.md`: the PolyForm Noncommercial 1.0.0 text, unmodified, plus the required
  notice line.
- `LICENSE-MIT` and `LICENSE-APACHE` removed.
- `Cargo.toml` and `package.json` license fields set to `PolyForm-Noncommercial-1.0.0`.
- README, CONTRIBUTING and the About page updated. The WinGet `License` field already
  uses this identifier in `release.yml`.
