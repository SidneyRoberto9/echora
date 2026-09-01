# ADR 0009: SponsorBlock API usage under a noncommercial content license

## Status
Accepted (risk accepted, not eliminated — see Consequences)

## Context
SponsorBlock segment-skip data (sponsor/intro/self-promo timestamps for a
YouTube video) is only available from the community-run `sponsor.ajay.app`
API. Unlike ADR 0006's dependencies, this isn't a bundled binary — it's a
runtime HTTPS call, so ADR 0006's GPL/linking analysis doesn't apply. The
applicable license instead covers the *data* the API returns: SponsorBlock's
database and API content are CC BY-NC-SA 4.0 (noncommercial, share-alike),
per SponsorBlock's own
[Database and API License wiki](https://github.com/ajayyy/SponsorBlock/wiki/Database-and-API-License).
That page states plainly that a use of the API/database can itself violate
the license (not just redistributing a full copy), and offers to grant a
different license on request for uses that would otherwise conflict.

A licensing-compliance review (2026-09-01) found:
- Echora's own `LICENSE` (PolyForm Strict + addendum) grants no right to
  commercial use at all — its only permitted purposes are "Any noncommercial
  purpose," "Personal Uses," and use by "Noncommercial Organizations." The
  addendum explicitly confirms ordinary end-user operation of an official
  release as a permitted "Personal Uses"/"Noncommercial Purposes" activity.
- Echora carries no monetization signal today (no pricing, ads, paid tier,
  sponsorship gating).
- Precedent exists for this exact integration shape — live, ephemeral,
  per-video API queries at playback time, nothing vendored — from free/
  noncommercial software (e.g. Jellyfin's `jellyfin-sponsorblock` and
  `jellyfin-plugin-tubearchivist-sponsorblock` plugins), with no known
  enforcement action.
- Creative Commons' own guidance on NC licenses is use-based, not
  identity-based: an ephemeral, non-redistributed, non-monetized query made
  during a user's own private listening is a strong noncommercial-use fact
  pattern.

Verdict: **RISK (low), not BLOCKER.** The maintainer's stated practice
(offering alternate licenses on request) is the cheap path to SAFE, but the
maintainer was not contacted — this decision proceeds on the risk-accepted
basis below instead.

## Decision
1. Query the SponsorBlock API directly (K-Anonymity hash-prefix scheme, no
   API key) for the currently-playing video's segments only — no bulk or
   database download, no mirroring.
2. Never persist segment data beyond the current playback session: no disk
   cache, no export, no re-serving to any other user or system. Timestamps
   exist only long enough to drive `seek` commands over Echora's existing
   mpv IPC connection.
3. Attribute SponsorBlock per their requested template, at minimum in
   `THIRD_PARTY_NOTICES.md`, and surface a credit in Settings/About.
4. Keep Echora's distribution model noncommercial (no ads, no paid tier, no
   monetized sponsorship) for as long as this integration ships. If that
   ever changes, re-run this review before shipping the change alongside
   SponsorBlock.
5. No written permission was sought from the SponsorBlock maintainer before
   shipping. This ADR is the record of that decision — revisit and contact
   the maintainer if Echora's usage pattern, scale, or monetization model
   changes.

## Consequences
- Echora gets SponsorBlock segment-skip without vendoring any SponsorBlock
  code or database content.
- The NC risk is not eliminated, only mitigated and accepted — a future
  monetization decision must re-open this ADR before shipping, since it
  would invalidate the noncommercial-use basis this decision rests on.
- This is a technical/licensing analysis, not legal advice. Professional
  legal review is recommended before wide-scale distribution.
