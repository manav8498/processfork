# landing/

Single-page static landing site, served at <https://processfork.dev>
via GitHub Pages from the `landing/` directory.

Stack: HTML + Tailwind CDN (zero build step). Total payload ≈ 8 KB
HTML + 80 KB Tailwind JIT.

## Local preview

```bash
cd landing
python3 -m http.server 8080
open http://localhost:8080/
```

## Deploy

Configure GitHub Pages → source: `main` branch, folder `/landing`.
The `landing/CNAME` file (operator-supplied) sets the custom domain.

## Editing

The HTML is hand-written; edit `index.html` directly. Two small
"components" (`Card`, `Layer`) are rendered by the inline `<script>`
at the bottom, so the markup reads declaratively.
