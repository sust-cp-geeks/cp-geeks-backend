# SUST CP Geeks Backend

REST API powering the SUST Competitive Programming Community Platform — built with Rust, Axum, and PostgreSQL.

![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust&logoColor=white)
![Axum](https://img.shields.io/badge/Axum-0.8-blue)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-Neon-316192?logo=postgresql&logoColor=white)
![License](https://img.shields.io/badge/License-MIT-green)

## Architecture

```mermaid
%%{init: {"flowchart": {"nodeSpacing": 22, "rankSpacing": 48}}}%%
flowchart TD
    user["👤 User<br/>Web Frontend / API Client"]

    subgraph backend["⚙️ cp-geeks backend · Rust + Axum + Tokio"]
        pipeline["CORS + Tracing → Axum Router → JWT Auth Guard"]

        auth["🔐 auth & users<br/><i>register · login · OTP<br/>ID cards · admin approvals</i>"]
        content["📋 content<br/><i>contests · events · teams<br/>announcements · problemset</i>"]
        cf["📊 codeforces<br/><i>profile stats<br/>leaderboard</i>"]
        ranker["🏆 vjudge ranker<br/><i>ICPC ranking · PDF<br/>(cached sessions)</i>"]

        pipeline --> auth
        pipeline --> content
        pipeline --> cf
        pipeline --> ranker
    end

    db[("🐘 Neon<br/>PostgreSQL")]
    store[("🗄️ Supabase<br/>Storage")]
    resend(["✉️ Resend"])
    cf_api(["🌐 Codeforces API"])
    vj_api(["🌐 VJudge API"])

    user == "REST / JSON" ==> pipeline

    auth -- "OTP emails" --> resend
    auth -- "users" --> db
    auth -- "ID card photos" --> store
    content -- "CRUD" --> db
    cf -- "handles" --> db
    cf -- "ratings · solves" --> cf_api
    ranker -- "standings" --> vj_api

    style backend fill:#f6f8fa,stroke:#8b949e,color:#24292f,stroke-width:1.5px;

    classDef userStyle fill:#d6ccff,stroke:#7c3aed,color:#1e1b4b,stroke-width:2px;
    classDef pipeStyle fill:#f3d1f4,stroke:#c026d3,color:#4a044e,stroke-width:2px;
    classDef svcStyle fill:#b9f6ca,stroke:#15803d,color:#052e16,stroke-width:2px;
    classDef ioStyle fill:#bbdefb,stroke:#1d4ed8,color:#172554,stroke-width:2px;
    classDef dbStyle fill:#fff,stroke:#334155,color:#0f172a,stroke-width:2px;
    classDef extStyle fill:#a7f3d0,stroke:#0f766e,color:#042f2e,stroke-width:2px;

    class user userStyle;
    class pipeline pipeStyle;
    class auth,content svcStyle;
    class cf,ranker ioStyle;
    class db,store dbStyle;
    class resend,cf_api,vj_api extStyle;
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Runtime / Framework | Tokio · Axum 0.8 |
| Database | PostgreSQL (Neon) via SQLx |
| Auth | JWT · Argon2id · rate-limited OTP |
| File storage | Supabase Storage (private bucket, signed URLs) |
| Email | Resend (OTP + password reset) |
| External APIs | Codeforces · VJudge |

## Getting Started

```bash
git clone git@github.com:sust-cp-geeks/cp-geeks-backend.git
cd cp-geeks-backend

cp .env.example .env   # fill in the variables below
cargo run              # serves at http://localhost:8080
```

Apply the schema to a fresh database in order — the files are idempotent, so
re-running them is safe:

```bash
for f in migrations/*.sql; do psql "$DATABASE_URL" -f "$f"; done
```

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Yes | Neon PostgreSQL connection string |
| `JWT_SECRET` | Yes | Secret key for signing JWT tokens |
| `RESEND_API_KEY` | Yes | Resend API key for OTP emails |
| `SUPABASE_URL` | For ID cards | Supabase project URL |
| `SUPABASE_SECRET_KEY` | For ID cards | Secret key — not the publishable one |
| `SUPABASE_BUCKET` | For ID cards | Private bucket for ID card photos |
| `RESEND_FROM_EMAIL` | No | Sender address (defaults to `onboarding@resend.dev`) |
| `CORS_ALLOWED_ORIGINS` | No | Comma-separated allowed origins (defaults to localhost `:5173`, `:4173`, `:3000`) |
| `PORT` | No | Listen port (defaults to `8080`) |
| `RUST_LOG` | No | Log filter (defaults to `info,tower_http=debug`) |

## API

| Group | Endpoints | Access |
|-------|-----------|--------|
| Auth | `register`, `verify-otp`, `resend-otp`, `login`, `forgot-password`, `reset-password`, `status`, `change-email` ×2 | Public |
| Profile | `me` (get/update), `{id}`, `search` | User · `{id}` and `search` are public |
| Codeforces | `profile/{id}`, `leaderboard` | User |
| Contests | CRUD (5) | User / Admin |
| Announcements | CRUD + `categories` (6) | User / Admin · Manager |
| Events + Teams | CRUD (8) | Public read / Admin · Manager |
| Problemset | `GET /` + 3 create endpoints | Public read / Admin write |
| Admin | User management, ID card review, recovery (9) | Admin |
| VJudge Ranker | `analyze`, `pdf/{session_id}`, `contest-title/{id}` | Public |
| Health | Server status | Public |

Full request/response reference: [`docs/api.md`](docs/api.md)

## Project Structure

```
src/
├── main.rs          # entry point, router, CORS, graceful shutdown
├── app_state.rs     # shared state (db pool, ranker cache, rate limiter)
├── errors.rs        # AppError → HTTP response, role guards
├── validation.rs    # shared input + datetime validation
├── config/          # database connection pool
├── models/          # request/response + domain types
├── handlers/        # HTTP handlers per resource
├── services/        # external clients (codeforces, vjudge, email,
│                    #   storage, image processing) + ranker logic
├── middleware/      # JWT extractor + session-validity check
├── routes/          # route definitions per resource
└── utils/           # JWT, OTP, rate limiting

migrations/          # schema, applied in filename order
docs/api.md          # full request/response reference
fonts/               # bundled TTFs for ranker PDF export
```

## Security

- **Passwords** — Argon2id hashing, never returned in any response
- **Tokens** — JWT (HMAC-SHA256, 7-day expiry). A password reset, ban, or admin
  email change invalidates every token issued before it
- **OTP** — 6-digit, 10-minute expiry, single-use, and burned after 5 wrong guesses
- **Rate limits** — per-email on OTP attempts, logins and outbound mail; per-IP on the ranker
- **ID cards** — private bucket, 5-minute signed URLs, EXIF stripped on upload,
  deleted as soon as an admin decides
- **Queries** — parameterized throughout; database errors are logged, never echoed to clients

## License

MIT — built by [SUST CP Geeks](https://github.com/sust-cp-geeks)
