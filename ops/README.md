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
