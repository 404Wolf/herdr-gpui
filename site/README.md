# Website

The project site, served at <https://herdrui.dev> by the `herdrui-site`
Cloudflare Worker. `.github/workflows/site.yml` deploys it whenever `site/`
changes on `main`. It is plain
HTML, CSS and JavaScript with no build step. Its one external request is the
GitHub API call in `assets/site.js` that fills in the star count on the GitHub
button; if it fails, the button simply reads "GitHub".

```sh
python3 -m http.server -d site 8000   # then open http://localhost:8000/
```

Opening `site/index.html` directly also works. To run it as Cloudflare serves
it, with the headers from `_headers` and the www redirect:

```sh
npx wrangler@4.144.0 dev --config site/wrangler.jsonc
```

## Layout

| File | Purpose |
| --- | --- |
| `index.html` | The page: TUI/window comparison, film, capability rows, screenshots, install |
| `assets/site.css`, `assets/site.js` | Page styles and behaviour |
| `assets/kit.css`, `assets/kit.js` | HTML renderings of the app window and the screenshot list (`HM.SHOTS`) |
| `assets/shots/` | Screenshot captures, one `<id>.webp` per entry in `HM.SHOTS` |
| `assets/film.mp4`, `assets/film-poster.webp` | The showcase film and its poster frame |
| `wrangler.jsonc`, `worker/index.js` | The Worker: custom domains, static assets, and the www-to-apex redirect |
| `_headers` | Response headers for every page and the cache lifetime of `assets/` |
| `.assetsignore` | Files in `site/` that are never published |

Every path is relative, so the site works under any prefix, such as
`/herdr-gpui/` behind a proxy.

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
blog's Netlify `_redirects`, which fetches each page from herdrui.dev:

```text
/herdr-gpui/*  https://herdrui.dev/:splat  200
```
