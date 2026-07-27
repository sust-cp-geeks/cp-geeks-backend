<div align="center">

# SUST CP Geeks Backend

**High-performance REST API powering the SUST Competitive Programming Community Platform**

[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Axum](https://img.shields.io/badge/Axum-0.8-blue?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-316192?style=for-the-badge&logo=postgresql&logoColor=white)](https://neon.tech/)
[![JWT](https://img.shields.io/badge/JWT-000000?style=for-the-badge&logo=jsonwebtokens&logoColor=white)](https://jwt.io/)

</div>

---

## Tech Stack

| Layer | Technology | Purpose |
|-------|-----------|---------|
| Runtime | Tokio | Async runtime with work-stealing scheduler |
| Framework | Axum 0.8 | Ergonomic, type-safe HTTP framework |
| Database | PostgreSQL (Neon) | Serverless Postgres with connection pooling |
| ORM | SQLx | Compile-time checked SQL queries |
| Auth | JWT + Argon2id | Stateless authentication with memory-hard hashing |
| Email | Resend | Transactional email for OTP verification + password reset |
| External API | Codeforces | Live rating, solve stats, contest history |
| External API | VJudge | Contest standings for ICPC-style ranking |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [PostgreSQL](https://neon.tech/) database (Neon recommended)
- [Resend](https://resend.com/) API key (for email OTP)

### Setup

```bash
git clone git@github.com:sust-cp-geeks/cp-geeks-backend.git
cd cp-geeks-backend

cp .env.example .env
# edit .env — see Environment Variables below

cargo run
```

The server starts at **`http://localhost:8080`**

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Yes | Neon PostgreSQL connection string |
| `JWT_SECRET` | Yes | Secret key for signing JWT tokens |
| `RESEND_API_KEY` | Yes | Resend API key for OTP emails |
| `RESEND_FROM_EMAIL` | No | Sender address (defaults to `onboarding@resend.dev`) |

## API Endpoints Overview

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

> **Full API Reference with request/response shapes:** [`docs/api.md`](docs/api.md)

## Architecture

```mermaid
flowchart TD
    %% Client Tier
    subgraph CL ["Client Layer"]
        client["Web Frontend / API Clients"]
    end

    %% Gateway Tier
    subgraph GW ["" ]
        direction LR
        cors["CORS & Tracing"]
        jwt["JWT Auth Guard"]
        router["Axum Router"]
    end

    %% Application Layer
    subgraph APP ["Core Application Services & Modules"]
        auth_svc[" Auth & User Management\n(Register, Login, OTP, Admin Approvals)"]
        ranker_svc[" VJudge Ranker Service\n(ICPC Ranking Engine, PDF Generator)"]
        cf_svc[" Codeforces Integration\n(Profile Stats & Community Leaderboard)"]
        crud_svc[" Community Content Management\n(Contests, Announcements, Events & Teams)"]
    end

    %% Infrastructure & External Layer
    subgraph INFRA ["Data Layer & External Integrations"]
        direction LR
        db[("🐘 Neon PostgreSQL\n(Serverless Database)")]
        resend[" Resend API\n(Transactional Email)"]
        cf_api[" Codeforces API\n(Live User Data)"]
        vj_api[" VJudge API\n(Contest Standings)"]
    end

    %% Flow Connections
    client -->|"HTTP / REST Requests"| cors
    cors --> jwt
    jwt --> router

    router -->|"Auth & Admin Routes"| auth_svc
    router -->|"Ranker & PDF Routes"| ranker_svc
    router -->|"CF Stats Routes"| cf_svc
    router -->|"CRUD Routes"| crud_svc

    auth_svc -->|"User Profiles & Passwords"| db
    auth_svc -->|"Dispatch Verification Emails"| resend

    ranker_svc -->|"Fetch Live Standings JSON"| vj_api
    ranker_svc -->|"Query User Handles"| db

    cf_svc -->|"Fetch Submissions & Rating History"| cf_api

    crud_svc -->|"Persist Contests & Team Data"| db

    %% Professional Styling
    classDef clientStyle fill:#1e1b4b,color:#e0e7ff,stroke:#6366f1,stroke-width:2px;
    classDef gwStyle fill:#0f172a,color:#f8fafc,stroke:#38bdf8,stroke-width:2px;
    classDef appStyle fill:#064e3b,color:#ecfdf5,stroke:#34d399,stroke-width:2px;
    classDef infraStyle fill:#311042,color:#fae8ff,stroke:#c084fc,stroke-width:2px;

    class client clientStyle;
    class cors,jwt,router gwStyle;
    class auth_svc,ranker_svc,cf_svc,crud_svc appStyle;
    class db,resend,cf_api,vj_api infraStyle;
```

### Request Lifecycle

```
Client Request
     │
     ▼
┌─────────────┐
│  CORS Layer  │  ← allows frontend origins
└──────┬──────┘
       ▼
┌─────────────┐
│ Axum Router  │  ← matches route → handler
└──────┬──────┘
       ▼
┌─────────────┐
│JWT Middleware│  ← extracts Claims from Bearer token (protected routes only)
└──────┬──────┘
       ▼
┌─────────────┐
│   Handler    │  ← validates input, orchestrates logic
└──────┬──────┘
       ▼
┌─────────────┐
│   Service    │  ← business logic, external API calls
└──────┬──────┘
       ▼
┌─────────────┐
│  Database /  │  ← SQLx queries (Neon PostgreSQL)
│ External API │  ← reqwest HTTP calls (Codeforces, VJudge, Resend)
└─────────────┘
```

## Project Structure

```
src/
├── main.rs                      # entry point, router assembly, cors
├── app_state.rs                 # shared application state (db pool + results cache)
├── errors.rs                    # unified AppError enum + IntoResponse
├── validation.rs                # input validation helpers
├── config/
│   └── database.rs              # neon postgres connection pool
├── models/
│   ├── user.rs                  # User, RegisterInput, LoginInput
│   ├── contest.rs               # Contest, CreateContest, UpdateContest
│   ├── announcement.rs          # Announcement, CreateAnnouncement
│   ├── event.rs                 # Event, Team, TeamMember
│   ├── codeforces.rs            # CF API types, ProfileStats, Leaderboard
│   └── ranker.rs                # VJudge contest types, RankerRequest/Response
├── handlers/
│   ├── auth_handler.rs          # register, login, OTP, password reset
│   ├── user_handler.rs          # profile management
│   ├── admin_handler.rs         # user approval, rejection, banning
│   ├── contest_handler.rs       # contest CRUD
│   ├── announcement_handler.rs  # announcement CRUD
│   ├── event_handler.rs         # event + team CRUD
│   ├── codeforces_handler.rs    # CF profile stats, leaderboard
│   ├── ranker_handler.rs        # VJudge ranker + PDF download
│   └── health_handler.rs        # health check
├── services/
│   ├── email.rs                 # OTP + password reset emails via Resend API
│   ├── codeforces.rs            # CF API client (validate, fetch, aggregate)
│   ├── vjudge.rs                # VJudge contest data fetcher
│   └── ranker.rs                # ICPC ranking (Sum Seconds First) + multi-contest handle merge
├── middleware/
│   └── auth_middleware.rs       # JWT claims extractor
├── routes/                      # route definitions per resource
└── utils/
    ├── jwt.rs                   # token creation + verification
    └── otp.rs                   # OTP generation, storage, verification
```

## Security

- Argon2id password hashing (memory-hard, salt-per-user)
- Email OTP verification (6-digit, 10-minute expiry, single-use)
- JWT stateless auth with HMAC-SHA256 signing (7-day expiry)
- Parameterized SQL queries (zero injection surface)
- User enumeration prevention on login
- Codeforces handle validation against live API
- Password hashes never exposed in API responses

## License

MIT

---

<div align="center">

**Built with Rust by [SUST CP Geeks](https://github.com/sust-cp-geeks)**

</div>
