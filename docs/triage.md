# Triage

What to check when something is broken, and how to tell the failure modes apart.
Every entry here is something that has actually happened to this project, not a
list of things that theoretically could.

Work top to bottom. Most reports are answered before you reach the specific
sections.

---

## Step 0 — is the backend actually up?

```bash
curl -s https://api.sustcpgeeks.me/api/health
```

```json
{ "status": "ok", "database": "connected", "storage": "ok" }
```

| Field | Values | What it means |
|---|---|---|
| `status` | `ok` (HTTP 200) · `error` (HTTP 503) | `error` means the database is unreachable. Nothing else will work. |
| `database` | `connected` | Anything else and the API is effectively down. |
| `storage` | `ok` · `not_configured` · `unavailable` · `timeout` | Supabase, used only for ID card photos. **Never changes the status code** — the site works fine without it, registration with a photo does not. |

If this returns JSON at all, the server is running and TLS is fine. That already
rules out most of the scary-sounding reports.

If it does not respond at all, skip to [The service is down](#the-service-is-down).

---

## "Could not connect to the server" in the browser, but the API is up

**This is almost always CORS, not connectivity.** The browser refuses to show
the real response, so the frontend reports it the same way it reports a dead
server. The two are indistinguishable from the user's side and need completely
different fixes.

Test it by asking the API how it feels about the origin:

```bash
curl -s -o /dev/null -D - -X OPTIONS https://api.sustcpgeeks.me/api/health \
  -H "Origin: https://sustcpgeeks.me" \
  -H "Access-Control-Request-Method: GET" | grep -i access-control-allow-origin
```

A missing `access-control-allow-origin` header means that origin is not allowed.
Fix it on the server, not in code — the list is an environment variable so a new
frontend URL never needs a deploy:

```bash
sudo nano /etc/cpgeeks/backend.env      # CORS_ALLOWED_ORIGINS, comma separated
sudo systemctl restart cpgeeks-backend
```

**Trap:** adding a new hostname to DNS is only half the job. The moment
`www.sustcpgeeks.me` resolves, browsers start sending
`Origin: https://www.sustcpgeeks.me`, which is a *different* origin from the
apex domain. If it is not in the list, the site loads and every single API call
fails with this exact symptom.

---

## Emails are not arriving

OTP, password reset and email change all go through Resend.

- **Nothing arrives for anyone, and logs show a 403.** The sender address is not
  on a verified domain. `onboarding@resend.dev` only ever delivers to the
  address that owns the Resend account — it looks like it works when you test it
  yourself, and fails for every real member.
- **Only some arrive.** Check the daily send limit on the Resend plan. Onboarding
  a few hundred members at 100/day takes days, and the failures look identical
  to a broken integration.

`RESEND_FROM_EMAIL` in `/etc/cpgeeks/backend.env` must be on the verified
domain.

---

## ID card upload fails / "Failed to store file"

Storage is Supabase, and it is the one dependency that can vanish quietly.

- `health` says `storage: unavailable` or `timeout` → check the Supabase
  dashboard.
- Logs say **"could not resolve host … supabase.co"** → the project is *paused*,
  not deleted. A paused project drops its DNS record, which makes it look like
  the URL is wrong or the project is gone. Resume it from the dashboard and the
  hostname comes back.

Registration without a photo keeps working throughout, which is why this can go
unnoticed for a while.

---

## Codeforces or AtCoder data looks wrong or missing

Both are third-party sites that go down, and neither is on the request path for
ratings any more — a background sync writes them to our tables every 6 hours.

- **A profile shows a banner saying the data is stale.** Working as designed.
  Codeforces is unreachable, so the last synced rating is shown rather than
  failing the page. Recent activity and solve counts are hidden because we do
  not store them. It clears itself when Codeforces returns.
- **The banner mentions a specific handle error.** That is not an outage — the
  handle is wrong or was renamed. The member should fix it on their profile.
- **The leaderboard is up to 6 hours behind.** Expected. It reads stored ratings
  so that a Codeforces outage cannot empty it.
- **The leaderboard is empty.** Not expected. That is a database problem, not an
  upstream one.

---

## PDF export downloads nothing, or opens a page

The ranker renders PDFs with bundled fonts resolved relative to the service's
working directory (`/opt/cpgeeks`). If `fonts/` did not ship alongside the
binary, generation fails with `Failed to open font file ./fonts/…`.

```bash
ls /opt/cpgeeks/fonts/
```

Empty or missing means the deploy was incomplete — re-run `deploy/deploy.sh`.

---

## The service is down

```bash
sudo systemctl status cpgeeks-backend
sudo journalctl -u cpgeeks-backend -n 100 --no-pager      # recent logs
sudo journalctl -u cpgeeks-backend -f                     # follow live
sudo systemctl restart cpgeeks-backend
```

Things that have actually put it here:

- **`3F000: no schema has been selected to create in`** — a pooled Neon
  connection with an empty `search_path`. Fixed at the database level with
  `ALTER DATABASE neondb SET search_path TO "$user", public;`. Note that Neon's
  pooler rejects setting `search_path` as a startup option, so that is not an
  alternative.
- **`AddrInUse`** — something else is already on the port. Usually a stray local
  run, not a server problem.
- **Database unreachable** — check the Neon dashboard before anything else.

Caddy sits in front and handles TLS automatically. If certificates are the
suspect:

```bash
sudo systemctl status caddy
sudo journalctl -u caddy -n 50 --no-pager
```

---

## Uploads fail on large files

Registration accepts an ID card photo up to 5 MB. A larger one produces
`Error parsing multipart/form-data`, which reads like a malformed request rather
than a size limit — the request is rejected before the handler ever sees it.

The size lives in one place, `MAX_UPLOAD_BYTES` in
`src/services/image_upload.rs`. The route's body limit (`REGISTER_BODY_LIMIT` in
`src/routes/auth_routes.rs`) is derived from it — two photos plus a megabyte of
form fields — so changing the constant is enough and the two cannot drift apart.

---

## Lost data, or need to restore

Restoring is not a triage step — it is a deliberate operation with its own
failure modes, and doing it under pressure from memory is how a recoverable
incident becomes an unrecoverable one. See [`backups.md`](backups.md), which
covers what is and is not backed up, and how to restore into a scratch branch
rather than over the live database.

---

## Still stuck? Open an issue

Include, at minimum:

1. The output of the `health` command above.
2. What you did, what you expected, what happened.
3. Whether it affects everyone or just you — log out and try in a private
   window, since a stale token looks like a permissions bug.
4. For anything server-side, the last 50 lines of
   `journalctl -u cpgeeks-backend`.

Say what you already ruled out. "Health is ok and it fails in a private window
too" saves an entire round trip.
