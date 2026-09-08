# Backups and restore

The nightly dump runs at 02:00 server time (`Persistent=true`, so a missed run
fires when the machine comes back), keeps 14 local
copies, and — once the offsite step below is configured — pushes each one to S3.

Neon's free plan keeps a six-hour restore window. That covers a mistake noticed
immediately and nothing else: a bad migration found the next morning is already
outside it. These dumps are what covers everything past six hours.

---

## What is and is not covered

**Covered** — everything in the Postgres database: members, announcements,
events, contests, teams, problemset, synced ratings and rating history.

**Not covered, and worth knowing before you need it:**

| | Why | What to do if it is lost |
|---|---|---|
| ID card photographs | Live in Supabase Storage, not Postgres | Nothing — they are deleted once an admin reviews them, so the store is meant to be near-empty. Anything still there is unreviewed and can be re-requested from the member. |
| `/etc/cpgeeks/backend.env` | Never backed up on purpose; a copy of every production secret sitting in a dump is a bigger risk than re-creating it | Re-create from the provider dashboards. Faster than it sounds, and forces a rotation anyway. |
| The binary, fonts, Caddy config | All rebuildable from git | `deploy/deploy.sh` |

---

## One-time setup: the offsite copy

Without this the dumps sit on the same volume as the database they protect, and
losing the instance loses both at once.

**1. Create a private bucket** (Singapore, to match the instance):

```bash
aws s3api create-bucket --bucket cpgeeks-backups \
  --region ap-southeast-1 \
  --create-bucket-configuration LocationConstraint=ap-southeast-1

aws s3api put-public-access-block --bucket cpgeeks-backups \
  --public-access-block-configuration \
  "BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true"
```

These dumps contain every member's name and email. The bucket must never be
public.

**2. Expire old copies** so storage does not grow forever — 90 days is a
reasonable window given 14 local copies cover the recent past:

```bash
aws s3api put-bucket-lifecycle-configuration --bucket cpgeeks-backups \
  --lifecycle-configuration '{"Rules":[{"ID":"expire","Status":"Enabled","Filter":{"Prefix":"db/"},"Expiration":{"Days":90}}]}'
```

**3. Give the instance permission with a role, not a key.** In the AWS console:
IAM → Roles → create a role for EC2 with a policy allowing `s3:PutObject` and
`s3:GetObject` on `arn:aws:s3:::cpgeeks-backups/*`, then attach it to the
instance. The AWS CLI picks it up automatically.

Use a role rather than putting an access key in the env file. There is then no
credential on the box to leak, and nothing to rotate when a file gets
screenshotted.

**4. Point the script at it** — add to `/etc/cpgeeks/backend.env`:

```
BACKUP_S3_BUCKET=cpgeeks-backups
```

**5. Run it once by hand and read the output:**

```bash
sudo systemctl start cpgeeks-backup.service
sudo journalctl -u cpgeeks-backup -n 20 --no-pager
```

Expect both `backup ok:` and `offsite ok:` lines. The script compares the byte
count it uploaded against what S3 reports back, so `offsite ok` means the copy
was read back and matched — not merely that the upload command exited zero.

If `BACKUP_S3_BUCKET` is unset the script still works and says so plainly, which
is the old local-only behaviour.

---

## One-time setup: failure alerts

`cpgeeks-backup.service` has `OnFailure=cpgeeks-alert@%n.service`, which mails
through the Resend account the application already uses.

Add a destination to `/etc/cpgeeks/backend.env`:

```
ALERT_EMAIL=someone@example.com
```

`deploy/deploy.sh` ships the handler script itself. The unit file is a one-time
install, and the repository is not checked out on the server — so copy it up
from your laptop:

```bash
# on your laptop, from the repo
scp -i ~/.ssh/cpgeeks-backend-key.pem \
    deploy/cpgeeks-alert@.service ubuntu@api.sustcpgeeks.me:/tmp/

# then on the server
sudo install -o root -g root -m 0644 \
    /tmp/cpgeeks-alert@.service /etc/systemd/system/
sudo systemctl daemon-reload
```

The updated `cpgeeks-backup.service` and `cpgeeks-backend.service` need the same
treatment, since they now carry the `OnFailure=` line.

Test it without breaking anything:

```bash
sudo systemd-run --unit=alert-test --property=OnFailure=cpgeeks-alert@alert-test.service /bin/false
```

That runs a unit which fails immediately and should produce a mail.

**What this does not cover:** the backend almost never enters a `failed` state,
because it retries forever by design — see the comment in
`cpgeeks-backend.service`. Knowing the API is down needs an external uptime
check hitting `/api/health` on a schedule. Nothing here does that.

---

## The restore drill

A dump that has never been restored is a belief, not a backup. Do this once now,
and again after any change to the schema or the backup script.

Neon branches make it safe: the drill never touches production.

**1. Create a scratch branch** in the Neon console (Branches → New branch, from
`production`). Copy its connection string.

**2. Restore the newest dump into it:**

```bash
SCRATCH="postgresql://...neon.tech/neondb"     # the scratch branch, NOT production
LATEST=$(ls -1t /var/backups/cpgeeks/*.dump | head -1)

pg_restore --clean --if-exists --no-owner --dbname="$SCRATCH" "$LATEST"
```

Check the connection string twice before pressing enter. `--clean` drops objects
before recreating them, so pointing this at production would be exactly as bad
as it sounds.

**3. Verify the data is actually there** — not just that the command exited zero:

```bash
psql "$SCRATCH" -c "SELECT
  (SELECT count(*) FROM users)         AS users,
  (SELECT count(*) FROM announcements) AS announcements,
  (SELECT count(*) FROM events)        AS events,
  (SELECT count(*) FROM platform_profiles) AS profiles;"
```

Compare against production. Small differences are expected — the dump is from
last night — but the order of magnitude must match. Zero rows anywhere is a
failed drill.

**4. Delete the scratch branch** when done. Neon's free plan limits branches,
and a forgotten scratch branch holding member data is a liability.

Write down the date you last did this. If nobody can remember, it is due.

---

## Restoring for real

Same as the drill, with two differences: restore into a **new branch**, verify
it, and only then promote that branch to primary in the Neon console. Do not
`pg_restore --clean` over a live production database — if the restore fails
partway you have neither the old data nor the new.
