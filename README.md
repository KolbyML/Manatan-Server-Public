# Manatan-Server-Public

Public wrapper crate for Manatan-Server that links against prebuilt static libraries.

This repo does not include any private server source code. It provides:
- A Rust API compatible with Manatan (`build_state`, `build_router_without_cors`, `Config`)
- Target-specific static libraries stored under `lib/<target>/`

## Layout

- `src/` - public Rust wrapper and proxy router
- `lib/<target>/` - optional local static libraries for offline builds

## Environment overrides

- `MANATAN_BACKEND_HOST` (default: `127.0.0.1`)
- `MANATAN_BACKEND_PORT` (default: `MANATAN_PORT + 1`)

These control where the embedded Manatan-Server static library is started. The public router
proxies requests to that backend.

## Building

Place the static library for your target in `lib/<target>/` (or download the latest
release asset) and build normally.

Example:

```
lib/x86_64-unknown-linux-gnu/libmanatan_server.a
```

## Workflow

Static libraries are published from the private Manatan-Server repository to the
`stable` release tag. Publishing uses release assets only (no library history is
stored in git).
