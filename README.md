# Crustal

Crustal is a Rust workspace for experimenting with Rust-first UI rendering and
browser-side interactivity. Its main pieces are procedural macros for writing
markup and scoped styles, plus a small WebAssembly-side runtime library.
It also includes small companion crates: a Markdown parser and filesystem
helpers used by consuming applications, and a small LSP server for Crustal
macro diagnostics and hover information.

This repository is intended to be consumed by an application workspace, for
example as a Git dependency. Application-specific entry points, build process,
server integration, and deployment configuration live outside this workspace.

## Workspace Crates

| Crate | Type | Purpose |
| --- | --- | --- |
| `crustal-macros` | Proc macro | Provides `render!`, `css!`, `global_css!`, and `#[component]`. |
| `crustal-wasm` | Library | Provides client-side primitives such as signals, bindable text, routing, and caches. |
| `crustal-markdown` | Library | Markdown parser and HTML generator. |
| `crustal-blog-utils` | Library | Filesystem helpers for loading Markdown files and copying related assets. |
| `crustal-lsp` | Binary | Stdio LSP server for Crustal macro diagnostics and hover information. |

## Crate Details

### `crustal-macros`

`crustal-macros` provides rendering and CSS macros.

`render!` generates different code depending on the compilation target:

- Native targets build an SSR `String`.
- `wasm32` targets build DOM nodes with `web_sys`.

The syntax is JSX-like, but Rust expressions can be embedded with braces. This
lets application code compose markup while keeping routing paths, Markdown
output, and derived strings in normal Rust values.

```rust
use crustal_macros::render;

fn nav_item() {
    let label = "Guide";
    let href = "/guide";

    let _html = render! {
        <nav>
            <a class="nav-link" href={href}>{label}</a>
        </nav>
    };
}
```

On native targets, the rendered value can be converted to a string:

```rust
use crustal_macros::render;

fn panel_html() -> String {
    let title = "Hello";

    render!(<section class="panel">{title}</section>).to_string()
}

assert_eq!(panel_html(), "<section class=\"panel\">Hello</section>");
```

Expressions can also provide already-rendered HTML, which is useful when
combining the Markdown crate with the render macro:

```rust
use crustal_macros::render;
use crustal_markdown::markdown_to_html;

fn markdown_page() {
    let body = markdown_to_html("# hello world");

    let _page = render! {
        <main>
            {body}
        </main>
    };
}
```

`css!` generates a stable hashed class name and injects scoped CSS. It supports
top-level declarations, nested selectors using `&`, and `@media` blocks.

The macro returns the generated class name, so it can be passed directly into
`render!`:

```rust
use crustal_macros::{css, render};

fn styled_panel() {
    let panel = css!("
        display: grid;
        gap: 0.5rem;
        color: #222;

        &:hover {
            color: #0057d9;
        }

        & a {
            text-decoration: none;
        }

        @media (max-width: 768px) {
            gap: 0.75rem;
        }
    ");

    let title = "Example";
    let _html = render! {
        <section class={panel}>
            <h1>{title}</h1>
        </section>
    };
}
```

The same macro can also be written with token-style input:

```rust
use crustal_macros::css;

fn button_class() -> &'static str {
    css! {
        padding: 0.5rem 0.75rem;
        &:hover { opacity: 0.8; }
    }
}
```

String-literal input preserves selector spacing more directly, so it is usually
the clearer option for nested selectors and larger style blocks.

`global_css!` injects global CSS without wrapping it in a generated class.

On native targets, `css!` and `global_css!` call `crate::style::push`, so the
consuming application must provide that style collection API.

`#[component]` transforms component functions for use with the rendering macros.

### `crustal-wasm`

`crustal-wasm` is an `rlib` and does not define a `#[wasm_bindgen(start)]`
entry point. Consumers are expected to provide their own application startup.

The crate re-exports:

- `Signal` and `Derived` reactive values
- `Bindable` text binding support
- `Router` for client-side navigation
- `PageCache` and `ScrollCache`

The router intercepts same-origin path links, uses the History API, can prefetch
pages on hover when a page cache is configured, and can preserve scroll positions
through `ScrollCache`.

### `crustal-markdown`

`crustal-markdown` is a Markdown parser and HTML generator. It exposes a
`markdown_to_html` function backed by lexer, tokenizer, and generator modules.

Supported token types include:

- Headings
- Paragraphs
- Unordered and ordered list items
- Bold and italic text
- Links and images
- Inline code and fenced code blocks
- Block quotes
- Tables
- Line breaks
- Custom link cards
- Twitter placeholders

It also includes a few small helper functions:

- `get_frontmatter(input)` extracts recognized frontmatter fields.
- `remove_frontmatter(input)` returns the Markdown body without frontmatter.
- `html_to_string(html)` strips tags and selected Markdown markers for plain text descriptions.

The recognized frontmatter format is:

```markdown
---
id: 1
title: Example title
description: Example description
created_at: 2026-05-03
---

Content starts here.
```

### `crustal-blog-utils`

`crustal-blog-utils` contains filesystem helpers for applications that store
content as Markdown files:

- `list_markdown_files(posts_dir)` lists Markdown files in a posts directory.
- `read_post(posts_dir, md_file)` reads a single post and parses frontmatter.
- `read_posts(posts_dir)` reads valid posts and sorts them by numeric `id` in descending order.
- `copy_files_with_extension(src_dir, dest_dir, extension)` copies matching assets.
- `output_path(path)` builds a path relative to the current directory.

Posts without the required `id`, `title`, or `created_at` fields are skipped by
`read_post`. `description` is optional and defaults to an empty string.

### `crustal-lsp`

`crustal-lsp` is a synchronous stdio Language Server Protocol server for Rust
source files that contain Crustal macros. It currently supports diagnostics and
hover information for:

- `render!`
- `css!`
- `global_css!`

The server uses source-oriented scanning so diagnostics and hover ranges can
point back to byte-accurate locations in the editor. It does not implement
completion, formatting, code actions, semantic tokens, or go-to-definition.

Run it from the workspace root:

```bash
cargo run -p crustal-lsp
```

## Development

Run the standard checks from the workspace root:

```bash
cargo fmt --check
cargo check
cargo test
```

To test only the Markdown crate:

```bash
cargo test -p crustal-markdown
```

## Repository Layout

```text
.
|-- Cargo.toml
|-- crustal-blog-utils/
|-- crustal-lsp/
|-- crustal-macros/
|-- crustal-markdown/
`-- crustal-wasm/
```
