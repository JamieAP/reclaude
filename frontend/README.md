# Reclaude frontend

This React and TypeScript application displays data captured by Reclaude. It has
views for activity statistics, events, sessions, repositories, persona usage, and
saved focus summaries. It reads the local Rust backend through `/api`.

See the [project README](../README.md) for hook installation and the full privacy
and data-flow details.

## Development

You need Rust and Cargo for the backend, and Node.js and npm for the frontend.

Start the backend from the repository root in one terminal:

```bash
cargo run -- ui --no-open
```

In a second terminal, also starting from the repository root:

```bash
cd frontend
npm ci
npm run dev
```

Open the URL printed by Vite. Its development server proxies `/api` to the
backend at `http://localhost:8420`. If you change the backend port, update the
proxy in [vite.config.ts](vite.config.ts). The UI uses captured records from
`~/.reclaude/metadata.db`; a new database has no session activity to display.

With frontend dependencies installed and `reclaude` on PATH, `just serve` from
the repository root can start both servers instead.

## Build and serve

From the repository root:

```bash
cd frontend
npm ci
npm run build
cd ..
cargo run -- ui --no-open
```

The build checks TypeScript and writes the bundled UI to `frontend/dist`. Open
`http://127.0.0.1:8420` to use the UI served by the Rust backend. If no build is
found, that server runs in API-only mode.

For an installed binary outside the checkout, put the built files in
`~/.reclaude/frontend/dist` or run the binary from the repository root. The server
also checks for a build relative to a binary in the checkout's `target` directory.

## Checks

Run these from `frontend/`:

```bash
npm run lint
npm run build
```

## Local access

The backend binds to `127.0.0.1:8420` by default and provides no authentication.
Use the local Vite proxy during development. Keep both servers on loopback;
access from another machine requires a separately managed authenticated proxy.
Local processes and code in the UI origin can access the captured session data.

## License

[MIT](../LICENSE). Dependencies retain their own licenses and notices.
