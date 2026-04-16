import { chromium } from 'playwright';
import * as fs from 'fs/promises';
import * as path from 'path';
import TurndownService from 'turndown';
import { JSDOM } from 'jsdom';

const START_URL = 'https://developers.tiktok.com/doc/overview';
const ALLOWED_PREFIX = 'https://developers.tiktok.com/doc/';
const OUTPUT_DIR = path.resolve(__dirname, '../../docs/reference/tiktok-for-developers');
const PARSED_DIR = path.join(OUTPUT_DIR, 'parsed');
const MARKDOWN_DIR = path.join(OUTPUT_DIR, 'markdown');
const INDEX_PATH = path.join(OUTPUT_DIR, 'index.jsonl');
const MAX_PAGES = 1000;
const REQUEST_DELAY_MS = 1000;
const NAV_TIMEOUT_MS = 45000;

const turndown = new TurndownService({
  headingStyle: 'atx',
  codeBlockStyle: 'fenced',
});

type PageRecord = {
  url: string;
  title: string;
  fetched_at: string;
  slug: string;
  markdown: string;
  text: string;
  links: string[];
};

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function sanitizeFilename(input: string): string {
  return input
    .replace(/^https?:\/\//, '')
    .replace(/[^a-zA-Z0-9._-]+/g, '_')
    .replace(/^_+|_+$/g, '');
}

function urlToSlug(url: string): string {
  const u = new URL(url);
  const pathname = u.pathname.replace(/^\/+/, '').replace(/\/+$/, '');
  const base = pathname.length ? pathname : 'index';
  const suffix = u.search ? '_' + sanitizeFilename(u.search) : '';
  return sanitizeFilename(base + suffix);
}

function normalizeDocUrl(href: string, base: string): string | null {
  try {
    const url = new URL(href, base);
    url.hash = '';
    url.searchParams.delete('enter_method');

    if (url.origin !== 'https://developers.tiktok.com') {
      return null;
    }

    if (!url.href.startsWith(ALLOWED_PREFIX)) {
      return null;
    }

    return url.toString();
  } catch {
    return null;
  }
}

function extractMainHtml(document: Document): string {
  const candidates = [
    'main',
    'article',
    '[role="main"]',
    '.theme-doc-markdown',
    '.markdown',
    '.content',
    '.doc-content',
    '.DocSearch-content',
  ];

  for (const selector of candidates) {
    const el = document.querySelector(selector);
    if (el && el.innerHTML.trim().length > 0) {
      return el.innerHTML;
    }
  }

  return document.body?.innerHTML ?? '';
}

function extractLinks(document: Document, baseUrl: string): string[] {
  const seen = new Set<string>();
  const urls: string[] = [];

  for (const a of Array.from(document.querySelectorAll('a[href]'))) {
    const href = a.getAttribute('href');
    if (!href) continue;

    const normalized = normalizeDocUrl(href, baseUrl);
    if (!normalized) continue;
    if (seen.has(normalized)) continue;

    seen.add(normalized);
    urls.push(normalized);
  }

  urls.sort();
  return urls;
}

function htmlToText(html: string): string {
  const dom = new JSDOM(`<body>${html}</body>`);
  return dom.window.document.body.textContent?.replace(/\s+/g, ' ').trim() ?? '';
}

async function ensureDirs(): Promise<void> {
  await fs.mkdir(PARSED_DIR, { recursive: true });
  await fs.mkdir(MARKDOWN_DIR, { recursive: true });
}

async function appendJsonl(record: PageRecord): Promise<void> {
  await fs.appendFile(INDEX_PATH, JSON.stringify(record) + '\n', 'utf8');
}

async function writeRecord(record: PageRecord): Promise<void> {
  await fs.writeFile(path.join(PARSED_DIR, `${record.slug}.json`), JSON.stringify(record, null, 2), 'utf8');
  await fs.writeFile(path.join(MARKDOWN_DIR, `${record.slug}.md`), record.markdown, 'utf8');
  await appendJsonl(record);
}

async function main(): Promise<void> {
  await ensureDirs();
  await fs.writeFile(INDEX_PATH, '', 'utf8');

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    userAgent: 'Mozilla/5.0 (compatible; TikTokDocsCrawler/1.0; +local-use)',
  });
  const page = await context.newPage();
  page.setDefaultNavigationTimeout(NAV_TIMEOUT_MS);

  const queue: string[] = [START_URL];
  const seen = new Set<string>();
  let count = 0;

  while (queue.length > 0 && count < MAX_PAGES) {
    const url = queue.shift()!;
    if (seen.has(url)) continue;
    seen.add(url);

    console.log(`Fetching ${count + 1}: ${url}`);

    try {
      await page.goto(url, { waitUntil: 'networkidle' });
      await page.waitForTimeout(1000);

      const fullHtml = await page.content();
      const parsed = new JSDOM(fullHtml);
      const document = parsed.window.document;

      const mainHtml = extractMainHtml(document);
      const markdown = turndown.turndown(mainHtml);
      const text = htmlToText(mainHtml);
      const links = extractLinks(document, url);
      const title = document.title?.trim() || url;
      const slug = urlToSlug(url);

      const record: PageRecord = {
        url,
        title,
        fetched_at: new Date().toISOString(),
        slug,
        markdown,
        text,
        links,
      };

      await writeRecord(record);
      count += 1;

      for (const link of links) {
        if (!seen.has(link)) {
          queue.push(link);
        }
      }
    } catch (error) {
      console.error(`Failed to fetch ${url}:`, error);
    }

    await sleep(REQUEST_DELAY_MS);
  }

  await browser.close();
  console.log(`Done. Saved ${count} pages to ${OUTPUT_DIR}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});

