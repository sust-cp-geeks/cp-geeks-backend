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
| `500` | Internal server error |

---

## Authentication Flow

```
Register ──> OTP Email ──> Verify OTP ──> Login ──> JWT Token
                                            │
                              SUST email? ──┤──> status: active (can login)
                              Other email? ─┘──> status: pending (admin approval needed)
```

### User Status Lifecycle

| Status | Can Login? | How to reach |
|--------|-----------|--------------|
| `pending_verification` | No | Just registered, OTP not verified |
| `pending` | No | Email verified, waiting for admin approval (non-SUST) |
| `active` | Yes | Email verified (SUST) or admin approved |
| `rejected` | No | Admin rejected the user |

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
  "codeforces_handle": "Unga_Bunga"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `reg_number` | string | Yes | 5-50 characters |
| `name` | string | Yes | 2-100 characters |
| `email` | string | Yes | Must be valid email format |
| `password` | string | Yes | 6-255 characters |
| `codeforces_handle` | string | Yes | Validated against Codeforces API |

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
- `400` — Invalid Codeforces handle / validation failure
- `409` — Email already registered

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
| `name` | string | No | 2-100 characters |
| `vjudge_handle` | string | No | VJudge username |
| `codeforces_handle` | string | No | Validated against CF API if provided |

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
List all announcements.

**Access:** User (requires token)

**Success (200):**
```json
{
  "success": true,
  "data": [
    {
      "announcement_id": 1,
      "author_id": 5,
      "title": "Welcome to SUST CP Geeks",
      "content": "We are excited to launch...",
      "category": "general",
      "event_date": null,
      "created_at": "2026-03-28T10:00:00"
    }
  ]
}
```

---

### GET `/api/announcements/{id}`
Get a single announcement.

**Access:** User (requires token)

---

### POST `/api/announcements`
Create a new announcement.

**Access:** Admin only

**Request:**
```json
{
  "title": "Weekly Contest Reminder",
  "content": "This week's contest will be held on...",
  "category": "contest",
  "event_date": "2026-04-10T20:00:00"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `title` | string | Yes | 1-255 characters |
| `content` | string | Yes | 1-10000 characters |
| `category` | string | No | e.g. "general", "contest", "event" |
| `event_date` | string | No | ISO 8601 datetime (`YYYY-MM-DDTHH:MM:SS`) |

---

### PUT `/api/announcements/{id}`
Update an announcement. All fields optional.

**Access:** Admin only

---

### DELETE `/api/announcements/{id}`
Delete an announcement.

**Access:** Admin only

---

## 6. Events

### GET `/api/events`
List all events with their teams and team members.

**Access:** User (requires token)

**Success (200):**
```json
{
  "success": true,
  "data": [
    {
      "event_id": 1,
      "event_name": "ICPC Dhaka Regional 2025",
      "event_date": "2025-12-15T09:00:00",
      "location": "BUET, Dhaka",
      "description": "Annual ICPC regional contest",
      "created_by": 5,
      "created_at": "2025-11-01T10:00:00",
      "teams": [
        {
          "team_id": 1,
          "team_name": "SUST_Sigma",
          "standing": "3rd",
          "members": [
            { "member_id": 1, "member_name": "Neel", "codeforces_handle": "Unga_Bunga" },
            { "member_id": 2, "member_name": "Dipu", "codeforces_handle": "postmasterr" },
            { "member_id": 3, "member_name": "Faiyaz", "codeforces_handle": "EDM_FI" }
          ]
        }
      ]
    }
  ]
}
```

---

### GET `/api/events/{id}`
Get a single event with teams.

**Access:** User (requires token)

---

### POST `/api/events`
Create a new event.

**Access:** Admin/Manager

**Request:**
```json
{
  "event_name": "ICPC Dhaka Regional 2025",
  "event_date": "2025-12-15T09:00:00",
  "location": "BUET, Dhaka",
  "description": "Annual ICPC regional contest"
}
```

---

### PUT `/api/events/{id}`
Update an event. All fields optional.

**Access:** Admin/Manager

---

### DELETE `/api/events/{id}`
Delete an event and all its teams.

**Access:** Admin/Manager

---

### POST `/api/events/{event_id}/teams`
Add a team to an event (3 members required).

**Access:** Admin/Manager

**Request:**
```json
{
  "team_name": "SUST_Sigma",
  "standing": "3rd",
  "members": [
    { "member_name": "Neel", "codeforces_handle": "Unga_Bunga" },
    { "member_name": "Dipu", "codeforces_handle": "postmasterr" },
    { "member_name": "Faiyaz", "codeforces_handle": "EDM_FI" }
  ]
}
```

---

### PUT `/api/events/{event_id}/teams/{team_id}`
Update a team.

**Access:** Admin/Manager

---

### DELETE `/api/events/{event_id}/teams/{team_id}`
Delete a team.

**Access:** Admin/Manager

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
Ban an active user. Admins cannot ban themselves.

**Access:** Admin only

---

## 9. VJudge Contest Ranker

### POST `/api/ranker/analyze`
Analyze one or more VJudge contests and produce a ranked leaderboard. Fetches contest data directly from VJudge by contest ID.

**Access:** Public (no token required)

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
