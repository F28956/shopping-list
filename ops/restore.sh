#!/usr/bin/env bash
#
# Open a backup, check it, and leave it somewhere for you to look at.
#
# It deliberately does not put the database back. Restoring is stopping the server,
# moving a file and starting it again, and those are three decisions somebody should
# take deliberately at the moment they are taking them -- not a flag on a script run
# at four in the morning. This gets you as far as "here is a database, and it is
# sound", which is the part that needs a key and a tool.
#
# Run it on a laptop, not on the server: the private key is the thing the whole
# arrangement exists to keep off that machine, and this is the only step that needs
# it. Run it on a quiet afternoon, too. A backup nobody has ever opened is a belief,
# not a backup.
#
#   ops/restore.sh backups/shopping-list-20260827T090000Z.db.age ~/restored.db

set -euo pipefail

ARCHIVE="${1:?usage: restore.sh <backup.db.age> <destination.db>}"
DEST="${2:?usage: restore.sh <backup.db.age> <destination.db>}"
IDENTITY="${BACKUP_IDENTITY:-$HOME/.config/shopping-list/backup-key.txt}"

for tool in sqlite3 age; do
    command -v "$tool" >/dev/null || { echo "$tool is not installed" >&2; exit 1; }
done

[ -f "$ARCHIVE" ] || { echo "no such backup: $ARCHIVE" >&2; exit 1; }
[ -f "$IDENTITY" ] || {
    echo "no private key at $IDENTITY -- set BACKUP_IDENTITY to where yours is" >&2
    exit 1
}
# Refused rather than overwritten. The likeliest second argument to get wrong is the
# path to a live database.
[ -e "$DEST" ] && { echo "$DEST already exists; move it or choose another path" >&2; exit 1; }

age -d -i "$IDENTITY" -o "$DEST" "$ARCHIVE"

# `integrity_check` reads every page, so it catches the bit that rotted in object
# storage as well as the download that stopped early. Slower than `quick_check` and
# the reason to run this at all.
# `|| true`, because a file damaged badly enough makes sqlite3 exit non-zero, and
# `set -e` would then kill this script before it could say what was wrong. An empty
# answer is the shape that failure takes, and it is a failure.
result="$(sqlite3 "$DEST" 'PRAGMA integrity_check;' 2>&1 || true)"
if [ "$result" != "ok" ]; then
    echo "the restored database is damaged:" >&2
    echo "${result:-sqlite could not read it at all}" >&2
    exit 1
fi

echo "opened and checked: $DEST"
echo
echo "  people:  $(sqlite3 "$DEST" 'SELECT count(*) FROM users;')"
echo "  lists:   $(sqlite3 "$DEST" 'SELECT count(*) FROM lists;')"
echo "  items:   $(sqlite3 "$DEST" 'SELECT count(*) FROM items;')"
echo "  newest:  $(sqlite3 "$DEST" "SELECT coalesce(max(datetime(created_at, 'unixepoch')), 'nothing') FROM items;")"
echo
echo "To put it back: stop the server, move this over the database, start it again."
