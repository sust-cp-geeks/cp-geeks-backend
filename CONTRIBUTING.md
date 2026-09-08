# Contributing

This project is maintained by students, and most people who touch it are working
on a backend for the first time. The guidance below is written for that: it says
what to run and, where it matters, why — so you can tell a real failure from a
normal one instead of guessing.

If you are reporting something broken rather than changing code, you want
[`docs/triage.md`](docs/triage.md) first. It will often answer the question
faster than an issue can, and if it doesn't, it tells you what to put in one.

---

## Setting up

You need `cmake` and a C compiler alongside Rust. TLS here is handled by rustls,
whose crypto backend compiles from C; without them the build dies inside
`aws-lc-sys` with an error that never mentions TLS.

```bash
sudo apt install build-essential cmake        # debian / ubuntu

git clone git@github.com:sust-cp-geeks/cp-geeks-backend.git
cd cp-geeks-backend
cp .env.example .env                          # fill it in — see the README table
for f in migrations/*.sql; do psql "$DATABASE_URL" -f "$f"; done
cargo run                                     # http://localhost:8080
```

The first `cargo` command may spend a minute downloading a compiler. That is
`rust-toolchain.toml` doing its job: the version is pinned so that CI and your
laptop lint against exactly the same rules. Do not override it. We pinned it
after a Rust release added a lint that failed CI on a file nobody had touched,
while the same command locally reported nothing.

---

## Before you push

Run the three commands CI runs. They are the whole gate — if they pass locally
they pass in CI, which is the entire reason the toolchain is pinned:

```bash
cargo build --locked --all-targets
cargo test --locked
cargo clippy --all-targets -- -D warnings
```

`clippy` is enforced, not advisory. The baseline is zero warnings, so anything
it prints came from your change. If you believe a lint is wrong, say so in the
pull request rather than adding `#[allow]` quietly — sometimes it is wrong, and
that is worth a sentence.

`--locked` builds the exact versions in `Cargo.lock` instead of silently
resolving newer ones. If it fails because the lockfile is stale, that is a real
change and belongs in its own commit.

### Tests

Tests here do not need a database. That is deliberate: where a rule matters, the
rule gets pulled out into a plain function that can be tested directly — see
`check_role_change` in `admin_handler.rs`, `standings_order` in `ranker.rs`, or
`degraded_profile` in `codeforces_handler.rs`. If you are writing logic worth
trusting, do the same, and the test comes for free.

---

## Branches

`dev` is where work lands. `main` is what gets deployed. CI runs on both, so a
merge to `main` is never the first time the tests run.

```
your work ──▶ dev ──▶ main ──▶ deploy
```

Small, obvious changes can go straight to `dev`. Anything that changes an
endpoint's behaviour, touches auth, or is large enough to want a second pair of
eyes should be a pull request into `dev`. `main` then moves by fast-forward from
`dev` once CI is green.

> **Check the base branch on every pull request.** `main` is the repository
> default, so GitHub pre-fills it as the base. Almost every PR here should
> target `dev` instead — change it in the dropdown before you open the PR.

`main` is protected, and the protection is enforced for admins too, so it
applies to all three of us:

- a commit cannot reach `main` unless the **`build, test, lint`** check has
  already passed on that exact commit — which it will have, from the run on
  `dev`
- no force pushes, no deleting the branch

That is why the fast-forward still works: pushing `dev` first runs CI, and the
green check belongs to the commit rather than to the branch. Fast-forward
`main` before that run finishes and the push is rejected — wait for it to go
green, then push.

---

## Commits

One logical change per commit. The subject line is `type: lowercase summary` in
the imperative:

```
fix: serve the last synced codeforces profile when the api is down
feat: change member roles from the admin page
build: pin the rust toolchain so ci and laptops lint alike
docs: name the build dependencies rustls introduced
```

Types in use: `feat`, `fix`, `refactor`, `docs`, `build`, `ci`, `deploy`, `test`.

**The body is where the value is.** Say what was wrong and what the change does
about it — cause and effect, not a restatement of the diff. Six months from now
the diff will still be readable and the reason will not be. A body that explains
why a fallback exists, or why a lint was disabled, is worth more than a
perfectly formatted subject line.

---

## Pull requests

Fill in the template. The important part is the same as a commit body: what
breaks today, and what your change does about it.

Before asking for review, confirm the three commands above pass. A PR that fails
CI is not ready, and reviewing it wastes someone's evening.

---

## Keeping the docs true

Two files go stale silently, so update them in the same commit as the change:

- **`docs/api.md`** — every endpoint, request and response. It is currently
  exactly in sync with the code: 54 endpoints documented, 54 in the router. If
  you add, remove or change the shape of an endpoint, it changes here too.
- **`README.md`** — the architecture diagram and the structure tree. If you add
  a service, a background job or a top-level directory, the diagram is now
  wrong. The diagram is Mermaid, in the README, in text — edit it in place.

A wrong diagram is worse than no diagram, because people believe it.

---

## Never commit

- `.env`, or any real key, token or connection string. If one is ever pushed,
  treat it as leaked: rotate it, do not just delete the commit.
- `target/`, `dist/`, `node_modules/` — all gitignored.
- Scratch notes and one-off markdown. If it is not meant to be read by the next
  person, it does not belong in the repo.

---

## Getting unstuck

The architecture diagram in the README shows what talks to what, which is
usually enough to work out which part is failing. Beyond that,
[`docs/triage.md`](docs/triage.md) covers the failure modes we have actually hit
in production, with the symptom each one produces.

If you are still stuck, open an issue. A question that turns out to be a real
bug is a good issue, and a question that turns out to be a misunderstanding is
worth documenting anyway.
