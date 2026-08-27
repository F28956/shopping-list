# Serving HTTPS, and keeping the certificate fresh

**Built**, bar the parts marked Open below. `TLS_MODE` is `off`, `files` or `acme`,
and all three work.

One thing this document did not say and should have: **the plain listener answers
`/healthz` and redirects everything else.** T11 leans on that — a server that cannot
get a certificate serves no HTTPS at all, so redirecting the health check to a port
that will not complete a handshake hides the reason at exactly the moment somebody is
looking for it.

Both arrangements are supported and neither is assumed — T1's default stands, but the
operator guide leads with the reverse proxy, which is the configuration most people
running a home server are already in. In-process TLS is what somebody with a bare box
and one binary reaches for, and it is one variable away.

The server speaks cleartext HTTP on `0.0.0.0:8080` and nothing else. Every request
carries `Authorization: Bearer <google id token>` — a credential that is worth exactly
as much to whoever reads it off the wire as it is to the person who sent it — so this
is the gap that matters most once the address stops being `localhost`, and
[configuration.md](configuration.md)'s C6 turns it from a gap into a blocker: release
builds of the apps will refuse `http://`, which makes TLS a prerequisite for
self-hosting at all.

The other half of the problem is that certificates expire. A design where a person
renews them is a design that works until the first holiday, and Let's Encrypt is
moving towards certificates measured in days rather than months. Rotation has to be
something the process does, not something the operator remembers.

```
  boot                      first handshake            every ~2/3 of a lifetime
  ────                      ───────────────            ────────────────────────
  read the cached           order, validate,           renew in the background,
  account key and    ──→    serve — the caller    ──→  swap on the next
  certificate               waits, nobody else does    handshake, no restart
```

## Where this stands today

`main` binds one `TcpListener` and hands it to `axum::serve` with the composed router.
`web::cookie_secure()` already defaults to `Secure` and needs `SESSION_INSECURE=true`
to be talked out of it, which is to say the browser half is already written for a
world that has TLS and is currently living on the exception. Android reaches the
development machine through a cleartext exception naming `10.0.2.2`. Nothing anywhere
holds a private key.

---

## T1 · TLS terminates in this process, because the premise is one binary

The alternative — "put Caddy in front of it" — is the right answer for somebody who
already runs Caddy, and it stays supported (T7, mode `off`). It is the wrong default
for this application, whose whole shape is a single executable, one SQLite file, and a
person who wants their shopping list to sync. Requiring a second daemon, a second
configuration language and a second thing to upgrade in order to reach a working
first run is a tax on exactly the person this is for.

So: rustls in the process, ACME in the process, and a proxy is a thing you may put in
front rather than a thing you must.

## T2 · TLS is a property of the listener, and the router does not learn about it

`app()` composes the transports and that composition is a security boundary; it gets
no new arguments here. What changes is only what wraps the accepted socket before
hyper sees it. Two consequences worth stating because they are the payoff: every
existing router test keeps running against a plain in-memory service, and there is no
code path where a handler behaves differently depending on how it was reached.

The one exception is the header set — see T10 — and it is a layer applied at the same
place the other security headers already are.

## T3 · Renewal is folded into the accept loop, not into cron

`rustls-acme` (0.15, `tokio` + `axum` features) does this shape already: an
`AcmeState` polled alongside the listener acquires and renews, and a
`ResolvesServerCertAcme` implementing `rustls::server::ResolvesServerCert` hands the
current certificate to each handshake. A renewal is a new certificate in a resolver
that the next handshake reads; connections in flight are undisturbed and the process
does not restart.

Renew at roughly two thirds of the lifetime, and treat failure as retryable with
capped exponential backoff — never a tight loop. Let's Encrypt limits *failed*
validations to five per account per hostname per hour, and a misconfigured server that
retries hard will spend that budget in seconds and then look broken for reasons that
have nothing to do with the original mistake.

Every ACME event gets a log line at `info`, and the failures get the CA's own words
rather than a summary of them: "the certificate could not be renewed" is not
actionable, and `urn:ietf:params:acme:error:dns — no A record for list.example.com` is.

## T4 · The port the server listens on is not the port validation arrives at

This is the part that decides whether a custom port works, so it is worth being exact.
The service may listen wherever it is told. The challenge does not get a choice: the CA
resolves the name in DNS and connects to **443** for TLS-ALPN-01 or **80** for
HTTP-01, and there is no field anywhere that says "try 8443 instead".

Those two facts are compatible more often than they look, because what has to reach
port 443 is a *packet*, not a *process*:

| Arrangement | Validation | Works |
|---|---|---|
| `PORT=443` | TLS-ALPN-01, on the service listener | yes, nothing to arrange |
| `PORT=8443`, router forwards public 443 → 8443 | TLS-ALPN-01, on the service listener | yes — the challenge arrives on the same socket as ordinary traffic, which is the whole point of ALPN validation |
| `PORT=8443`, only public 80 → 80 forwarded | HTTP-01, on the redirect listener | yes |
| `PORT=8443`, nothing forwarded, CGNAT or no public DNS | neither | no — needs DNS-01 (Open) or mode `files` (T13) |
| Behind a proxy that terminates TLS | none of ours | mode `off`; the proxy holds the certificate |

Two things this rules out. There is no separate challenge listener bound to 443 while
the app serves 8443: if the process can bind 443 it can serve on it, and a design that
binds the privileged port for one purpose but not the other buys nothing and adds a
mode. And a certificate cannot be obtained for a port — names are certified, ports are
not, so `https://list.example.com:8443` is served by the certificate for
`list.example.com` and needs no accommodation beyond the forwarding above.

The port does matter to the clients, and that is already settled: configuration.md's
C3 stores `scheme://host[:port]` as the origin, so `https://list.example.com:8443` is
a thing a person can type on the first screen and a thing a `/join/<token>` link can
carry.

## T5 · Every client sends SNI, so passthrough vhost routing works — unless the address is an IP

Checked rather than assumed, because a proxy in `stream` mode routing on
`ssl_preread_server_name` has nothing else to route on: no SNI means no vhost, and the
symptom is the default backend answering for the wrong site.

| Client | Stack | SNI |
|---|---|---|
| iOS / iPadOS / macOS | `URLSession(configuration: .default)`, no delegate, no pinning (`ios/Shared/Sources/API.swift:56`) | yes — Apple's TLS stack, always |
| watchOS | the same `API` type (`ios/ShoppingListWatch/Sources/WatchApp.swift:10`) | yes |
| Android | `OkHttpClient.Builder()` with only `readTimeout` changed (`android/…/data/Api.kt:61`) | yes — OkHttp sets it on the socket for every connection |
| Browser | anything since IE6 on XP | yes |
| The server itself, calling Google | `reqwest` on rustls | yes — rustls requires a name and refuses to connect without one |

Not one of the four customises `SSLContext`, `sslSocketFactory`, `hostnameVerifier`,
`CertificatePinner` or `URLSessionDelegate`, and there is no ATS exception in any
`Info.plist`. That is the finding that matters: nothing here does anything clever with
TLS, so every client gets the platform's ordinary ClientHello, with SNI and with ALPN.
Any change to that — pinning in particular, which is the tempting one — has to be
weighed against this, since a pinned client and a passthrough proxy are fine but a
pinned client and a *terminating* proxy are not.

**The exception is an IP address.** SNI carries a hostname; RFC 6066 forbids literal
addresses in it, and every stack above obeys that by sending no SNI at all when the
origin is `https://192.168.1.10:8443`. Nothing routes such a connection by name, and
the CA will not certify it under this design either. This is not hypothetical — the
server already logs an IP URL for the LAN (`lan_address`), and it is exactly what a
person on their own network will paste into the first screen. So the address screen
should accept an IP only in the `files`/cleartext-development shape and say plainly
that a name is required for a certificate, rather than letting somebody discover it
after configuring a proxy.

## T6 · Passthrough and termination are different proxies, and they want different validation

Both are supported and the difference is one line of configuration here, but it is not
a detail the operator can be left to find:

| In front | What it does with TLS | This server | Validation |
|---|---|---|---|
| nginx `stream` + `ssl_preread` (passthrough) | forwards the bytes, routes on SNI | `TLS_MODE=acme`, holds the key | TLS-ALPN-01 — the challenge handshake passes through untouched, which is what makes this combination work at all |
| nginx `http` + `proxy_pass` (termination) | terminates, holds its own certificate | `TLS_MODE=off` on a private port | not ours; the proxy renews |
| A terminating proxy, but this server must still hold a public certificate | terminates and re-encrypts | `TLS_MODE=acme` | **HTTP-01, not ALPN** — an L7 proxy answers the ALPN challenge itself and the order fails. `rustls-acme` exposes `UseChallenge` for this |

Two consequences of passthrough worth writing down. ALPN is negotiated end to end, so
this process decides whether HTTP/2 happens — the rustls config needs
`alpn_protocols = ["h2", "http/1.1"]` or every client silently drops to HTTP/1.1,
which for an application built on a long-lived event stream per client is a real cost.
And the peer address becomes the proxy's, so every log line and anything that ever
rate-limits by address sees one client; the fix is the PROXY protocol
(`proxy_protocol on` in the `stream` block, parsed here before the TLS handshake), and
it is a header that must be *required* rather than merely accepted, since a server that
trusts an optional PROXY header from anyone lets anyone claim any address.

## T7 · Three modes, and the mode has to be said out loud

| Variable | Meaning |
|---|---|
| `TLS_MODE` | `off` (default), `acme`, or `files` |
| `PORT` | what to listen on. `8080` today; `443` is the natural value once `TLS_MODE=acme` |
| `TLS_DOMAINS` | comma-separated names to certify. Required for `acme`; every name must resolve here |
| `ACME_CONTACT` | `mailto:` address for expiry warnings. Optional, and worth setting |
| `ACME_DIRECTORY` | `production` (default) or `staging` |
| `TLS_CACHE_DIR` | where the account key and certificate live. Default `./tls` beside the database |
| `TLS_CERT` / `TLS_KEY` | PEM paths, for mode `files` |
| `HTTP_REDIRECT_PORT` | plain-HTTP listener. Default `80`; `off` to not open one |

`off` is the default because a laptop talking to its own simulators must keep working
with no configuration at all, and because a default that tries to reach a certificate
authority from a machine with no domain name is a default that fails slowly. It is
logged at startup in the same breath as `SESSION_INSECURE`, so a server that is
serving cleartext says so every time it starts.

`production` is the default directory rather than `staging`, which is the choice
against the grain and is deliberate: a staging certificate produces a server that
starts cleanly, serves happily, and is rejected by every client — a failure that looks
like a success everywhere except in a browser's advanced-options dialog. Being refused
by the real CA with a real reason is the better first experience. `staging` is
documented as the thing to set *while you are fighting with router forwarding*, which
is the situation the rate limits exist for.

## T8 · The key lives in a directory, not in the database

`DirCache` with mode `0700`, holding the ACME account key and the certificate, checked
at startup and refused if the mode is wider.

Putting them in SQLite is tempting — it would keep "one file to back up" true, which
is a real property of this application. It is still wrong. The database is the thing
that gets copied to a laptop to debug, attached to an issue, and synced to whatever
cloud drive the person uses; the certificate is the one piece of state that is *cheap
to lose* and expensive to leak, and folding it into the irreplaceable file gives it the
opposite handling from the one it wants. Losing the cache directory costs a fresh
order. Leaking the database now costs the private key too.

## T9 · Port 80 does two jobs and neither is serving the application

It answers `/.well-known/acme-challenge/*` when HTTP-01 is in use, and it answers
everything else with `308` to the `https://` origin, method and body preserved. No
route, no session layer, no state — a person who typed the address without a scheme
lands on the real server, and that is all.

If it cannot be bound, that is a warning and not a failure: the redirect is a courtesy,
and HTTP-01 is only one of the two ways in.

## T10 · HSTS goes on only when TLS is actually on

`Strict-Transport-Security: max-age=63072000` when `TLS_MODE` is not `off`, and absent
when it is. Not `preload`, and `includeSubDomains` only behind its own variable: both
are promises a person makes on behalf of a domain they may share with other things,
and the first is close to irreversible in shipped browsers.

This is one more layer in `security_headers()` and it wants the tests that shape
already has — a case asserting the header is present under `acme`, and a case
asserting it is *absent* under `off`, because an HSTS header served over cleartext
development is how you lock yourself out of your own laptop.

## T11 · A server that cannot get a certificate does not quietly serve cleartext

It starts, it binds, it logs the refusal, and handshakes fail until there is a
certificate. It does not fall back to HTTP on the same port.

Falling back would put bearer tokens on the wire in exchange for nothing: the clients
refuse `http://` in release builds anyway (C6), so the fallback cannot even produce a
working app — only a working *leak*. The plain listener from T9 stays up and keeps
answering `/healthz`, and that answer grows enough to be useful to a supervisor and to
a person reading it:

```
GET /healthz → 200  ok
                    tls: acme, list.example.com, expires in 63 days
```

with `tls: acme, list.example.com, no certificate — <the CA's reason>` when that is the
truth.

## T12 · Binding 443 without running as root

The reason to run on a custom port is usually this one, so the answer belongs next to
it rather than in a wiki. On Linux, either `AmbientCapabilities=CAP_NET_BIND_SERVICE`
in the unit file or `net.ipv4.ip_unprivileged_port_start=443`; on macOS, ports below
1024 need root and the practical answer for development is a high port. None of it is
code — it is three lines of the deployment document that stop a person from running
the whole application as root because that was the way that worked.

## T13 · Certificates from elsewhere stay a first-class mode

`TLS_MODE=files` with `TLS_CERT` and `TLS_KEY`, watched for change and reloaded into
the same resolver without a restart.

This is not a leftover. It is the answer for the case C6 hurts most — a server on a
home network, no public name, no inbound port — where the working setup is an internal
CA or `mkcert` and a certificate trusted on the handful of devices involved. Supporting
it costs one resolver implementation and removes the need to tell that person their
setup is unsupported.

## What is testable, and what needs a real CA

Most of it, if the seams are put in the right places:

* the mode/port/domain parsing, in the shape `audiences` already has — a function that
  takes what was configured and returns what was meant, tested without an environment;
* the redirect: a `308`, to the right origin, preserving the path and the method;
* the headers: HSTS present under `acme`, absent under `off`;
* the cache directory: refused when its mode is wider than `0700`;
* `/healthz` reporting each of the three certificate states.

The ACME conversation itself wants [Pebble](https://github.com/letsencrypt/pebble), the
CA's own test server, in a feature-gated integration test: order, validate over
TLS-ALPN-01, serve, force a renewal, assert the served certificate changed without the
process restarting. That last assertion is the one that matters and the one no unit
test can make. Staging remains the manual acceptance step before a first production
order.

## Dependencies, and the one that bites

`rustls-acme = { version = "0.15", features = ["tokio", "axum"] }`, which brings
rustls and `tokio-rustls`.

The trap: `reqwest` is already in this crate with `rustls`, so the binary ends up with
two paths into rustls, and rustls will panic at the first handshake with "no process
level CryptoProvider available" if the crypto backends disagree or none was installed.
Install one explicitly in `main`, before anything else —
`rustls::crypto::aws_lc_rs::default_provider().install_default()` — and pin `reqwest`
to the same backend. It is one line, and finding it the other way costs an afternoon.

## Open

**DNS-01, and the server nobody can reach.** The mode above needs an inbound packet on
443 or 80. A home server behind CGNAT has neither, and DNS-01 is the only validation
that works from a machine with no reachable port — at the cost of an API credential for
a DNS provider, which is a strictly more dangerous secret than the certificate it
obtains. `rustls-acme` does not implement it; `instant-acme` does, with the challenge
plumbing left to the caller. Worth doing once somebody actually needs it, and worth
knowing now that it is the reason mode `files` exists.

**Short-lived certificates and ARI.** Let's Encrypt is heading for certificates
measured in days, and renewal at "two thirds of the lifetime" becomes a renewal every
few days rather than every month or two. The mechanism above survives that — nothing
about it is scheduled by a human — but the retry budget and the rate limits stop being
theoretical, and ACME Renewal Info, where the CA tells the client when to renew, is
what makes it safe. Check whether the client implements ARI before switching to a
short-lived profile.

**Certificate Transparency makes the hostname public.** Every certificate a public CA
issues is logged, publicly and permanently. `list.<surname>.com` is then a searchable
statement that a person runs a server, which is a privacy cost that a personal
application should state rather than discover — and a genuine argument for mode
`files` on a private network. Wildcards blunt it and need DNS-01.

**Multiple names, and changing them.** `TLS_DOMAINS` implies a SAN certificate, and
editing the list means a new order. What happens to the old certificate, and whether a
name that stops resolving should block renewal for the names that still do, is a
decision worth making before it is discovered at renewal time.
