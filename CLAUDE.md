# CLAUDE.md

このディレクトリはコア実装用の独立 Rust workspace。

## 行動原則

- ユーザーの指示を最優先する
- 指示された範囲だけを実行し、余計な調査や作業を追加しない
- 明確な指示には確認せず即実行する
- 作業中に問題が起きた場合のみ端的に報告する

## Repository Writing Policy

- Use English for all repository-facing written materials.
- This includes PR titles and descriptions, issue titles and descriptions, commit messages, review comments, release notes, changelogs, documentation, comments added to code, and other project text.
- User-facing conversation may follow the user's language, but any text written into the repository or GitHub artifacts must be English.

## Build & Test Commands

```bash
cargo fmt --check
cargo check
cargo test
cargo test -p crustal-markdown
```

## Workspace Members

| Crate | Type | 役割 |
|-------|------|------|
| **crustal-markdown** | lib | Markdown→HTML 変換。lexer, tokenizer, generator で処理 |
| **crustal-macros** | proc-macro | `render!` マクロ（SSR/Client デュアルモード）と `#[component]` マクロ |
| **crustal-wasm** | rlib | Signal, Bindable, Router などのクライアントサイドライブラリ |
| **crustal-blog-utils** | lib | Markdown post 読み取り、sort、静的ファイルコピーなどのブログ生成補助 |

## Architecture Notes

- `render!` は target ごとに生成コードを切り替える
  - native target: SSR 用に `String` を組み立てる
  - wasm32 target: Client 用に `web_sys` で DOM 要素を生成する
- `crustal-wasm` は pure library（`rlib`）。`#[wasm_bindgen(start)]` は持たない
- アプリケーション固有の処理は利用側の workspace に置く
- blog app からは git dependency として参照される

## コーディング

- `cargo fmt` を必ずコミット前に実行する
- `cargo test` でテストが通ることを確認してからコミットする
- 不要な未追跡ファイルはコミット前に削除する

## Memory

- Claude の memory 機能（`~/.claude/projects/.../memory/`）は使わない
- アーキテクチャの知見や注意点はこの `CLAUDE.md` に直接書く
