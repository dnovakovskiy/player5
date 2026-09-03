# apps/bridge

Headless relay built from the same `sync` crate. Arrives in session 4.

Plan: joins the booth LAN (Pro DJ Link, later Ableton Link), keeps a clock
estimate, and serves tempo / beat / phase to browser clients over WebSocket
for the web app's `BridgeClock`. Joins the Cargo workspace as a member when
it lands.
