# Keys, and who holds them

A design, not an implementation. Nothing here is built yet.

[self-hosting.md](self-hosting.md)'s S8 makes a promise — *an attacker with root can
learn who shares a list with whom, and not what is on it* — and S9 says the mechanism
is a key per list held by its members. Everything that makes that promise true or
false is in this document. A key model that is merely plausible is worse than no
promise at all, because it is published.

```
  device key            account key            list key
  ──────────            ───────────            ────────
  one per phone,        one per person,        one per list per epoch,
  never leaves it  ──→  wrapped to each   ──→  wrapped to each member's
                        of their devices       account key
```

Three layers rather than two, for one reason: **adding a phone should be one
operation, not one per list.** With list keys wrapped straight to devices, buying a
phone means re-wrapping every list you are in, from a device that must be awake to do
it. With an account key in between it is a single wrap that covers everything, now and
in the future.

## What this has to be true of

Written down first, because every decision below is a trade against one of them:

* **It works offline.** A device holds the keys for everything it can see, and needs
  nothing from anybody to read or write. That is the whole premise of
  [offline.md](offline.md) and it forbids any design where reading requires asking.
* **The server is not trusted with content.** It is trusted to order, to store, and
  to enforce who may append — see K9.
* **This is a household, not an enterprise.** Four people who see each other daily.
  It is the reason K4 works and the reason K10 is mostly not a cryptography problem.
* **Ordinary people must be able to lose a phone.** A model that is correct and
  strands somebody is not correct.

## Where this stands today

Nothing. Everything is plaintext, and `Actor` is as close to an identity as the code
gets.

What is already right, and would otherwise have to be undone:

* **`items.uuid` and `lists.uuid` are minted on the device.** Identity does not wait
  for a server, which is what lets an operation be sealed before anybody has heard of
  it.
* **The change stream carries no content.** A nudge says "re-read", so encrypting the
  content changes nothing about how devices learn there is something to read.
* **`POST /api/sync` already takes operations rather than resources.** The unit this
  design encrypts already exists.
* **The Apple clients hold an opaque session token in the keychain.** The habit of
  keeping a secret on the device, and the code that does it, is written.

---

# Part one · The three keys

## K1 · Device key, account key, list key

**Device key.** One keypair per installation — phone, watch, Mac, browser profile.
The private half is generated on the device and never leaves it, by any route,
including backups where that can be arranged. A device is a thing that can be
revoked; that is its whole purpose.

**Account key.** One keypair per person. It is what a list key is wrapped to, so it
is what membership names. Its private half is wrapped to each of that person's
devices — see K5.

**List key.** Symmetric, one per list per epoch (K7). Wrapped to the account key of
every member. This is what actually encrypts operations.

The watch is a device with its own key, which is a change from today: it currently
has no identity at all and asks the phone for a token each time. Giving it one is
strictly better — it can read its lists with the phone in another room, which is what
`WatchIdentity` keeping a session token was already reaching for.

## K2 · Algorithms, and one format for wrapping

* **Sealing operations:** XChaCha20-Poly1305. The large nonce is the reason — with
  several devices writing offline under the same list key, random nonces have to be
  safe without coordination, and a 192-bit nonce makes them so. AES-GCM would need a
  counter nobody can share.
* **Wrapping to a public key:** X25519 with HPKE (RFC 9180), or **`age`**, which is
  the same primitives in an audited, boring container with recipients, identities and
  a passphrase mode that K11 wants for free. [self-hosting.md](self-hosting.md)'s S12
  already reaches for `age` for backups; one format for both is one fewer thing to get
  right.
* **Recovery phrase:** a BIP-39 wordlist to a seed to an X25519 key.

**Associated data is not optional.** Every sealed operation binds `list_id`, `epoch`,
the sequence number the server assigned, and the device that wrote it. Without it the
server can move a ciphertext from one list to another, or replay yesterday's into
today, and neither needs a key. With it, both fail to open.

Note what follows from that: **the server cannot forge content.** It has no list key,
so it cannot produce a ciphertext anybody will open. Its remaining powers are to
withhold, to reorder within what it is given, and to lie about keys — see K6. That is
why nothing here signs operations: signatures would defend against an attacker who
already cannot do the thing.

## K3 · Where the private halves live

| Platform | Store | Notes |
|---|---|---|
| iOS, macOS, watchOS | Keychain, `AfterFirstUnlock` | The accessibility `Keychain.swift` already uses, for the same reason: a background refresh on a locked phone must still work |
| Android | Keystore | StrongBox where present |
| Browser | IndexedDB, non-extractable `CryptoKey` | Script cannot exfiltrate it; clearing site data destroys it, so a browser is always a secondary device |

**Hardware-backed keys are declined, deliberately.** Secure Enclave and StrongBox do
P-256 and not X25519, so using them means either two curve families or P-256
everywhere. What they buy is protection against extraction from a rooted or
jailbroken device, which is not in S8's threat model — the OS keychain already covers
a stolen locked phone. Software X25519 everywhere is one curve, one code path, and
one thing to reason about. Revisit if the threat model ever grows.

---

# Part two · Getting keys to people

## K4 · An invite carries the list key, in the fragment

A share link becomes `https://server/invite/TOKEN#k=BASE64KEY`. Everything after the
`#` is never sent to a server by any browser, which means **the server never sees the
key it is helping to distribute.** The recipient's client reads it, wraps it to their
own account key, and uploads the wrap.

This is the decision that makes the rest cheap. The obvious alternative — ask the
server for the recipient's public key and wrap to it — needs a key directory the
server could lie about, and needs the sender online at the moment the recipient
accepts. This needs neither.

It also matches the semantics already written down in `models/invite.rs`: *an
invitation is a bearer credential; whoever holds the link gets the role in it.* The
key in the fragment does not make the link more dangerous than it already was. It
makes it exactly as dangerous, and honestly so.

Two consequences to state in the UI rather than bury:

* A link pasted into a group chat shares the list with the group chat.
* Once sent, a link cannot be unsent. Revocation is K7, and it is forward-looking.

## K5 · Adding a device is one wrap

A new device generates its key and shows a code. An existing device of the same
person scans or types it, wraps the **account** private key to the new device's public
key, and uploads it. The new device now opens every list that person is in, including
lists it has never heard of, including lists added later.

The pairing is deliberately device-to-device and human-mediated. The server carries
the wrapped blob and cannot open it; the only thing it could do is withhold, which is
visible.

**Platform keychain sync is the default path, not the exception.** iCloud Keychain and
Android's Block Store already move small secrets between a person's devices, and a new
phone restored from a backup should simply have the account key. Pairing exists for
the cases that do not — a Mac alongside an Android phone, a browser, a first watch.
Designing pairing as the normal route and sync as an optimisation would make the
common case the awkward one.

## K6 · The directory, and the one place the server can lie

K4 removes the need for a key directory when sharing. It does not remove it entirely:
**rotation needs one.** When a member is removed, the new epoch's key must be wrapped
to everyone remaining, and their account public keys come from the server.

So the honest statement of the residual weakness is narrow: *a malicious server can
substitute a public key at rotation time and read subsequent epochs.* It cannot read
anything shared through K4, and it cannot read anything already written.

The cheap defence is trust on first use — a client pins each member's account public
key the first time it wraps to them, and refuses, loudly, if it ever changes without
that member having paired a new account key. Fingerprint comparison in person is the
thorough version and is almost certainly more than a household wants; the pin costs a
column and closes most of it.

**This belongs in S8's published threat model**, phrased plainly, rather than being
true and unmentioned.

---

# Part three · Taking access away

## K7 · Epochs: rotation moves forward and does not rewrite the past

Removing somebody from a list mints a new list key, wraps it to the remaining members,
and increments the epoch. New operations are sealed under it. Old operations stay
under the old key, which every remaining member still holds and which the removed
person also still holds.

**Re-encrypting the history is not worth doing.** It is expensive, it must be done by
a member rather than the server, and it buys nothing: they have already read it. The
schema keeps every epoch key a member was ever given, because that is what reading
last month requires.

This is a real departure from the instinct recorded in [offline.md](offline.md) that
*delete is final, a fact on the server rather than an intent on a device.* Removal is
now genuinely forward-looking, and the UI has to say so — "they will not see anything
from now on" and not "they have been removed", because the second is a promise about
the past that nothing can keep.

**One leak worth knowing.** A member who was offline during a rotation keeps writing
under the old epoch, and those writes are readable by the person who was removed. It
resolves as soon as that device syncs and learns about the new epoch. Bounded, and
unavoidable without an authority everyone can reach — which is precisely what being
offline means.

## K8 · A lost device

Two levels, and the app should offer both because they cost very different things.

**Stop it being used.** Delete the device's wrap of the account key and the server
stops serving it. Cheap, immediate, and dishonest on its own: the device still holds
whatever it cached.

**Assume it is read.** Rotate the account key, re-wrap it to the remaining devices,
and rotate every list that person is in. Bounded work for a household — a handful of
lists — and it is the only version that means anything if the phone is genuinely gone.
It is a panic button, and it should look like one.

Presenting only the first would be the more comfortable design and the wrong one.

## K9 · What a member can do that the server cannot

Encryption is confidentiality. It is not authorisation, and the server keeps doing the
job it does today: deciding who may append to a list's log. Without that, anybody who
learned a list id could flood it with blobs nobody can read but everybody must store.

Inside a list, the model is deliberately flat. Members share a key, so any member can
write anything and attribute it to anyone — the history saying *Rimantas added Milk*
is a claim by whoever held the key, not a proof. Per-device signing would fix it and
is not worth its weight for four people who live together. It does mean the
viewer role cannot survive as a cryptographic guarantee, only as a convention the
server enforces. Say so rather than implying otherwise.

---

# Part four · Losing everything

## K10 · Most recovery is somebody re-adding you

The framing that makes this tractable: **a shared list does not need cryptographic
recovery.** Lose every device, generate a fresh account key, and a member re-wraps the
list key to it. That is the ordinary share flow with a different reason for running,
and in a household — where the app is used by people who can hand each other a phone
— it covers nearly every real case.

What is genuinely unrecoverable is a list nobody else is in. For a shopping list that
is the weekly shop of somebody who lives alone, which is a real person, but it means
the hard mechanism below is a fallback rather than the main path.

Recovery therefore decomposes into three, and only the last needs cryptography:

1. **A new phone**, restored from backup — the platform keychain already moved the
   account key. Nothing happens.
2. **A new phone with nothing on it, but another device or another member** — pairing
   (K5) or a re-share (above).
3. **Everything gone, and lists nobody else has** — K11.

## K11 · The recovery phrase, for the lists nobody else is in

A twelve-word phrase, derived to a key that wraps a copy of the account key, stored on
the server. Offered once, skippable, and re-offerable later from settings.

The alternatives and why not:

* **A recovery password**, Argon2id-derived. Equivalent in mechanism, worse in
  practice: people reuse passwords, and a server-held wrap under a reused password is
  the one construction that quietly undoes S8. It also reintroduces the thing Sign in
  with Apple just removed.
* **Escrow to another member.** Elegant for a household and a bad idea on inspection:
  it hands somebody your account key, which is every list you are in, including the
  ones they are not.
* **Nothing.** Defensible, and the answer if K11 turns out to be more UI than it is
  worth. Say it plainly at first run rather than discovering it later.

**Skipping must be allowed and must be honest.** "If you lose every device, lists that
only you can see are gone, permanently" is a sentence a person can act on. A dialogue
that will not let them past is a dialogue they defeat by writing the phrase in the
notes app on the phone they are about to lose.

---

## What is encrypted, and what the shape of it costs

The unit is the **operation**, not the row. `Add`, `SetDone`, `Update`, `Delete` and
`ClearDone` are serialised, sealed under the list key with the associated data in K2,
and appended. The server holds `(list_id, seq, epoch, device, created_at, ciphertext)`
and can read none of the last one.

Which means item rows stop being a thing the server has. Items become a client-side
projection of the log, and with them go:

* **`order_by=done_at` and paging.** Clients hold the whole list — they already fetch
  `size=500`, which is to say everything — and sort locally.
* **Server-side grouping by tag**, and the quick-add line parser, which move into
  `domain` compiled to the client rather than called over HTTP.
* **The server-rendered web UI**, as S9 already records.

Reference data is untouched. Units and tags are global, shared and not secret; what is
secret is which of them an item names, and that is inside the ciphertext.

`done_at` goes inside too, despite being the thing the list is ordered by, because
leaving it out publishes when each person shops and what they crossed off when. The
merge rule that reads it — last write wins on a clamped device clock — moves to the
client along with everything else in `service::sync`, which keeps working because the
server still assigns the sequence numbers that give it a total order.

## In what order

1. **K3 and K1**, keys generated and stored, used for nothing. Nothing observable
   changes and every platform's storage gets exercised.
2. **K5**, pairing and platform sync, still with nothing encrypted. It is the flow
   most likely to be wrong and the cheapest to fix while it protects nothing.
3. **Sealing operations** under a list key held by one person. Single-member lists
   only; no sharing, no rotation.
4. **K4**, sharing by invite.
5. **K7**, epochs and rotation — after sharing, because rotation is meaningless
   before it, and because the schema for holding several epochs is easier to get right
   once there is a reason for it.
6. **K10 and K11**, recovery.
7. **K8**'s panic button.

Steps 1 and 2 are shippable on their own and change nothing a person sees, which makes
them a good way to find out whether the rest is affordable.

## Open

**Whether the browser is a device at all.** K3 makes it always-secondary because
clearing site data destroys the key. A person who uses the web UI on a shared machine,
loses it every time, and re-pairs on each visit will hate it. The alternative — a
passphrase-derived key in the browser — reintroduces exactly the construction K11
rejects. It may be that the honest answer is that the web UI reads nothing and the
browser is not a client any more.

**Whether rotation is manual or automatic.** K7 rotates on removal. Rotating on a
schedule as well would bound the damage from a key that leaked without anybody
noticing, at the cost of work nobody asked for and epochs accumulating for ever.

**How many epochs to keep.** Keeping all of them means a member who joined in year
three carries keys for years one and two if they are to read the history. Discarding
old epochs is a retention policy expressed as a key deletion, which is neat, and it is
also [self-hosting.md](self-hosting.md)'s S13 crypto-shredding pointed at yourself.

**The pin in K6.** Trust on first use is the cheap answer; whether a household will
ever understand the warning it produces is a different question, and a warning nobody
understands is a warning that gets dismissed.
