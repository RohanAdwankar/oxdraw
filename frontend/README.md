## Oxdraw Next.js Frontend

This directory contains the client application that powers the Oxdraw interactive editor. It is a statically-exported Next.js app that talks to the Rust CLI over HTTP.

### Commands

```bash
# install dependencies
npm install

# run a local dev server (http://localhost:3000)
npm run dev

# create a production export consumed by `oxdraw --edit`
npm run build
```

During development run the CLI server separately (for example `cargo run -- serve --input <file>`), then point the frontend at it by setting `NEXT_PUBLIC_OXDRAW_API=http://127.0.0.1:5151` before `npm run dev`.

The `npm run build` step produces a static bundle in `frontend/out/`. The Oxdraw CLI automatically serves those assets when you launch `oxdraw --edit`.
