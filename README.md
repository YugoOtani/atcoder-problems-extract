# daily-atcoder

AtCoder Problems のデータを使って、ABC / ARC の未AC・未出題問題から 1 日 5 問程度の匿名ミニコンテストを生成する Rust CLI です。

問題を解いている間は **コンテスト名・問題番号・Difficulty を表示しません**。問題文はローカルWebサーバーからブラウザに表示され、各問題の `Reveal / submit` から正体を開示した後、AtCoder の問題ページまたは提出ページへ移動して普段どおり提出します。

問題ページの `Bookmark` から、正体を開示せずにあとで解き直したい問題を保存できます。ブックマークはデータルートの `bookmarks.json` に保存され、`Bookmarks` 画面から一覧と保存元のHTMLを確認できます。解除は保存元の問題ページで行います。

## Commands

```bash
cargo run -- start
cargo run -- open 2026-09-13
cargo run -- root set /path/to/daily-atcoder
```

リリースビルドして PATH に置く場合:

```bash
cargo build --release
# target/release/daily(.exe)
daily start
daily open 2026-09-13
daily root set /path/to/daily-atcoder
```

`start` と `open` はブラウザを開いた後、ローカルWebサーバーとして動作し続けます。終了するときはターミナルで `Ctrl+C` を押してください。

別ディレクトリをデータルートにする場合:

```bash
daily --root /path/to/daily-atcoder start
# または、起動せずにデータルートだけを保存
daily root set /path/to/daily-atcoder
```

どちらの方法でも指定したパスは `~/.daily-config` に保存されるため、次回からは `--root` を省略できます。
保存先を変更するときは、別のパスを指定して同じコマンドを再実行してください。
`--root` 配下に `config.toml`, `cache/`, `contests/` を置きます。まだ保存設定がない場合は、従来どおりカレントディレクトリをデータルートとして使います。

利用可能なコマンドとオプションはヘルプで確認できます。

```bash
daily help
daily help root
daily help root set
```

## Generated files

```text
contests/
└─ 2026-09-13/
   ├─ index.html
   ├─ q1.html ... q5.html
   ├─ result.html
   ├─ style.css
   ├─ reveal/
   │  └─ q1.html ... q5.html
   └─ contest.json
```

`contest.json` だけが各Qの元問題情報を保持します。匿名問題ページには contest id / problem index / Difficulty を埋め込みません。

ブックマークは日付をまたいで共有されます。

```text
bookmarks.json
```

## Selection defaults

- Difficulty weight: 0–399 = 20, 400–799 = 40, 800–1199 = 28, 1200–1599 = 10, 1600–1999 = 2
- 5問中: `<800` が最低2問、`>=800` が最低1問、`>=1200` は最大2問、`>=1600` は最大1問
- ABC weight 2 / ARC weight 1、かつ各1問以上
- 同一公式コンテストから最大1問
- 同じ bucket の中では古い問題を少し引きやすくする
- 最後にQ1〜Q5をシャッフル

すべて `config.toml` から変更できます。

## Cache / API behavior

AtCoder Problems の静的データは `cache/` に保存し、デフォルト24時間再利用します。提出履歴は初回のみ過去分をページング取得し、以後は最新キャッシュ以降を増分取得します。

AtCoder Problems API は非公式で、連続アクセスを避けるよう案内されています。そのためAPIページング間には1秒強の間隔を置きます。

## Notes

- 問題文は AtCoder の `#task-statement .lang-ja` を優先して抽出します。AtCoder 側のDOMが大きく変わった場合は `src/html.rs` の selector が主なメンテ箇所です。
- `<var>` 内のLaTeXは生成HTML側でMathJaxに渡します。
- 画像はローカル保存せず AtCoder 上のURLを参照します。
- Submit機能・ログイン・コード実行・ジャッジ監視は持ちません。
