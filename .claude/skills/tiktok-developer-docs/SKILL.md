---
name: tiktok-developer-docs
description: Reference lookup for TikTok's developer platform docs (Research API, Data Portability API, Content Posting API, OAuth scopes, request/response schemas, rate limits). Use when a question concerns TikTok's developer-facing APIs and a precise answer requires consulting the scraped documentation in this repo, rather than a general explanation.
---

# TikTok for Developers — Local Documentation Corpus

A snapshot of `developers.tiktok.com/doc/*` lives in this repo as flat files. When a question hinges on the actual TikTok API surface (endpoints, parameters, error codes, scopes, quotas), grep here before answering — TikTok ships breaking changes and your training data will be out of date.

## What's here

`docs/reference/tiktok-for-developers/`
- `markdown/` — 208 `.md` files, one per doc page, named after the page slug (e.g. `doc_research-api-get-started.md`)
- `parsed/` — same 208 pages as JSON, each `{ url, title, fetched_at, slug, markdown, text, links }`
- `index.jsonl` — all 208 records concatenated, one per line. Useful for one-shot `rg` over the entire corpus.

The corpus was scraped on **2026-04-16**. To refresh it, see `tools/tiktok-docs-crawler/`.

## How to look things up

The slug naming is descriptive; start there.

```sh
# List everything related to a topic
ls docs/reference/tiktok-for-developers/markdown/ | rg research-api

# Search content across all pages (cheap; the .text field is plain text)
rg -l 'rate limit' docs/reference/tiktok-for-developers/markdown/

# Pull one page's title quickly
rg -n '"title":' docs/reference/tiktok-for-developers/index.jsonl | head
```

Prefer reading individual `markdown/*.md` files when you need the human-readable doc. Use `index.jsonl` only when you genuinely need a single-pass scan over all pages.

## What this project is doing with these docs

This repo is building a data pipeline where the **Research API** is the middle stage. When in doubt about which subsection is relevant, lean on these:

- `doc_about-research-api.md` — overview
- `doc_research-api-get-started.md` — auth flow, scopes
- `doc_research-api-specs-query-*.md` — per-endpoint request/response specs
- `doc_research-api-codebook.md` — field-level reference
- `doc_research-api-faq.md` — quotas, gotchas

The Data Portability API (`doc_data-portability-*.md`) is the adjacent surface; consult it if the question is about user-side data export rather than researcher-side bulk query.

## When NOT to consult these docs

- General questions about TikTok the product (not the developer platform).
- Questions where the user wants a conceptual explanation, not a precise API reference.
- Questions answerable from this repo's own code without needing TikTok's spec.
