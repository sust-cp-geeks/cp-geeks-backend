#!/usr/bin/env bash
# Nightly database dump. Neon's free plan keeps a six-hour restore window,
# which covers a mistake noticed immediately and nothing else — a bad migration
# found the next morning is already outside it.
set -euo pipefail

ENV_FILE=/etc/cpgeeks/backend.env
DIR=/var/backups/cpgeeks
KEEP=14

# Read the one value we need rather than sourcing the whole file. Sourcing
# pulls every secret into the shell environment for no reason, and makes the
# script hostage to how bash parses values written for systemd's parser.
DB_URL=$(sed -n 's/^DATABASE_URL=//p' "$ENV_FILE" | head -1 | sed 's/^"//; s/"$//')

if [ -z "$DB_URL" ]; then
    echo "no DATABASE_URL found in $ENV_FILE" >&2
    exit 1
fi

# pg_dump needs a real session; neon's pooler is pgbouncer in transaction mode
# and breaks it, so dump against the direct endpoint
DIRECT_URL="${DB_URL/-pooler/}"

mkdir -p "$DIR"
STAMP=$(date -u +%Y-%m-%dT%H%M%SZ)
FILE="$DIR/cpgeeks-$STAMP.dump"

pg_dump "$DIRECT_URL" --format=custom --file="$FILE"

# a dump that cannot be listed is not a backup. fail loudly rather than
# accumulate corrupt files nobody checks until the day they are needed.
if ! pg_restore --list "$FILE" > /dev/null 2>&1; then
    echo "dump failed verification, removing: $FILE" >&2
    rm -f "$FILE"
    exit 1
fi

chmod 600 "$FILE"
echo "backup ok: $FILE ($(stat -c %s "$FILE") bytes, $(pg_restore --list "$FILE" | grep -c 'TABLE DATA') tables)"

# rotate, newest first
ls -1t "$DIR"/cpgeeks-*.dump 2>/dev/null | tail -n +$((KEEP + 1)) | xargs -r rm --
