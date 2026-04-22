# Hiraeth Web UI

This crate serves the browser-based administration UI.

## Frontend workflow

Install frontend build dependencies once:

```bash
cd hiraeth_web
npm ci
```

Build the checked-in stylesheet:

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

- `assets/app.css` is generated and checked into the repo.
- CI rebuilds `assets/app.css` and fails if the committed file is stale.
- App-owned assets like `app.css`, `app.js`, and the favicon use `must-revalidate` so UI changes are picked up quickly during development.
- Vendored assets under `assets/vendor/` are treated as stable and cached aggressively.
