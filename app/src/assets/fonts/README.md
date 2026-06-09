# Bundled fonts

These fonts are bundled locally so the app makes **no network requests at runtime**
(privacy-first / local-only). Each is a variable font (`wght` axis), latin subset, in
woff2.

| File | Family | Use in UI | Upstream |
| --- | --- | --- | --- |
| `Inter-variable.woff2` | Inter | body / UI text | https://github.com/rsms/inter |
| `HankenGrotesk-variable.woff2` | Hanken Grotesk | headline (`biasdiff` title) | https://github.com/marcologous/hanken-grotesk |
| `Geist-variable.woff2` | Geist | labels (badge, field labels, panel headers) | https://github.com/vercel/geist-font |
| `GeistMono-variable.woff2` | Geist Mono | monospace (inputs, danger list) | https://github.com/vercel/geist-font |

Japanese text is intentionally **not** bundled — it falls back to the system gothic
(`Hiragino Kaku Gothic ProN` → `Yu Gothic`), so only small latin subsets are shipped.

## License

All four are licensed under the **SIL Open Font License 1.1**. The full text for each is
in this directory:

- `Inter-OFL.txt` — © 2016 The Inter Project Authors
- `HankenGrotesk-OFL.txt` — © 2021 The Hanken Grotesk Project Authors
- `Geist-OFL.txt` — © 2024 The Geist Project Authors
- `GeistMono-OFL.txt` — © 2024 The Geist Project Authors

Files were fetched from [Fontsource](https://fontsource.org/) via
`cdn.jsdelivr.net/fontsource` (variable, `latin-wght-normal`).
