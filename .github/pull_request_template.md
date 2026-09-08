## What this changes

<!-- What was wrong, and what this does about it. Cause and effect, not a
     restatement of the diff — the diff is already readable, the reason is not. -->

## Why it was wrong

<!-- The part that is worth writing down. If this is a fix, what was the actual
     failure? If it's a feature, what could nobody do before? -->

## How it was checked

<!-- What you ran, not what you believe. "Added three tests covering the
     last-admin case" or "hit the endpoint with a bad handle and got the stale
     response" — both beat "tested locally". -->

## Checklist

- [ ] `cargo build --locked --all-targets` passes
- [ ] `cargo test --locked` passes
- [ ] `cargo clippy --all-targets -- -D warnings` is clean
- [ ] `docs/api.md` updated, if any endpoint changed shape
- [ ] `README.md` diagram updated, if a service or background job was added
- [ ] No secrets, `.env` files or connection strings in the diff

<!-- Targets `dev`. `main` only moves by fast-forward from `dev` once CI is green. -->
