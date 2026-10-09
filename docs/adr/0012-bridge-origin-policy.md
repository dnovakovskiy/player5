# ADR-0012: Which pages may use the bridge

Status: Accepted (amends [ADR-0007](0007-bridge.md))

## Context

ADR-0007 accepted that "any page a DJ visits could connect to a bridge on
`localhost` and read the tempo or change the follow target". Browsers do
not apply the same-origin policy to WebSockets: a page on any site can open
`ws://localhost:17505/ws`, and the browser only tells the server where the
page came from in the `Origin` header. The follow target is shared by the
whole booth (ADR-0007 §5), so a stray ad or a hostile page in another tab
could switch the drum machine to another deck mid-set, and could hold
connection slots. DNS rebinding (a public name re-pointed at the bridge's
address) defeats a plain "Origin equals Host" check. Framing the
bridge-served app and tricking a click on its Follow menu would bypass an
origin check altogether.

The legitimate clients are few: the app the bridge serves itself (from its
LAN address, `localhost` or a `.local` name), the app on a local dev server,
non-browser tools, and possibly a copy of the app hosted elsewhere that the
DJ deliberately points at the bridge.

## Decision

1. The bridge checks `Origin` on every WebSocket upgrade
   (`http::origin_allowed`). Allowed:
   - no `Origin` header (not a browser page);
   - a loopback host: `localhost`, `*.localhost`, `127.0.0.0/8`, `[::1]`
     (only software on the machine serves those);
   - the bridge's own pages: `Origin` equals the `Host` header and that host
     is an IP literal, a single-label name or a `.local` name, none of
     which a remote attacker can re-bind;
   - origins given with `--allow-origin <origin>` (repeatable; `*` turns the
     check off).
2. A refused page gets the upgrade and then an immediate close with code
   1008 and a reason naming `--allow-origin`, before any clock data; its
   messages are never read. (A refused handshake would tell the page
   nothing; the web app shows the reason with its own origin filled in.)
3. Static responses carry `frame-ancestors 'self'` and
   `X-Frame-Options: SAMEORIGIN`.
4. Connection limits: 64 in total as before, and at most 16 at once from one
   non-loopback address, so one machine (or a slow-loris client, which the
   5 s request-head deadline already bounds) cannot take every slot.
   `/bridge.json` stays readable from anywhere: it only names the protocol
   and the source.

## Consequences

- A copy of the app hosted on another origin (e.g. a static site) can no
  longer use a bridge on `localhost` without `--allow-origin`; the app says
  so in its clock panel.
- Reading the tempo is refused to foreign pages too, which costs nothing.
- Software running on the DJ laptop itself is still trusted, as before.
