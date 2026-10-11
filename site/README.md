# Website

The project site, published to <https://penso.github.io/herdr-gpui/> by
`.github/workflows/site.yml` whenever `site/` changes on `main`. It is plain
HTML, CSS and JavaScript with no build step and no external requests.

```sh
python3 -m http.server -d site 8000   # then open http://localhost:8000/
```

Opening `site/index.html` directly also works.

## Layout

| File | Purpose |
| --- | --- |
| `index.html` | The page: TUI/window comparison, film, capability rows, screenshots, install |
| `assets/site.css`, `assets/site.js` | Page styles and behaviour |
| `assets/kit.css`, `assets/kit.js` | HTML renderings of the app window and the screenshot list (`HM.SHOTS`) |
| `assets/shots/` | Screenshot captures, one `<id>.webp` per entry in `HM.SHOTS` |
| `assets/film.mp4`, `assets/film-poster.webp` | The showcase film and its poster frame |

Every path is relative, so the site works under any prefix, such as
`/herdr-gpui/` on GitHub Pages or behind a proxy.

## Film and screenshots

The film section and each screenshot appear only once their file exists, so
the page never shows placeholders. Add `?draft` to the URL to see the
placeholders and the shot descriptions while capturing.

- Capture every screenshot from a seeded demo session, never a real one:
  terminal content, repository names, hosts and notes are published.
- Screenshots are 16:10 WebP at 2x, named after their `HM.SHOTS` id (for
  example `assets/shots/teleport.webp`). Each entry's `spec` says what to show.
- The film is an H.264 MP4 with a WebP poster. Its chapters are listed in
  `HM.CHAPTERS`.

## Claims about Herdr

The comparison rows in `assets/site.js` describe what Herdr's own TUI does,
following Herdr's README and documentation. Herdr is an unrelated project:
keep those claims accurate and respectful, update them when Herdr changes, and
keep the unaffiliated notice in the footer.

## Serving at pen.so

To serve the same site at `https://pen.so/herdr-gpui/`, add this rule to the
blog's Netlify `_redirects`, which fetches each page from GitHub Pages:

```text
/herdr-gpui/*  https://penso.github.io/herdr-gpui/:splat  200
```
