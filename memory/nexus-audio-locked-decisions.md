---
name: nexus-audio-locked-decisions
description: Locked architecture decisions for the NEXUS//AUDIO player project
metadata:
  type: project
---

NEXUS//AUDIO is a Rust+egui retro-CRT music & audiobook player the user is building as a Linux-first replacement for MusicBee (MusicBee is Windows-only; user is moving to Linux). Plan lives in NEXUS_AUDIO_IMPLEMENTATION_PLAN.md.

Locked decisions (user-confirmed, do not relitigate):
- **Library scale: 50k+ tracks.** SQLite (`rusqlite` bundled) + FTS5 + indexed sort/paging. Virtualized list rendering mandatory. Small mutable state (playlists/resume/queue/settings) stays atomic JSON.
- **Audio engine: symphonia + cpal — LOCKED (2026-05-18).** Phase 1 spike passed decisively on a real 5h M4B (AAC): seek to 4h12m landed within 29 ms, narration coherent; also clean on MP3 (native-48k pure decode) and AAC M4A. rodio never needed.
- **Known finding — HE-AAC/SBR:** symphonia decodes HE-AAC at the base-layer rate (the sample M4B's header said 44100 but decoded at 22050; SBR not applied). Fine for spoken-word audiobooks; would lose the top octave for HE-AAC *music*.
- **HE-AAC duration bug (found Phase 6 spike, 2026-05-18):** symphonia derives M4B duration as n_frames/sample_rate using the SBR-doubled rate → **HALF the real length** (sample book: symphonia 18809 s vs true 37618 s). Position & seek ARE real-time-correct (device plays real wall-clock; 29 ms seek spike proves it) — only duration was wrong. **Fix: read authoritative duration from MP4 `moov>mvhd` (`audiobooks::chapters::mp4_duration_secs`) for MP4/M4B; engine takes a duration override for audiobooks.** Non-MP4 uses lofty/symphonia duration.
- **M4B chapters:** Nero `chpl` rare; real Audible/iTunes M4B uses a QuickTime chapter **text track** (audio trak `tref>chap` → text trak sample table). Hand-rolled parser in `audiobooks::chapters` handles both; validated on the real sample (19 chapters correct). QuickTime parser is the one that matters.
- **Known finding — resampler required:** the real engine MUST use a proper anti-aliased resampler (e.g. `rubato` sinc). The spike's crude linear resampler aliased audibly on bright/distorted 44.1k→48k music; clean paths (no resample, or low-HF speech) were unaffected. This is an engine work item, not an engine defect.
- **Music seeking is best-effort / low priority. Audiobook auto-resume is THE critical correctness requirement** and the project's acceptance test. Resume lands in Phase 3, not deferred.
- **No playback speed control** — out of scope; no time-stretch subsystem. No tokio.
- **Tag editing deferred to Phase 8 (optional).** File is canonical for tags; DB is a cache keyed by (path, mtime, size). `lofty` write capability retained so write-back stays possible.
- **De-scoped features (user, 2026-05-18) — do NOT build unless reversed:** command palette, scanline overlay, animated EQ bars, mockup-style Settings *panel* (a functional Settings *view* IS wanted), track ratings, play-count. Full prioritized backlog lives in the plan's "Phase 7+ — User Backlog" section. Next actionable: fix MP3 audiobooks leaking into All-Tracks (music scanner must exclude `audiobook_folders`).
- **No cover art — OUT OF SCOPE (user decision, 2026-05-18).** Albums view stays text/ASCII only. User explicitly wants to keep it simple. Do not add image rendering; the `egui_extras` image feature can stay unused. Reviewed the Phase 1 CRT shell and Phase 2 library views: user reaction "looks amazing so far."

**Why:** these reshaped the original plan significantly; re-deriving them wastes a turn.
**How to apply:** treat the plan's §1 decision list as authoritative. Known soft spot: M4B chapter-atom timestamp parsing (Phase 6) likely needs hand-rolled MP4 atom parsing — lofty may not surface chapter times.
