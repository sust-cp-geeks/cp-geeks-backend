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

        auth["🔐 auth & users<br/><i>register · login · OTP<br/>reset · admin approvals</i>"]
        content["📋 content<br/><i>contests · events<br/>announcements · teams</i>"]
        cf["📊 codeforces<br/><i>profile stats<br/>leaderboard</i>"]
        ranker["🏆 vjudge ranker<br/><i>ICPC ranking · PDF<br/>(cached sessions)</i>"]

        pipeline --> auth
        pipeline --> content
        pipeline --> cf
        pipeline --> ranker
    end

    db[("🐘 Neon<br/>PostgreSQL")]
    resend(["✉️ Resend"])
    cf_api(["🌐 Codeforces API"])
    vj_api(["🌐 VJudge API"])

    user == "REST / JSON" ==> pipeline

    auth -- "OTP emails" --> resend
    auth -- "users" --> db
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
    class db dbStyle;
    class resend,cf_api,vj_api extStyle;
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Runtime / Framework | Tokio · Axum 0.8 |
| Database | PostgreSQL (Neon) via SQLx |
| Auth | JWT · Argon2id |
| Email | Resend (OTP + password reset) |
| External APIs | Codeforces · VJudge |

## Getting Started

```bash
git clone git@github.com:sust-cp-geeks/cp-geeks-backend.git
cd cp-geeks-backend

cp .env.example .env   # fill in the variables below
cargo run              # serves at http://localhost:8080
```

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Yes | Neon PostgreSQL connection string |
| `JWT_SECRET` | Yes | Secret key for signing JWT tokens |
| `RESEND_API_KEY` | Yes | Resend API key for OTP emails |
| `RESEND_FROM_EMAIL` | No | Sender address (defaults to `onboarding@resend.dev`) |

## API

| Group | Endpoints | Access |
|-------|-----------|--------|
| Auth | `register`, `verify-otp`, `resend-otp`, `login`, `forgot-password`, `reset-password` | Public |
| Profile | `get_me`, `update_me` | User |
| Codeforces | `profile/{id}`, `leaderboard` | User |
| VJudge Ranker | `analyze`, `pdf/{session_id}` | Public |
| Contests | CRUD (5 endpoints) | User / Admin |
| Announcements | CRUD (5 endpoints) | User / Admin |
| Events + Teams | CRUD (8 endpoints) | User / Admin / Manager |
| Admin | User management (5 endpoints) | Admin |
| Health | Server status | Public |

Full request/response reference: [`docs/api.md`](docs/api.md)

## Project Structure

```
src/
├── main.rs          # entry point, router assembly, CORS
├── app_state.rs     # shared state (db pool + results cache)
├── config/          # database connection pool
├── models/          # request/response + domain types
├── handlers/        # HTTP handlers per resource
├── services/        # business logic, external API clients
├── middleware/      # JWT claims extractor
├── routes/          # route definitions per resource
└── utils/           # JWT + OTP helpers
```

## Security

- Argon2id password hashing · JWT (HMAC-SHA256, 7-day expiry)
- Email OTP verification — 6-digit, 10-minute expiry, single-use
- Parameterized SQL queries, user-enumeration prevention, no hash exposure in responses

## License

MIT — built by [SUST CP Geeks](https://github.com/sust-cp-geeks)
