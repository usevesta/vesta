# Pairing & the Secure Link

Status: **design, revised after an adversarial review.** Implemented in `crates/vesta-link` (v1: on the home network; v1.1: away from home).

This document specifies how a phone or tablet becomes a trusted client of a home ("Add a phone"), how it reaches the Hub afterwards, and what an attacker can and cannot do. How the UI and API are served on each device (one-time login code + `HttpOnly` cookie) is covered in [`local-session.md`](local-session.md); this document covers everything between devices.

## Goals
1. **Being in the home is required, but not enough.** A phone joins only by scanning a QR code shown on the wall panel, and adding anyone who is not a time-limited guest needs the approval of an existing admin.
2. **Nothing to type, nothing to understand.** No accounts, passwords, IPs, ports or Wi-Fi settings. The UI says "Add a phone" / "Añadir un móvil".
3. **Mutual authentication on every connection**, by public keys. No shared passwords that can leak.
4. **No new crypto.** iroh (QUIC + TLS 1.3 with raw public keys, RFC 7250) for transport and authentication; the OS CSPRNG for secrets; standard HMAC where a derived value is needed.
5. **Works offline.** At home, pairing and control need no internet and no third-party service.
6. **Revocable and visible.** Every addition is announced to the admins with a one-tap "Remove". Removal takes effect immediately.

## Identities
| Key | Where it lives | Notes |
|---|---|---|
| **Hub key** (Ed25519, iroh `SecretKey`) | Hub data dir, file mode `0600`. On Android: app-private storage encrypted with an Android Keystore key, excluded from Auto Backup (`dataExtractionRules`). | Its public key (iroh `EndpointId`) **is the home's address**. Generated on first start. Treated as **semi-secret**: only paired devices should know it (see threat model). |
| **Client key**, one **per home** (Ed25519) | Encrypted at rest with a key held by the OS keystore: iOS Keychain (`…ThisDeviceOnly`); macOS data-protection keychain (needs a signed app + entitlement); Android Keystore (excluded from backup); Windows DPAPI; Linux Secret Service, falling back to a `0600` file when there is none (headless). | Separate keys per home, so two homes cannot tell they share a phone. Never transmitted. |

Honest limits:
- iroh signs TLS handshakes with the raw Ed25519 key, so the key is in memory while the app runs. Hardware keystores (the Secure Enclave only does P-256) protect it at rest only.
- DPAPI and Secret Service protect against other users and offline disk theft, not against other processes of the same user.
- When a keystore key is lost (restore to a new phone, factory reset), the encrypted file is useless. The app must say "Add this phone again", never fail obscurely.
- Losing the **hub** key means every phone has to be added again. This is why hub key backup is in the v1.x plan, not "later".

## Roles
| Role | UI (en / es) | Can |
|---|---|---|
| Admin | "Can change settings" / "Puede cambiar ajustes" (option on a household member) | Everything, including adding and removing phones and editing screens |
| Member | "Someone who lives here" / "Alguien de casa" | Use everything, including doors (with confirmation); no settings |
| Guest | "Guest until…" / "Invitado hasta…" | Only what the admin allowed, until the expiry |

- The role is **looked up on every request**, never cached per connection.
- Changing or removing a role closes that client's open connections, so a WebSocket cannot outlive a demotion.
- Expiry uses wall-clock time from the network. If the hub has no trustworthy time (no network, no hardware clock), guests count as expired.

## Pairing with a QR code

```
 Wall panel (Hub)                                   Phone (new client)
 ────────────────                                   ──────────────────
 "Add a phone" → pick who it is for
 Member/Admin: needs the settings PIN or approval
   on an existing admin phone (Guests: no approval)
 Opens a pairing window while this screen is open
 (max 10 min). Secret S (128 bit) rotates every
 ~30 s; the current and previous S are accepted.
 Starts announcing itself on the home network.
 Shows QR = { hub EndpointId, S, LAN address hints }
                          ── camera ──▶            Scans QR (system camera or in-app)
                                                   Creates a client key for this home
                ◀── iroh connect, ALPN vesta/pair/1, direct path only ──
 TLS 1.3: the phone verifies the hub key from the QR;
 the hub learns the phone's EndpointId
                ◀── PairRequest { S, suggested name } ──
 Window open? S current or previous? (constant time)
 Marks S as used (kept until it expires)
 Stores client { EndpointId, name, role, expiry }
                ── PairResponse { home id, home name, role } ──▶
 Panel: "<name> added · Undo"                      Phone: "<home name> is ready"
 All admin phones: "<name> was added · Remove"
```

### Why this is safe
- **The QR authenticates the hub to the phone.** The phone only talks to the key printed in the QR, so a fake hub on the network cannot intercept the pairing.
- **S authenticates the phone to the hub.** Only someone looking at the panel now knows it, and it travels only inside the TLS channel to the verified hub key, so it cannot be phished or sniffed.
- **The role comes from the hub.** It is fixed when the window is opened, so the phone cannot ask for more.

### Window rules
- `vesta/pair/1` is accepted **only while a window is open**. At other times the connection is closed right after the handshake.
- **Direct path only:** a pairing over a relay is refused, so a photo of the QR is useless away from the home network. In v1 relays are disabled anyway; in v1.1 the hub checks the connection's selected path.
- **No failure counter for the QR.** S cannot be guessed, and a counter would let anyone who knows the hub's address block "Add a phone" by sending junk. Instead there is one global rate limit on unknown keys.
- **A used S answers loudly.** If a second phone presents an S that was already used, the panel and that phone both say "Someone else used this code", and the panel offers to remove the phone that used it. A photographed QR cannot win silently.
- **0-RTT is never used.** `PairRequest` and commands are never accepted as QUIC early data: the hub never calls `into_0rtt`, and a test asserts it.

### Device name
The phone suggests a name. Since iOS 16 that is usually just "iPhone", so the panel asks for a friendly name right after adding ("Whose phone is this?"). The name is a label, never a security check.

### QR payload
The QR carries a link, so the phone's system camera opens the app directly:

```
https://<pairing domain>/p#<base64url(payload)>
```

- `payload` = `version (1 byte) ‖ hub EndpointId (32 bytes) ‖ secret (16 bytes) ‖ LAN address hints`. Binary keeps the QR small and easy to scan from a wall-mounted screen.
- The part after `#` is never sent to a server, but scripts on the page could read it. So the page at the pairing domain is **static HTML with `Content-Security-Policy: script-src 'none'`**, and only explains how to install VESTA.
- The domain is a project-controlled static site (a GitHub Pages subdomain works). It is a convenience for opening the app, not a dependency: the app can scan the QR itself.
- Because S rotates while the screen is open, someone who leaves to install the app first can come back and scan again.

### Phones or computers without a camera
**There is no numeric code.** An admin sends an **invite link** from their phone through any messenger. The link carries the hub key and a single-use secret limited to the role the admin picked. The admin approves the first connection with one tap ("<name> wants to join · Allow").

## Connecting after pairing
- **Transport:** an iroh connection from the client's per-home key to the hub's EndpointId, ALPN `vesta/api/1`. The hub accepts it only if the remote EndpointId belongs to a paired client that has not expired. The check runs in iroh's `after_handshake` hook. Unknown keys are closed right after the handshake, under the global rate limit for unknown keys.
- **Endpoint configuration:**
  - v1 uses the `Minimal` preset with `RelayMode::Disabled` and no DNS/pkarr publishing. **Nothing leaves the home network.**
  - v1.1 enables relays and the hub's relay-only pkarr record. **Clients never publish.**
  - Client endpoints are created per home and only while needed, so a relay cannot link homes that share a phone.
- **Finding the hub on the home network:**
  1. Cached addresses from the last connection, plus the QR's address hints.
  2. If those fail (the hub's IP changed): a multicast DNS lookup for a **per-home service name derived with HMAC from a secret given at pairing**. A LAN scan then shows "some device", never "a VESTA hub with key X". On iOS this needs the multicast entitlement and goes through Bonjour (`NWBrowser`) in the Swift plugin, feeding iroh's in-memory address lookup. Android needs a `MulticastLock`.
  - The OS will ask for "Local Network" permission (iOS 14+, macOS 15+). The app explains why in plain words before the prompt. If the person says no, the app shows a clear way to fix it.
- **Away from home (v1.1):** iroh DNS/pkarr lookup and relays. iroh tries to hole-punch a direct path; the relay is plan B.
- **What flows inside:** plain HTTP/1.1 and WebSocket, one QUIC bidirectional stream per proxied TCP connection, no custom RPC.
  - **Hub:** each stream is served by hyper (`http1` with upgrades) running the same Axum router, with an `Identity { client EndpointId }` from the authenticated connection. The role is resolved per request. The hub's Host/Origin guard is meant for its own loopback listener, not for tunnelled streams, so authorization never relies on it. `max_concurrent_bidi_streams` is sized for a WebView's keep-alive connections.
  - **Phone:** the `vesta-link` local proxy is a small reverse proxy, not a byte pipe. It **serves the bundled UI itself** and forwards only `/api/*`. It checks the session cookie, the exact `Origin` (port included) and `Host` on every request, including WebSocket upgrades. Then it strips the cookie and forwards the request through the tunnel.
  - **The frontend is byte-for-byte the same on every device.** No HTML or JavaScript supplied by a hub ever runs in the WebView.
- **Multiple homes:** one proxy origin and **one active home at a time**. Switching homes re-targets the proxy and issues a new session. Cookies ignore ports (RFC 6265 §8.5), so separate `127.0.0.1` ports would not keep homes apart anyway.
- **Tauri IPC** is never granted to a `127.0.0.1` origin by pattern. At most the shell grants it to the exact origin it created at runtime, with only the app's own commands. Navigation to any other origin is blocked.
- **Authorization** happens in one place in the hub, from `Identity` and the current role. Sensitive commands also require `confirmed: true` (see `capabilities.md`). On mobile they also require a biometric check in the app.

## Removing a phone
- Removing a phone from the panel or from an admin phone deletes it from the hub's client list and immediately closes its connections. Its next attempt is refused, and the app shows "This phone was removed from <home name>".
- A guest's access ends at its expiry the same way.
- "Remove this home" on the phone deletes its key and entry locally.

## Data at rest
| Where | Contents |
|---|---|
| Hub `clients.json` | EndpointId, name, role, created/expires/last-seen per client. Public keys only. |
| Hub `identity.key` | Hub secret key (see Identities). |
| Phone | Per-home key in the keystore; per-home entry (hub EndpointId, name, role, cached addresses, mDNS name secret). |

Pairing secrets live only in memory, and only while a window is open.

## Threat model
| Who | Can | Cannot |
|---|---|---|
| Someone on the home Wi-Fi (guest, compromised gadget) | See encrypted QUIC traffic. See that some device answers multicast DNS. Spot a pairing window while one is open. | Reach the API (loopback only; the tunnel requires a paired key). Learn or replay pairing secrets. Pair from outside the home network. |
| Anyone who learned the hub's EndpointId (e.g. a removed phone) | Probably learn the home's LAN and public IP candidates: iroh shares address candidates before the `after_handshake` hook rejects the key. Check whether the home is online. | Get past `after_handshake` without a paired key. Block pairing (no failure counter). |
| Anyone on the internet without the EndpointId | Nothing specific to VESTA | Find the home: the key is not published anywhere in v1. In v1.1 only a relay URL is published. |
| Relay / DNS operator (v1.1; number 0 by default, self-hostable) | See which keys talk, when, and how much. See the hub's IP while it holds its relay connection, and clients' IPs. | Read or alter any traffic (end-to-end encrypted to the peers' keys). |
| Another app on the same phone or tablet | Connect to `127.0.0.1` ports | Use the API without the session cookie that only the app's WebView holds. Get past the proxy's Origin/Host/cookie checks. |
| Someone who can touch the panel (visitor, babysitter) | Add a time-limited **guest** phone; every admin is notified | Add a member or admin without the settings PIN or an admin's approval. Stay hidden: every addition is announced. |
| A visitor who photographs the QR | Try to use it from inside the home network | Use it from elsewhere (direct path only). Use it silently: a used code reveals the race on both screens. |
| Thief of a phone | Use the app if the phone is unlocked and the app is open | Open doors without the biometric check. Keep access after removal. |
| Thief of the wall tablet | Impersonate the home to its phones with the stolen hub key | Stay trusted after the home is set up again: re-adding a phone to the new hub offers "Replace <home>", which deletes the old entry. |
| Malicious QR sticker | Make a phone add a fake "home" | Control the real home, learn its keys (per-home client keys), or run code in the app (no hub-supplied HTML/JS, one active home). |

Out of scope for now: a compromised hub OS, malware with root on the phone.

## Planned hardening
- **Hardware-bound confirmation for sensitive commands.** At pairing, the app also registers a P-256 key held by the Secure Enclave or StrongBox and bound to biometrics. Opening a door then needs a signature over the command and a fresh hub challenge. The hub verifies that the biometric check happened instead of trusting the app.
- **Signed hub-key rotation** pushed to current phones. This limits the damage of a leaked EndpointId and is also the base for "move to new hardware".
- **Delay address candidates until the hook accepts:** to be raised with iroh upstream (see the EndpointId row of the threat model).
- **Self-hosted relay** setting.

## Implementation plan
1. **`vesta-link` (v1, LAN only, relays off):**
   - Hub identity and client store (`clients.json`).
   - Pairing window with rotating S and `vesta/pair/1`, with the hook checks.
   - `vesta/api/1` accept loop serving the hub router per stream.
   - Client-side pairing and the reverse proxy.
   - Tests with two in-process iroh endpoints (no relays, no internet), covering: wrong, used and rotated S; a closed window; 0-RTT refused; role changes closing connections; Origin/Host/cookie checks in the proxy.
2. **Panel UI:**
   - "Add a phone": role picker, settings PIN, rotating QR, "Added · Undo".
   - A list of phones with "Remove".
   - The "Someone else used this code" state.
3. **App:**
   - QR scanning (system camera link and in-app scanner).
   - Keystore storage per platform, with backup exclusion.
   - Local Network permission explainer and recovery screen.
   - The home list.
4. **v1.1:** relays and DNS lookup for away from home, invite links, admin approval and addition notifications through ntfy, and hub key backup.

## Open questions
- **Remote video when no direct path exists.** Three candidates:
  - WebRTC with STUN;
  - a local TURN shim over the iroh connection;
  - a fallback stream through the tunnel.

  To be decided in the doorbell spike (v2).
- **Admin approval before push notifications exist (v1).** The settings PIN on the panel is the v1 mechanism; approval on an admin phone arrives with v1.1.
