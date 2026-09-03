# Pro DJ Link (CDJ-3000 / XDJ)

Status: sources collected; to be digested in session 4 alongside the
`ProDjLink` clock source and `apps/bridge/`.

Sources to digest:

- Deep Symmetry, *Packet Analysis* (dysentery):
  https://djl-analysis.deepsymmetry.org/ — beat packets (port 50001),
  status packets (port 50002), device announcements (port 50000), the
  high-precision position packets on the CDJ-3000, device-number
  negotiation.
- `beat-link` source (reference behaviour for joining as a device, beat
  finder, virtual CDJ, tempo master handoff):
  https://github.com/Deep-Symmetry/beat-link
- `prolink-connect` (an independent implementation to cross-check):
  https://github.com/EvanPurkhiser/prolink-connect

Fixtures: `pro-dj-link/*.pcap` captures land here with the digest.
