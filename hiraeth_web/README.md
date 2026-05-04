# Hiraeth Web UI

This crate serves the browser-based administration UI.

## Frontend workflow

The Tailwind/Daisy build currently requires Node.js 20 or newer.

Install frontend build dependencies once:

```bash
cd hiraeth_web
npm install --include=optional
```

Build the stylesheet directly:

```bash
cd hiraeth_web
npm run build:css
```

Watch and rebuild during UI work:

```bash
cd hiraeth_web
npm run watch:css
```

## Notes

- `assets/app.css` is generated and ignored from version control.
- `cargo build`, `cargo check`, `cargo test`, and Docker builds that compile `hiraeth_web` regenerate `assets/app.css` automatically after `npm install --include=optional` has been run.
- App-owned assets like `app.css`, `app.js`, and the favicon are embedded in the binary and served with immutable caching (`max-age=31536000, immutable`).
- Vendored assets under `assets/vendor/` are treated as stable and cached aggressively.
