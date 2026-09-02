# API Reference

> Base URL: `http://localhost:8080`

---

## Common Headers

| Header | Value | When |
|--------|-------|------|
| `Content-Type` | `application/json` | All POST/PUT requests |
| `Authorization` | `Bearer <token>` | All protected routes |

## Error Format

Every error follows this shape:

```json
{
  "success": false,
  "error": "Human-readable error message"
}
```

| HTTP Code | Meaning |
|-----------|---------|
| `400` | Bad request / validation error |
| `401` | Missing or invalid token |
| `403` | Insufficient permissions (not admin/manager) |
| `404` | Resource not found |
| `409` | Conflict (e.g. duplicate email) |
| `429` | Too many attempts — rate limited |
| `500` | Internal server error |

## Rate Limits

Auth endpoints are rate limited. The `429` body states how long to wait.

| Action | Limit | Counted per | Window |
|--------|-------|-------------|--------|
| Wrong OTP code | 5 | email | 15 min — also invalidates the live code |
| Failed login | 10 | email | 15 min — reset by a successful login |
| OTP emails (register + resend + forgot, combined) | 5 | email | 1 hour |
| `POST /api/ranker/analyze` | 10 | IP address | 5 min |

Counters are held in memory, so restarting the server clears them.

---

## Authentication Flow

```
Register ──> OTP Email ──> Verify OTP ──> Login ──> JWT Token
                                            │
                              SUST email? ──┤──> status: active (can login)
                              Other email? ─┘──> status: pending (admin approval needed)
```

Two ways to register:

| Door | How | Outcome |
|------|-----|---------|
| **A — has university email** | JSON body, no ID card | OTP verified → `active` straight away |
| **B — no university email yet** | multipart body **with ID card photos** | OTP verified → `pending`, an admin reviews the card |

A Door B student can move onto their university address later with
`POST /api/auth/change-email`, which activates the account without admin
review and deletes their stored ID card.

### User Status Lifecycle

| Status | Can Login? | How to reach |
|--------|-----------|--------------|
| `pending_verification` | No | Just registered, OTP not verified |
| `pending` | No | Email verified, waiting for admin approval (non-SUST) |
| `active` | Yes | Email verified (SUST), admin approved, or moved onto a SUST address |
| `rejected` | No | Admin rejected or banned the user |

Check any account's status without logging in: `GET /api/auth/status?email=...`

**Sessions.** Tokens last 7 days, but a password reset, a ban, or an admin
changing someone's email invalidates every token issued before that moment —
those requests return `401 "Session expired. Please login again."`

---

## 1. Authentication

### POST `/api/auth/register`
Create a new account. Sends a 6-digit OTP to the provided email.

**Access:** Public

**Request:**
```json
{
  "reg_number": "2021331083",
  "name": "Niloy Chandra Deb",
  "email": "2021331083@student.sust.edu",
  "password": "test123456",
  "codeforces_handle": "Unga_Bunga",
  "vjudge_handle": "neel_vj"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `reg_number` | string | Yes | 5-50 characters |
| `name` | string | Yes | 2-100 characters |
| `email` | string | Yes | Must be valid email format |
| `password` | string | Yes | 6-255 characters |
| `codeforces_handle` | string | No | 1-50 chars, validated against the Codeforces API |
| `vjudge_handle` | string | No | 1-100 chars, not verified (VJudge has no public lookup) |

Both handles are trimmed; an empty string counts as "not provided".
`vjudge_handle` is how the ranker maps VJudge standings to real names, so a
user without one shows as `unregistered` in results.

**Registering without a university email (Door B)**

Send the same fields as `multipart/form-data` plus two ID card photos. Anything
that is not an `@student.sust.edu` address **requires** them.

| Part | Required | Notes |
|------|----------|-------|
| `id_card_front` | Yes | JPEG / PNG / WebP, max 5 MB |
| `id_card_back` | Yes | Same |

Images are checked by file signature (not the filename or `Content-Type`),
resized to 1600px, and re-encoded — which strips EXIF, including GPS data. They
are stored privately and deleted once an admin approves or rejects the account.

```bash
curl -X POST http://localhost:8080/api/auth/register \
  -F "reg_number=2021331083" -F "name=Niloy Chandra Deb" \
  -F "email=someone@gmail.com" -F "password=test123456" \
  -F "id_card_front=@front.jpg" -F "id_card_back=@back.jpg"
```

**Success (201):**
```json
{
  "success": true,
  "user_id": 1,
  "status": "pending_verification",
  "message": "Registered — check your email for the verification code"
}
```

**Errors:**
- `400` — Validation failure, invalid Codeforces handle, unreadable/oversized image, only one ID card side sent, or a non-SUST email with no ID card
- `409` — Email or registration number already registered
- `429` — Too many OTP emails for this address

Nothing is created if any step fails — a bounced OTP email does not leave a
half-made account behind.

---

### POST `/api/auth/verify-otp`
Verify the email using the 6-digit code sent to the user's inbox.

**Access:** Public

**Request:**
```json
{
  "email": "2021331083@student.sust.edu",
  "code": "847293"
}
```

**Success (200):**
```json
{
  "success": true,
  "status": "active",
  "message": "Email verified — you can now log in"
}
```

> SUST emails (`@student.sust.edu`) become `active` immediately.
> Other emails become `pending` (admin approval required).

**Errors:**
- `400` — Invalid or expired verification code

---

### POST `/api/auth/resend-otp`
Resend the verification code. Only works for `pending_verification` accounts.

**Access:** Public

**Request:**
```json
{
  "email": "2021331083@student.sust.edu"
}
```

**Success (200):**
```json
{
  "success": true,
  "message": "New verification code sent — check your email"
}
```

**Errors:**
- `400` — Account already verified
- `404` — No account found with this email

---

### POST `/api/auth/login`
Login and receive a JWT token. Only `active` accounts can login.

**Access:** Public

**Request:**
```json
{
  "email": "2021331083@student.sust.edu",
  "password": "test123456"
}
```

**Success (200):**
```json
{
  "success": true,
  "token": "eyJ0eXAiOiJKV1QiLCJhbGci...",
  "user": {
    "user_id": 1,
    "name": "Niloy Chandra Deb",
    "email": "2021331083@student.sust.edu",
    "is_admin": false,
    "is_manager": false
  }
}
```

> Use the `token` in the `Authorization` header for all protected routes:
> `Authorization: Bearer <token>`

**Errors:**
- `401` — Invalid email or password
- `401` — Please verify your email first
- `401` — Account pending admin approval
- `401` — Account has been rejected

---

### POST `/api/auth/forgot-password`
Initiate a password reset. Returns the account holder's name and sends a 6-digit OTP to the email.

**Access:** Public

**Request:**
```json
{
  "email": "2021331083@student.sust.edu"
}
```

**Success (200):**
```json
{
  "success": true,
  "name": "Niloy Chandra Deb",
  "message": "Password reset code sent — check your email"
}
```

> The `name` field lets the frontend confirm to the user which account they're resetting.

**Errors:**
- `400` — Account is pending verification or has been rejected
- `404` — No account found with this email

---

### POST `/api/auth/reset-password`
Verify the reset OTP and set a new password. This is a single-step endpoint — provide the OTP and new password together.

**Access:** Public

**Request:**
```json
{
  "email": "2021331083@student.sust.edu",
  "code": "847293",
  "new_password": "mynewsecurepassword"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `email` | string | Yes | The email used in `/forgot-password` |
| `code` | string | Yes | 6-digit OTP from the reset email |
| `new_password` | string | Yes | 6-255 characters |

**Success (200):**
```json
{
  "success": true,
  "message": "Password reset successfully — you can now log in with your new password"
}
```

**Errors:**
- `400` — Invalid or expired reset code
- `400` — New password too short

---

### GET `/api/auth/status`
Check where an account stands without logging in — useful while a student is
waiting on admin approval.

**Access:** Public

`GET /api/auth/status?email=someone@gmail.com`

**Success (200):**
```json
{
  "success": true,
  "status": "pending",
  "message": "Your account is waiting for an admin to review it"
}
```

Returns the status string only, never personal data.

**Errors:** `404` — no account with that email · `429` — rate limited

---

### POST `/api/auth/change-email`
Move an account onto an `@student.sust.edu` address. Step 1 of 2.

**Access:** Public — authenticated by **password**, not a token, because
`pending` accounts cannot log in and so have no token to present.

**Request:**
```json
{
  "current_email": "someone@gmail.com",
  "password": "test123456",
  "new_email": "2021331083@student.sust.edu"
}
```

The 6-digit code is sent to `new_email` — receiving it is what proves ownership.

**Errors:**
- `400` — target is not an `@student.sust.edu` address, or is already your address
- `401` — wrong password or unknown account
- `403` — account is rejected
- `409` — that address is already in use
- `429` — rate limited

Re-sending the same request re-issues the code; it does not conflict with itself.

---

### POST `/api/auth/change-email/verify`
Step 2 of 2. Completes the change.

**Access:** Public

**Request:**
```json
{ "new_email": "2021331083@student.sust.edu", "code": "123456" }
```

**Success (200):**
```json
{
  "success": true,
  "email": "2021331083@student.sust.edu",
  "status": "active",
  "message": "Email updated and your account is now active — you can log in"
}
```

The account becomes `active` regardless of which waiting state it was in, and
any stored ID card is deleted — reading a university inbox establishes exactly
what the manual review was for.

**Errors:** `400` — no pending change for that address, or wrong/expired code · `409` — address taken in the meantime

---

## 2. User Profile

### GET `/api/users/me`
Get the logged-in user's profile.

**Access:** User (requires token)

**Success (200):**
```json
{
  "success": true,
  "data": {
    "user_id": 1,
    "reg_number": "2021331083",
    "name": "Niloy Chandra Deb",
    "email": "2021331083@student.sust.edu",
    "vjudge_handle": null,
    "codeforces_handle": "Unga_Bunga",
    "is_admin": false,
    "is_manager": false,
    "status": "active",
    "id_card_path": null
  }
}
```

---

### PUT `/api/users/me`
Update profile. All fields are optional — only send what you want to change.

**Access:** User (requires token)

**Request:**
```json
{
  "name": "Niloy Neel",
  "vjudge_handle": "neel_vj",
  "codeforces_handle": "neel_cf"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `name` | string | No | 2-100 characters, trimmed |
| `vjudge_handle` | string | No | 1-100 characters, trimmed |
| `codeforces_handle` | string | No | 1-50 characters, validated against the CF API |

Lengths count **characters, not bytes**, so a Bengali name is not cut short.
Sending `""` for either handle clears it to `null`; omitting a field leaves it
unchanged. An oversized value is a `400`, not a `500`.

**Success (200):**
```json
{
  "success": true,
  "message": "Profile updated successfully",
  "data": {
    "user_id": 1,
    "reg_number": "2021331083",
    "name": "Niloy Neel",
    "email": "2021331083@student.sust.edu",
    "vjudge_handle": "neel_vj",
    "codeforces_handle": "neel_cf",
    "is_admin": false,
    "is_manager": false,
    "status": "active",
    "id_card_path": null
  }
}
```

---

### GET `/api/users/{id}`
Look up a single user by id.

**Access:** Public — no token required

**Errors:** `404` — no such user

---

### GET `/api/users/search`
Search users by name. `GET /api/users/search?name=ne`

**Access:** Public — no token required

Returns at most 10 matches. Omitting `name` returns the first 10 users.

> **Note:** both of these are public and return each user's `email` and
> `reg_number`. That is a known open issue — locking them down changes a
> response the frontend consumes, so it is being handled together with the
> frontend work rather than piecemeal.

---

## 3. Codeforces Stats

### GET `/api/cf/profile/{user_id}`
Get a user's live Codeforces stats. Data is fetched in real-time from the Codeforces API.

**Access:** User (requires token)

**URL Params:** `user_id` (integer) — the user's ID from our database

**Success (200):**
```json
{
  "success": true,
  "data": {
    "codeforces_handle": "Unga_Bunga",
    "current_rating": 1250,
    "current_rank": "pupil",
    "max_rating": 1310,
    "max_rank": "pupil",
    "solve_counts": {
      "last_1_month": {
        "total": 12,
        "buckets": {
          "0-499": 0,
          "500-999": 0,
          "1000-1499": 2,
          "1500-1999": 3,
          "2000-2499": 4,
          "2500-2999": 2,
          "3000+": 1
        }
      },
      "last_6_months": {
        "total": 78,
        "buckets": {
          "0-499": 0,
          "500-999": 9,
          "1000-1499": 10,
          "1500-1999": 17,
          "2000-2499": 15,
          "2500-2999": 14,
          "3000+": 13
        }
      },
      "last_1_year": {
        "total": 188,
        "buckets": {
          "0-499": 0,
          "500-999": 23,
          "1000-1499": 33,
          "1500-1999": 36,
          "2000-2499": 36,
          "2500-2999": 30,
          "3000+": 30
        }
      }
    },
    "recent_contests": [
      {
        "contest_name": "Codeforces Round 1094 (Div. 1 + Div. 2)",
        "rank": 13,
        "old_rating": 3541,
        "new_rating": 3470,
        "rating_change": -71,
        "date": "2026-04-25T17:05:00"
      },
      {
        "contest_name": "Codeforces Round 1093 (Div. 1)",
        "rank": 44,
        "old_rating": 3755,
        "new_rating": 3541,
        "rating_change": -214,
        "date": "2026-04-13T16:35:00"
      }
    ]
  }
}
```

**Response field reference:**

| Field | Type | Description |
|-------|------|-------------|
| `current_rating` | int or null | Current CF rating |
| `current_rank` | string or null | Current CF rank title |
| `max_rating` | int or null | All-time highest rating |
| `max_rank` | string or null | All-time highest rank title |
| `solve_counts` | object | Unique accepted problems by time period |
| `solve_counts.*.total` | int | Total unique solves in period |
| `solve_counts.*.buckets` | object | Counts per 500-rating difficulty bucket |
| `recent_contests` | array | Last 15 rated contests (most recent first) |
| `recent_contests[].rating_change` | int | `new_rating - old_rating` (can be negative) |
| `recent_contests[].date` | string | ISO 8601 format |
| `contest_attendance` | array | Every contest since their first rated one — see below |
| `attendance_summary` | object | `total_contests`, `participated`, `missed`, `ineligible` |

**Contest attendance.** Every Codeforces contest from the member's **first rated
contest** onward, newest first, capped at 100, each flagged participated or not.

```json
{
  "attendance_summary": {
    "total_contests": 100, "participated": 14, "missed": 68, "ineligible": 18
  },
  "contest_attendance": [
    { "contest_id": 2153, "contest_name": "Codeforces Round 1118 (Div. 2)",
      "date": "2026-08-29T14:35:00", "participated": true, "eligible": true,
      "rank": 4049, "old_rating": 1252, "new_rating": 1282, "rating_change": 30 },
    { "contest_id": 2151, "contest_name": "Codeforces Round 1116 (Div. 1)",
      "date": "2026-08-09T14:35:00", "participated": false, "eligible": false,
      "rank": null, "old_rating": null, "new_rating": null, "rating_change": null }
  ]
}
```

| Field | Meaning |
|-------|---------|
| `participated` | They competed and it was rated for them |
| `eligible` | They were **allowed** to enter, judged against their rating at that time |
| `rank`, `old_rating`, `new_rating`, `rating_change` | Only present when `participated` |

`eligible` is false when the member could not enter (a 1282-rated pupil cannot
join a Div. 1 round) or the contest was unrated — an unrated entry never appears
in `user.rating`, so attendance is undetectable and calling it missed would be
wrong. Only `eligible && !participated` counts toward `missed`.

Division rules are read from the contest name, since Codeforces encodes them
nowhere else. Combined `Div. 1 + Div. 2` rounds count as open, and participation
always overrides the rule so a misparsed name can never hide a real entry.

`recent_contests` is unchanged and still returns the last 15 rated performances.

**Errors:**
- `404` — User not found or has no Codeforces handle

---

### GET `/api/cf/leaderboard`
Community leaderboard of all active registered users ranked by Codeforces rating.

**Access:** User (requires token)

**Success (200):**
```json
{
  "success": true,
  "count": 5,
  "data": [
    { "rank": 1, "name": "Niloy", "codeforces_handle": "Unga_Bunga", "current_rating": 1250 },
    { "rank": 2, "name": "Dipu", "codeforces_handle": "postmasterr", "current_rating": 1392 },
    { "rank": 3, "name": "Faiyaz", "codeforces_handle": "EDM_FI", "current_rating": 1292 },
    { "rank": 4, "name": "Alif", "codeforces_handle": "alif_new", "current_rating": null },
    { "rank": 4, "name": "Babul", "codeforces_handle": "babul_new", "current_rating": null }
  ]
}
```

**Leaderboard rules:**
- Rated users are sorted by `current_rating` descending (rank 1, 2, 3...)
- All unrated users share the same last rank with `current_rating: null`
- Only `active` users with a CF handle appear on the leaderboard

---

## Date Formats

Anywhere a datetime is accepted (`contest_date` on contests, `event_date` on
announcements — events have no date field), these all parse:

| Format | Example |
|--------|---------|
| Canonical | `2026-09-01T18:00:00` |
| Without seconds | `2026-09-01T18:00` |
| Space separated | `2026-09-01 18:00:00` |
| RFC 3339 with offset | `2026-09-01T18:00:00Z` — normalised to UTC |
| Empty string | `""` — clears the field |

Anything else is a `400`. Previously a malformed date was silently stored as
`null`.

---

## 4. Contests

### GET `/api/contests`
List all contests.

**Access:** User (requires token)

**Success (200):**
```json
{
  "success": true,
  "data": [
    {
      "contest_id": 1,
      "title": "TFC Round 8",
      "contest_link": "https://vjudge.net/contest/123",
      "contest_date": "2026-04-04T20:00:00",
      "created_at": "2026-03-28T10:00:00"
    }
  ]
}
```

---

### GET `/api/contests/{id}`
Get a single contest by ID.

**Access:** User (requires token)

---

### POST `/api/contests`
Create a new contest.

**Access:** Admin only

**Request:**
```json
{
  "title": "TFC Round 8",
  "contest_link": "https://vjudge.net/contest/123",
  "contest_date": "2026-04-04T20:00:00"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `title` | string | Yes | 1-255 characters |
| `contest_link` | string | No | URL to the contest |
| `contest_date` | string | No | ISO 8601 datetime |

---

### PUT `/api/contests/{id}`
Update a contest. All fields optional.

**Access:** Admin only

**Request:**
```json
{
  "title": "TFC Round 8 (Updated)"
}
```

---

### DELETE `/api/contests/{id}`
Delete a contest.

**Access:** Admin only

**Success (200):**
```json
{
  "success": true,
  "message": "Contest deleted"
}
```

---

## 5. Announcements

### GET `/api/announcements`
List announcements. **Pinned posts always come first**, then the chosen ordering.

**Access:** User (requires token) — announcements are not public

**Query parameters** (all optional):

| Param | Effect |
|-------|--------|
| `category` | Only this category. Case-insensitive; an unknown value is a `400` |
| `upcoming` | `true` → only posts whose `event_date` is still ahead, **soonest first** |
| `limit` | How many to return. Default `50`, clamped to `1-100` |

Without `upcoming`, the feed is newest-first by `created_at`.

```
GET /api/announcements?upcoming=true
GET /api/announcements?category=Contest&limit=10
```

**Success (200):**
```json
{
  "success": true,
  "count": 1,
  "data": [
    {
      "post_id": 12,
      "author_id": 5,
      "author_name": "Faiyaz Ismail",
      "title": "TFC Registration Open",
      "content": "Register before Friday.",
      "category": "Contest",
      "event_date": "2027-01-15T15:00:00",
      "created_at": "2026-08-19T18:30:12",
      "updated_at": null,
      "is_pinned": true,
      "link_url": "https://vjudge.net/contest/650000",
      "link_label": "Register here",
      "event_id": null,
      "contest_no": 1,
      "event_description": null,
      "contest_title": "TFC Round 8"
    }
  ]
}
```

| Field | Notes |
|-------|-------|
| `author_name` | Joined from users. `null` if that account was deleted |
| `updated_at` | `null` until the post is edited |
| `is_pinned` | Pinned posts sort above everything else |
| `link_url` / `link_label` | One outbound link — contest, blog, article or video — and its button text |
| `event_id` / `contest_no` | Optional tie to something already in the system |
| `event_description` / `contest_title` | Joined name of that tie, so you can label the link without another request |

---

### GET `/api/announcements/{id}`
Get a single announcement.

**Access:** User (requires token)

---

### GET `/api/announcements/categories`
The category values the API accepts. Build your dropdown from this rather than
hardcoding it.

**Access:** Public

```json
{ "success": true, "data": ["Contest", "Result", "Notice", "Update", "General"] }
```

---

### POST `/api/announcements`
Create a new announcement. The author is taken from your token.

**Access:** Admin **or Manager**

**Request:**
```json
{
  "title": "TFC Round 8 Registration",
  "content": "Register before Friday. Room 630, bring your own laptop.",
  "category": "Contest",
  "event_date": "2027-01-15T15:00:00",
  "is_pinned": true,
  "link_url": "https://vjudge.net/contest/650000",
  "link_label": "Register here",
  "contest_no": 1
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `title` | string | Yes | 1-255 characters |
| `content` | string | Yes | 1-10000 characters |
| `category` | string | No | One of `Contest`, `Result`, `Notice`, `Update`, `General` |
| `event_date` | string | No | See [Date Formats](#date-formats) |
| `is_pinned` | bool | No | Defaults to `false` |
| `link_url` | string | No | Max 500 chars, **must start with `http://` or `https://`** |
| `link_label` | string | No | Max 100 chars. Button text. Requires `link_url` |
| `event_id` | int | No | Ties the post to an event. `404` if it doesn't exist |
| `contest_no` | int | No | Ties the post to a contest. `404` if it doesn't exist |

**About the link.** One outbound link per post, meant for a contest, blog,
article or video. Only `http`/`https` is accepted — `javascript:` and `data:`
URLs are rejected with a `400`, since this value ends up as an anchor in the
frontend. Sending `link_label` without `link_url` is a `400`.

Deleting a referenced event or contest clears the tie; the post itself stays.

Category matching is case-insensitive and stored canonically — send `contest`,
get `Contest`. An unknown value is a `400` listing the valid ones. Omitting it,
or sending `""`, means no category.

Every read returns `author_name` (joined from users, `null` if that account was
deleted) and `updated_at` (`null` until the post is edited).

---

### PUT `/api/announcements/{id}`
Update an announcement. All fields optional — omitted fields keep their current
value. Stamps `updated_at`.

**Access:** Admin **or Manager**

Link editing:

| Sent | Result |
|------|--------|
| `link_url` + `link_label` | Both replaced |
| `link_label` only | Label replaced, existing URL kept |
| neither | Both unchanged |
| `link_url: ""` | Link cleared — **label is cleared too** |

Pin or unpin with `{"is_pinned": true}` / `{"is_pinned": false}`.

---

### DELETE `/api/announcements/{id}`
Delete an announcement.

**Access:** Admin **or Manager**

---

## 6. Events

Reading is **public**; writing requires admin or manager.

> Events have no `event_date`. A previous version of this document described
> `event_name`, `event_date`, `location`, `created_by`, `team_name` and
> `standing` — none of those exist. The shapes below were captured from a live
> round-trip.

### GET `/api/events`
List all events with their teams and members.

**Access:** Public — no token required

**Success (200):**
```json
{
  "success": true,
  "count": 1,
  "data": [
    {
      "event_id": 11,
      "title": "ICPC Dhaka Regional 2026",
      "description": "Team formation contests for the regional.",
      "vjudge_contest_ids": [650000, 600123],
      "merged_handles": [
        { "name": "boss_saim", "handles": ["alsaim", "alsaim199"] }
      ],
      "teams": [
        {
          "team_id": 6,
          "coach_name": "Dr Coach",
          "members": [
            { "member_id": 25, "reg_number": "2021331021", "user_id": 5, "name": "Faiyaz Ismail" },
            { "member_id": 26, "reg_number": "2021331011", "user_id": 6, "name": "Dipu Debnath" },
            { "member_id": 27, "reg_number": "2021331083", "user_id": 32, "name": "Niloy Chandra Deb" }
          ]
        }
      ]
    }
  ]
}
```

| Field | Notes |
|-------|-------|
| `title` | Required on create, 1-255 characters |
| `description` | Required, 1-10000 characters |
| `vjudge_contest_ids` | Optional array of VJudge contest ids |
| `merged_handles` | Optional; combines several VJudge handles under one name, same shape the ranker accepts |
| `teams[].coach_name` | Optional |
| `teams[].members[].user_id` / `name` | Joined from users on `reg_number`. **Both `null` when no account matches** — a member can be listed before they register |

---

### GET `/api/events/{id}`
One event with its teams.

**Access:** Public — no token required

**Errors:** `404` — no such event

---

### POST `/api/events`
Create an event.

**Access:** Admin or Manager

**Request:**
```json
{
  "title": "ICPC Dhaka Regional 2026",
  "description": "Team formation contests for the regional.",
  "vjudge_contest_ids": [650000, 600123],
  "merged_handles": [
    { "name": "boss_saim", "handles": ["alsaim", "alsaim199"] }
  ]
}
```

`title` and `description` are required; the other two are optional.

---

### PUT `/api/events/{id}`
Update an event. All fields optional — omitted fields keep their value.

**Access:** Admin or Manager

---

### DELETE `/api/events/{id}`
Delete an event. Its teams and their members go with it (`ON DELETE CASCADE`).

**Access:** Admin or Manager

---

### POST `/api/events/{event_id}/teams`
Add a team. **Exactly 3 members**, given as registration numbers.

**Access:** Admin or Manager

**Request:**
```json
{
  "coach_name": "Dr Coach",
  "members": ["2021331021", "2021331011", "2021331083"]
}
```

Members are plain registration-number strings, not objects. They are matched to
user accounts on read, so a number that belongs to nobody is still accepted and
comes back with `user_id: null`.

**Errors:** `400` — not exactly 3 members · `404` — no such event

---

### PUT `/api/events/{event_id}/teams/{team_id}`
Replace a team's coach and all 3 members.

**Access:** Admin or Manager

**Errors:** `400` — not exactly 3 members · `404` — the team does not belong to
that event

---

### DELETE `/api/events/{event_id}/teams/{team_id}`
Delete a team and its members.

**Access:** Admin or Manager

**Errors:** `404` — the team does not belong to that event

> The `event_id` in both team URLs is enforced. Passing a different event's id
> returns `404` rather than acting on the team.

---

## 7. Admin Panel

### GET `/api/admin/users`
List all users. Supports status filter via query params.

**Access:** Admin only

**Query params:** `?status=pending` | `?status=active` | `?status=rejected`

**Success (200):**
```json
{
  "success": true,
  "data": [
    {
      "user_id": 2,
      "reg_number": "2021331002",
      "name": "Someone",
      "email": "someone@gmail.com",
      "status": "pending",
      "is_admin": false,
      "is_manager": false
    }
  ]
}
```

---

### GET `/api/admin/users/{id}`
View a specific user's details.

**Access:** Admin only

---

### PUT `/api/admin/users/{id}/approve`
Approve a pending user (sets status to `active`).

**Access:** Admin only

**Success (200):**
```json
{
  "success": true,
  "message": "User approved"
}
```

---

### PUT `/api/admin/users/{id}/reject`
Reject a pending user.

**Access:** Admin only

---

### PUT `/api/admin/users/{id}/ban`
Ban an active user. Admins cannot ban themselves. Ends the user's sessions
immediately.

**Access:** Admin only

---

### GET `/api/admin/users/{id}/id-card`
Short-lived links to a pending student's ID card photos, for reviewing a Door B
registration. The bucket is private; these are the only way to view them.

**Access:** Admin only

**Success (200):**
```json
{
  "success": true,
  "data": {
    "front_url": "https://...supabase.co/storage/v1/object/sign/...",
    "back_url": "https://...",
    "expires_in_seconds": 300
  }
}
```

Both links expire after 5 minutes. The photos are deleted once the account is
approved or rejected, after which this returns `404`.

---

### PUT `/api/admin/users/{id}/reactivate`
Undo a ban or a rejection — sets the account back to `active`. `approve` only
accepts `pending`, so without this a mistaken ban was permanent.

**Access:** Admin only

**Errors:** `400` — user is already active · `404` — no such user

---

### PUT `/api/admin/users/{id}/email`
Correct a mistyped address. A student who typos their email never receives the
code and cannot self-serve a fix, since `change-email` only accepts university
addresses.

**Access:** Admin only

**Request:** `{ "email": "corrected@example.com" }`

Ends the user's sessions, since the address changed under them.

**Errors:** `409` — that address is already in use

---

### DELETE `/api/admin/users/{id}`
Remove an account outright. Also deletes any stored ID card. Admins cannot
delete themselves.

**Access:** Admin only

**Errors:** `400` — deleting yourself · `409` — the user has written announcements

---

## 8. Problemset

Curated practice material, grouped as sections → subsections → items.

### GET `/api/problems`
The full tree in one response.

**Access:** Public

---

### POST `/api/problems/sections`
**Access:** Admin only · `{ "name": "Graph Theory", "description": "..." }`

### POST `/api/problems/subsections`
**Access:** Admin only · `{ "section_id": 1, "name": "BFS and DFS", "description": "..." }`

### POST `/api/problems/items`
**Access:** Admin only

```json
{
  "subsection_id": 1,
  "item_type": "problem",
  "title": "Shortest Path",
  "url": "https://codeforces.com/problemset/problem/20/C",
  "platform": "Codeforces"
}
```

`url` must start with `http://` or `https://`. An unknown `section_id` or
`subsection_id` returns `404`. These three return `{"success": true}` with
status `200` and no id — re-fetch `GET /api/problems` to see the new row.

---

## 9. VJudge Contest Ranker

### POST `/api/ranker/analyze`
Analyze one or more VJudge contests and produce a ranked leaderboard. Fetches contest data directly from VJudge by contest ID.

**Access:** Public (no token required)

**Limits:** at most **50** contest IDs per request (each one is a separate
outbound VJudge fetch), and **10 requests per 5 minutes per IP**. Results are
cached for 6 hours for PDF download and are lost when the server restarts.

`problem_weights` is reflected in the reported `total_score` but does **not**
affect ranking order — rankings sort on raw solve count, then penalty, then
upsolved, with handle as a final tiebreak so equal rows stay stable between runs.

**Request:**
```json
{
  "title": "TFC Season 1 Final Standings",
  "contest_ids": [811682, 811683],
  "problem_weights": [[100, 200, 300, 400, 500, 600, 700], null],
  "custom_titles": ["TFC Round 1", "TFC Round 2"],
  "merged_handles": [
    {
      "name": "Soccho Merged",
      "handles": ["Soccho_27", "2021331027"]
    }
  ]
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `title` | string | Yes | Name for the result set (appears on PDF) |
| `contest_ids` | array of integers | Yes | VJudge contest IDs to analyze |
| `problem_weights` | array or null | No | Per-contest problem weights. `null` = all equal weight (1.0). Each entry can be an array of weights or `null`. |
| `custom_titles` | array of strings | No | Optional names to override the default VJudge contest titles. |
| `merged_handles` | array of objects | No | Optional list to merge multiple VJudge accounts into one participant. Each object requires `name` (string) and `handles` (array of strings). |

**Success (200):**
```json
{
  "success": true,
  "session_id": "a1b2c3d4-e5f6-...",
  "data": {
    "title": "TFC Season 1 Final Standings",
    "total_contests": 2,
    "total_participants": 30,
    "rankings": [
      {
        "rank": 1,
        "real_name": "Rafid Bin Nasim Soccho",
        "handle": "Soccho_27,2021331027",
        "total_score": 11.0,
        "problems_solved": 11,
        "total_upsolved": 2,
        "total_penalty": 162,
        "contests_participated": 2,
        "contest_details": [
          { "contest_name": "TFC Round 1", "solved": 6, "upsolved": 1, "penalty": 81, "score": 6.0, "participated": true },
          { "contest_name": "TFC Round 2", "solved": 5, "upsolved": 1, "penalty": 81, "score": 5.0, "participated": true }
        ]
      }
    ]
  }
}
```

**Ranking algorithm (ICPC-style):**
1. Sort by `problems_solved` DESC (higher is better)
2. Then by `total_penalty` ASC (lower is better)
3. Then by `total_upsolved` DESC (tiebreaker)
4. Equal solved + penalty + upsolved = same rank

**Penalty formula (Sum Seconds First):** `floor(sum_of_solve_time_seconds / 60) + (20 * total_wrong_attempts_before_AC)`

**Errors:**
- `400` — Empty title or empty contest_ids
- `400` — VJudge contest not found / not accessible
- `500` — VJudge API unreachable

---

### GET `/api/ranker/contest-title/{id}`
Fetch a VJudge contest's title. Useful for confirming an ID is reachable before
running a full analysis.

**Access:** Public

```json
{ "success": true, "title": "SUST Intra Contest 2026" }
```

---

### GET `/api/ranker/pdf/{session_id}`
Download a branded PDF of the ranking results.

**Access:** Public (no token required)

**URL Params:** `session_id` (string) — returned from the `/analyze` endpoint
**Query Params:** `?include_details=true|false` (boolean) — include individual contest details as nested rows

**Response:** `Content-Type: application/pdf`

The PDF contains:
- "SUST CP Geeks" header
- Custom title from the analyze request
- Table: `Rank | Handle | Contests Count | Solved | Penalty | Upsolved | Total Solved`
- Generation date footer

**Errors:**
- `404` — Session not found (run `/analyze` first)

> **Note:** Session data is stored in memory. It is lost when the server restarts.

---

## 10. Health Check

### GET `/api/health`
Check server and database connectivity.

**Access:** Public (no token required)

**Success (200):**
```json
{
  "success": true,
  "status": "healthy",
  "database": "connected"
}
```
