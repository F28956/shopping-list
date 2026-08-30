# Running it

Operational instructions, as opposed to design. The reasoning for all of this is in
[docs/self-hosting.md](../docs/self-hosting.md); this is what to type.

## Backups

The arrangement in one sentence: **the server holds a public key, so it can write
backups it cannot read.** Somebody who takes the machine takes a directory of
ciphertext. The private key lives somewhere else — a laptop, a password manager, a
printed page in a drawer — and is needed only to restore.

That is the whole reason for using [age](https://age-encryption.org) rather than a
passphrase. A passphrase good enough to matter has to be stored somewhere, and the
convenient place to store it is the machine being defended against.

### Once

Make a key **on your laptop, not on the server**:

```bash
age-keygen -o ~/.config/shopping-list/backup-key.txt
```

It prints the public key — `age1...`. That half goes on the server. The file it wrote
holds the private half and must never go there.

Keep a second copy of that file somewhere that is not the laptop. It is the only thing
standing between a dead disk and starting again from nothing, and it is small enough
to print.

### Every night

```bash
SHOPPING_LIST_DB=/var/lib/shopping-list/data.db \
BACKUP_DIR=/var/backups/shopping-list \
BACKUP_RECIPIENT=age1... \
ops/backup.sh
```

`BACKUP_KEEP` is how many to retain, defaulting to 90. Put it in a systemd timer or a
crontab; it takes a second and the server keeps serving throughout, because
`VACUUM INTO` snapshots a live database rather than copying a moving file.

Copy the directory somewhere off the machine — object storage, another box, an
external disk. It is already encrypted, so where it goes matters much less than that
it goes.

### Restoring, and testing that you can

```bash
ops/restore.sh /var/backups/shopping-list/shopping-list-20260827T090000Z.db.age ~/restored.db
```

Run it **on the laptop with the private key**, not on the server. It decrypts, runs
`PRAGMA integrity_check` over every page, and prints how many people, lists and items
came back and when the newest was added. It never touches a live database: putting the
file back is stopping the server, moving it into place, and starting it again, and
those are decisions to take deliberately rather than by passing a flag.

**Do this on a quiet afternoon, before you need it.** A backup nobody has ever opened
is a belief. The count of lists is there so that a file which decrypts and passes an
integrity check can still be recognised as the wrong one.

If `restore.sh` says the database is damaged, that copy is gone — try an older one.
That is what `BACKUP_KEEP` is for.

## HTTPS

The clients refuse `http://` in release builds, so this is a prerequisite rather than
a hardening step. There are two ways to it and the server supports both; pick by what
you already run.

### Behind a reverse proxy — start here

If you already run Caddy, nginx or Traefik, this is the answer. Certificate renewal
lives in a component whose whole job that is, the application never touches port 80 or
holds a private key, and the failure modes are somebody else's well-documented ones.

Set nothing. `TLS_MODE` defaults to `off`, and the server logs a warning saying so
every time it starts — which is correct here and a leak anywhere else, so leave the
warning alone rather than silencing it.

```
# Caddyfile
list.example.com {
    reverse_proxy localhost:8080
}
```

Bind the application to localhost if the proxy is on the same machine, and let the
proxy be the only thing on a public port.

**One trap worth knowing**, from [docs/tls.md](../docs/tls.md) T6: if you put a
*terminating* proxy in front and also want this server to hold its own certificate,
ACME must use HTTP-01 rather than TLS-ALPN-01 — an L7 proxy answers the ALPN challenge
itself and the order fails saying nothing useful. Most people do not want that
arrangement; the ones who do lose an afternoon to it.

### In the process — for a bare box

One binary, no second daemon and no second configuration language.

```
TLS_MODE=acme
TLS_DOMAINS=list.example.com
ACME_CONTACT=you@example.com
PORT=443
```

The name must resolve to this machine and the CA must be able to reach it: it connects
to **port 443** for TLS-ALPN-01 or **80** for HTTP-01, and no setting anywhere changes
that. Serving on a high port is fine as long as your router forwards the public 443 to
it — what has to reach 443 is a packet, not a process.

While you are fighting with port forwarding, set `ACME_DIRECTORY=staging`. Let's
Encrypt limits *failed* validations to five per hostname per hour, and a misconfigured
server burns that in seconds and then looks broken for reasons unrelated to the
original mistake. **Take it out again afterwards** — a staging certificate produces a
server that starts cleanly, serves happily, and is rejected by every client.

To bind 443 without running the whole application as root: on Linux, either
`AmbientCapabilities=CAP_NET_BIND_SERVICE` in the unit file or
`net.ipv4.ip_unprivileged_port_start=443`. On macOS, ports below 1024 need root and
the practical answer for development is a high port.

Already have a certificate — a corporate CA, a wildcard, `mkcert` on your laptop?

```
TLS_MODE=files
TLS_CERT=/etc/shopping-list/fullchain.pem
TLS_KEY=/etc/shopping-list/privkey.pem
```

Use the **full chain**, not just the leaf. A server that sends only its own
certificate works in a browser that happens to have cached the intermediate and fails
on a phone that has not, which is the most annoying shape a TLS problem takes.

### When it will not work

A server that cannot get a certificate does not quietly serve cleartext — it binds,
logs the refusal, and fails handshakes. Ask the plain listener, which is the one thing
that is not redirected:

```bash
curl http://your-server/healthz
```

It answers `ok` and a line saying what TLS is doing, including the CA's own words when
an order failed. `tls: acme, list.example.com, no certificate — ...no A record for
list.example.com` is a sentence you can act on.

## Running it under systemd

[`shopping-list.service`](shopping-list.service) is the unit. Three directories, kept
apart on purpose:

| | |
|---|---|
| `/usr/local/bin/shopping-list-server` | the binary, root-owned and not writable by the service |
| `/etc/shopping-list/server.env` | the settings, root-owned `0600` |
| `/var/lib/shopping-list/` | the database, owned by the service user |

### Once

**The account first.** systemd will not create the user named in `User=`, and a
missing one fails the unit *before* the binary runs — so the journal holds systemd's
`Failed to determine user credentials: No such process` and not one word from the
application.

```bash
sudo cp ops/shopping-list.sysusers /usr/lib/sysusers.d/shopping-list.conf
sudo systemd-sysusers
```

or, the same thing by hand:

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin shopping-list
```

Then the binary and the settings:

```bash
sudo install -o root -g root -m 0755 shopping-list-server /usr/local/bin/
sudo install -d -o root -g root -m 0755 /etc/shopping-list
sudo install -o root -g root -m 0600 /dev/null /etc/shopping-list/server.env
sudoedit /etc/shopping-list/server.env
```

```bash
sudo cp ops/shopping-list.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now shopping-list
```

`/var/lib/shopping-list` is not created by hand — `StateDirectory=` in the unit makes
it on first start and gives it to the service user.

### The settings file is read by systemd, not by the application

`EnvironmentFile=` is loaded **as root, before privileges are dropped**, so the file
holding a Google client secret never has to be readable by the account the server runs
as. Leaving a `.env` in the working directory would mean the opposite: `dotenvy`
searches from there, so anything running as `shopping-list` could read the secret.

The syntax is close to a `.env` but not identical — no `export`, no mid-line `#`
comments, and quote any value containing spaces.

### Claiming it

The claim code is printed once at startup, while nobody owns the server:

```bash
sudo journalctl -u shopping-list -n 40
```

It is new on every restart and never stored, so reading it out of last month's journal
gets you nothing.

### Checking on it

```bash
systemctl status shopping-list
journalctl -u shopping-list -f
curl http://localhost:8080/healthz
```

`/healthz` answers `ok` and a line saying what TLS is doing, including the CA's own
words when an ACME order failed.

### If it will not start

**First, work out whether the binary ever ran.** If the journal holds systemd's own
failure line and nothing from the application, it failed before exec and the
application never had a chance to say anything.

```bash
systemctl status shopping-list -l --no-pager
systemctl show shopping-list -p Result -p ExecMainStatus -p ExecMainCode
```

`Result=` and the exit status name the cause, and systemd's codes are specific:

| | |
|---|---|
| `Result=exit-code`, status `217/USER` | the `shopping-list` account does not exist |
| `Result=exit-code`, status `203/EXEC` | the binary is missing, not executable, or built for another architecture |
| `Result=resources`, "Failed to load environment files" | `/etc/shopping-list/server.env` is not there |
| `Result=exit-code`, status `226/NAMESPACE` | a sandbox directive cannot be applied on this kernel |
| `Result=exit-code`, status `1` | the binary ran and refused — its own reason is in the journal |
| `Result=exit-code`, status `200/CHDIR` | `WorkingDirectory=` does not exist |

Only the last two mean the application was reached at all. A wrong architecture is
worth ruling out early — `file /usr/local/bin/shopping-list-server` should say
`x86-64`, and `ldd --version` should be 2.28 or newer.

To take systemd out of it entirely and see what the server itself says:

```bash
sudo -u shopping-list env $(sudo cat /etc/shopping-list/server.env | grep -v '^#' | xargs) \
    /usr/local/bin/shopping-list-server
```

And to check the unit file itself parses, which catches a directive in the wrong
section:

```bash
systemd-analyze verify /etc/systemd/system/shopping-list.service
```

Once the binary is running, the server refuses to start rather than run on
configuration it cannot make sense of —
a `TLS_MODE` it does not recognise, a `METRICS_PORT` that collides with the
application's, a scrape endpoint exposed to the network with no token, a database path
that is wrong. All of those are one line in the journal.

The unit gives up after five attempts in two minutes rather than restarting for ever,
because a restart loop buries that line. `systemctl reset-failed shopping-list` clears
the count once the file is fixed.

### Binding port 443

Only relevant if this process holds its own certificate; behind a reverse proxy it
does not. Two lines near the end of the unit are commented out and grant
`CAP_NET_BIND_SERVICE` — uncomment both, or use
`net.ipv4.ip_unprivileged_port_start=443`, rather than running the whole application
as root.

### Backups

`ops/backup.sh` reads the database directly and needs no cooperation from the running
server, so a timer can run it while the service is up:

```bash
sudo systemctl edit --force --full shopping-list-backup.timer
```

The sandboxing in the unit does not apply to it — it is a separate job, and it needs
`BACKUP_RECIPIENT` from the same place your key lives.

## Retention

What this server deletes on its own, and when:

| What | When | Where |
|---|---|---|
| Sessions idle past 90 days | Every 6 hours, and at boot | `housekeeping` in `server/src/main.rs` |
| A list's item history past 500 entries | On every write to it | `history::Entry::prune` |

Nothing else is deleted by anything except a person asking for it.

Backups are outside that, deliberately: `BACKUP_KEEP` is a separate decision made on a
separate machine, and a backup that vanished because the server tidied it up would not
be a backup.
