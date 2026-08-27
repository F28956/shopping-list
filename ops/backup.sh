#!/usr/bin/env bash
#
# A backup this machine cannot read.
#
# The machine holds a *public* key and nothing else, so the process that writes the
# backup has no way to open it — which is the whole point. Somebody who takes the
# server takes a directory of ciphertext, and the private key is on a laptop, a
# password manager or a piece of paper somewhere else entirely.
#
# Two steps and no cleverness:
#
#   1. `VACUUM INTO` — a consistent snapshot of a live database. Not `cp`, which
#      copies a moving file, and not `sqlite3 .dump`, which is slower and larger for
#      no gain here. The server keeps serving throughout.
#   2. `age -r` — encrypt to the recipient, then delete the plaintext snapshot.
#
# Restoring is in docs/self-hosting.md. It needs the private key, and if you cannot
# find the private key you do not have a backup — which is worth discovering on a
# quiet afternoon rather than after a disk fails. Test it.

set -euo pipefail

DB="${SHOPPING_LIST_DB:?set SHOPPING_LIST_DB to the database file}"
DEST="${BACKUP_DIR:?set BACKUP_DIR to where backups are written}"
RECIPIENT="${BACKUP_RECIPIENT:?set BACKUP_RECIPIENT to an age public key (age1...)}"
# How many to keep. Ninety days of dailies is small -- the database is measured in
# megabytes -- and is long enough that a corruption noticed late is still recoverable.
KEEP="${BACKUP_KEEP:-90}"

for tool in sqlite3 age; do
    command -v "$tool" >/dev/null || { echo "$tool is not installed" >&2; exit 1; }
done

case "$RECIPIENT" in
    age1*) ;;
    # Caught here rather than by age, because the failure otherwise arrives as an
    # unreadable backup discovered months later. A private key in this variable would
    # also "work", and would put the key on the machine the backup is defending
    # against.
    *) echo "BACKUP_RECIPIENT must be an age public key (age1...), not a path or a private key" >&2; exit 1 ;;
esac

mkdir -p "$DEST"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"

# A private directory rather than `mktemp` a file: `VACUUM INTO` refuses to write to a
# path that already exists, and `mktemp` exists to create the file. `mktemp -u` would
# hand back a name and a race.
workspace="$(mktemp -d "${TMPDIR:-/tmp}/shopping-list-backup.XXXXXX")"
snapshot="$workspace/snapshot.db"

# The snapshot is plaintext for as long as it takes to encrypt it, so the directory
# goes on every exit path, including a failure partway through.
trap 'rm -rf "$workspace"' EXIT

sqlite3 "$DB" "VACUUM INTO '$snapshot'"
age -r "$RECIPIENT" -o "$DEST/shopping-list-$stamp.db.age" "$snapshot"

# Only ever deletes files this script's own naming produced, so pointing BACKUP_DIR
# at a directory holding something else cannot lose it.
find "$DEST" -maxdepth 1 -name 'shopping-list-*.db.age' -type f \
    | sort -r | tail -n "+$((KEEP + 1))" | while read -r old; do rm -f "$old"; done

echo "wrote $DEST/shopping-list-$stamp.db.age"
