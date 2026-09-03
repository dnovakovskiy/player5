# iOS multicast entitlement

Pro DJ Link and Ableton Link discover peers with UDP broadcast/multicast.
On iOS 14+ that requires the `com.apple.developer.networking.multicast`
entitlement, which Apple grants on request and which takes weeks. The
brief says to file immediately even though iOS is the last platform.

## Action (human, cannot be automated)

1. Sign in to the Apple Developer account that will publish player5.
2. Submit the request at
   https://developer.apple.com/contact/request/networking-multicast
3. Describe the use: a DJ drum machine that joins a Pro DJ Link / Ableton
   Link network on the local booth LAN to follow tempo and beat phase;
   discovery uses UDP broadcast (Pro DJ Link, port 50000) and IPv4 multicast
   (Ableton Link). No internet traffic; no user data.
4. Note the request date and ticket here once filed.

| Field | Value |
|-------|-------|
| Filed on | _pending_ |
| Ticket / reference | _pending_ |
| Granted on | _pending_ |

## After approval

- Add the entitlement to the iOS target's `.entitlements` file and the App
  ID's capabilities in the developer portal.
- Session 7 will not start network features on iOS without it; the iOS
  shell can still run the internal clock.
