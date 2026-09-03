# Opus Quad

Status: sources collected; to be digested in session 5 alongside the
`OpusQuad` clock source.

Known constraints (from the project brief, to be confirmed against the
captures): the Opus Quad does not speak full Pro DJ Link; it talks to
rekordbox lighting mode, which we impersonate. That yields BPM and beat count
but only coarse phase (±200 ms), so the clock source interpolates.

Sources to digest:

- `kyleawayan/opus-quad-pro-dj-link-analysis` (packet captures and notes):
  https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis
- `beat-link` Opus Quad support discussion and code:
  https://github.com/Deep-Symmetry/beat-link

Fixtures: `opus-quad/*.pcap` captures land here with the digest.
