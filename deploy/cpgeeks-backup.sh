#!/usr/bin/env bash
# Nightly database dump. Neon's free plan keeps a six-hour restore window,
# which covers a mistake noticed immediately and nothing else — a bad migration
# found the next morning is already outside it.
#
# The local copy is the fast one. The S3 copy is the one that survives losing
# this instance: a dump sitting on the same volume as the database it protects
# is gone in exactly the scenarios you most need it.
set -euo pipefail

ENV_FILE=/etc/cpgeeks/backend.env
DIR=/var/backups/cpgeeks
KEEP=14

# Fail on the file before reading keys out of it. Under `set -e` a sed exit
# status from an unreadable file propagates out of the command substitution and
# kills the script before any friendly error runs — silently, which is the worst
# way for a backup to stop happening.
if [ ! -r "$ENV_FILE" ]; then
    echo "cannot read $ENV_FILE — is this running as root?" >&2
    exit 1
fi

# Read only the values we need rather than sourcing the whole file. Sourcing
# pulls every secret into the shell environment for no reason, and makes the
# script hostage to how bash parses values written for systemd's parser.
# 2>/dev/null so a missing env file produces one clear message below rather
# than a burst of raw sed errors in front of it
read_env() { sed -n "s/^$1=//p" "$ENV_FILE" 2>/dev/null | head -1 | sed 's/^"//; s/"$//'; }

DB_URL=$(read_env DATABASE_URL)
# optional. unset means local-only backups, which is the old behaviour.
S3_BUCKET=$(read_env BACKUP_S3_BUCKET)

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
BYTES=$(stat -c %s "$FILE")
TABLES=$(pg_restore --list "$FILE" | grep -c 'TABLE DATA')
echo "backup ok: $FILE ($BYTES bytes, $TABLES tables)"

# rotate locally, newest first
ls -1t "$DIR"/cpgeeks-*.dump 2>/dev/null | tail -n +$((KEEP + 1)) | xargs -r rm --

# --- offsite copy -----------------------------------------------------------
# Everything above has already succeeded, so a failure from here leaves a good
# local backup behind. It still exits non-zero, because a silently missing
# offsite copy is the whole problem this section exists to prevent — the unit's
# OnFailure= handler turns that into a mail rather than a line nobody reads.
if [ -z "$S3_BUCKET" ]; then
    echo "BACKUP_S3_BUCKET not set — local copy only, this instance is a single point of failure"
    exit 0
fi

if ! command -v aws > /dev/null 2>&1; then
    echo "BACKUP_S3_BUCKET is set but the aws cli is not installed" >&2
    exit 1
fi

KEY="db/$(basename "$FILE")"
# no credentials on disk: the instance role supplies them, so there is no key
# here to leak and nothing to rotate when someone screenshots this file
aws s3 cp "$FILE" "s3://$S3_BUCKET/$KEY" --only-show-errors

# trust nothing that has not been read back. a silent partial upload is exactly
# the failure this whole section exists to prevent.
REMOTE_BYTES=$(aws s3api head-object --bucket "$S3_BUCKET" --key "$KEY" \
    --query ContentLength --output text 2>/dev/null || echo "missing")

if [ "$REMOTE_BYTES" != "$BYTES" ]; then
    echo "offsite copy failed verification: local $BYTES bytes, remote $REMOTE_BYTES" >&2
    exit 1
fi

echo "offsite ok: s3://$S3_BUCKET/$KEY ($REMOTE_BYTES bytes)"
