use crate::model::{Candidate, ContestFile, ContestProblemFile};
use anyhow::{bail, Result};
use scraper::{Html, Selector};
use std::{fs, path::Path};

pub fn extract_statement(page_html: &str) -> Result<String> {
    let doc = Html::parse_document(page_html);
    let ja_selector = Selector::parse("#task-statement .lang-ja").unwrap();
    let task_selector = Selector::parse("#task-statement").unwrap();
    let html = if let Some(node) = doc.select(&ja_selector).next() {
        node.inner_html()
    } else if let Some(node) = doc.select(&task_selector).next() {
        node.inner_html()
    } else {
        bail!("AtCoder task statement not found; page structure may have changed")
    };
    Ok(normalize_atcoder_urls(&html))
}

pub fn write_contest(
    dir: &Path,
    date: &str,
    selected: &[Candidate],
    statements: &[String],
) -> Result<ContestFile> {
    fs::create_dir_all(dir.join("reveal"))?;
    fs::write(dir.join("style.css"), STYLE)?;

    let mut problems = Vec::new();
    for (i, p) in selected.iter().enumerate() {
        let slot = format!("q{}", i + 1);
        let url = format!("https://atcoder.jp/contests/{}/tasks/{}", p.contest_id, p.id);
        let submit_url = format!("https://atcoder.jp/contests/{}/submit?taskScreenName={}", p.contest_id, p.id);
        problems.push(ContestProblemFile {
            slot: slot.clone(),
            problem_id: p.id.clone(),
            contest_id: p.contest_id.clone(),
            problem_index: p.problem_index.clone(),
            title: p.title.clone(),
            difficulty: p.difficulty,
            raw_difficulty: p.raw_difficulty,
            url,
            submit_url,
        });
    }
    let contest = ContestFile { date: date.to_string(), problems };

    fs::write(dir.join("index.html"), render_index(&contest))?;
    fs::write(dir.join("result.html"), render_result(&contest))?;
    for (i, p) in contest.problems.iter().enumerate() {
        fs::write(dir.join(format!("q{}.html", i + 1)), render_problem_page(&contest, i, &statements[i]))?;
        fs::write(dir.join("reveal").join(format!("q{}.html", i + 1)), render_reveal_page(p))?;
    }
    fs::write(dir.join("contest.json"), serde_json::to_vec_pretty(&contest)?)?;
    Ok(contest)
}

fn render_index(contest: &ContestFile) -> String {
    let buttons = contest.problems.iter().enumerate().map(|(i, _)| {
        format!(r#"<a class="problem-card" href="q{}.html"><span>Q{}</span><small>Open problem</small></a>"#, i + 1, i + 1)
    }).collect::<Vec<_>>().join("\n");
    layout("Daily Contest", "style.css", &format!(r#"
<main class="home">
  <p class="eyebrow">Daily Contest</p>
  <h1>{}</h1>
  <p class="muted">Contest / problem index / difficulty are hidden until reveal.</p>
  <section class="problem-grid">{}</section>
  <a class="button danger" href="result.html">Reveal all problems</a>
</main>"#, html_escape(&contest.date), buttons))
}

fn render_problem_page(contest: &ContestFile, index: usize, statement: &str) -> String {
    let q = index + 1;
    let nav = contest.problems.iter().enumerate().map(|(i, _)| {
        let class = if i == index { "nav-q current" } else { "nav-q" };
        format!(r#"<a class="{}" href="q{}.html">Q{}</a>"#, class, i + 1, i + 1)
    }).collect::<Vec<_>>().join("");
    let body = format!(r#"
<header class="topbar">
  <a class="brand" href="index.html">Daily Contest</a>
  <nav>{}</nav>
</header>
<main class="problem-wrap">
  <div class="problem-heading"><span class="q-badge">Q{}</span></div>
  <article class="statement">{}</article>
  <div class="actions">
    <a class="button danger" href="reveal/q{}.html">Reveal / submit</a>
  </div>
</main>
<script>
document.querySelectorAll('.statement var').forEach((el) => {{
  const span = document.createElement('span');
  span.textContent = '\\(' + el.textContent.trim() + '\\)';
  el.replaceWith(span);
}});
window.MathJax = {{ tex: {{ inlineMath: [['\\(', '\\)']], displayMath: [['\\[', '\\]']] }} }};
</script>
<script defer src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-chtml.js"></script>
"#, nav, q, statement, q);
    layout(&format!("Q{q}"), "style.css", &body)
}

fn render_reveal_page(p: &ContestProblemFile) -> String {
    let body = format!(r#"
<main class="reveal">
  <p class="eyebrow">{}</p>
  <h1>{} {} — {}</h1>
  <dl>
    <div><dt>Difficulty</dt><dd>{}</dd></div>
    <div><dt>Problem ID</dt><dd>{}</dd></div>
  </dl>
  <div class="actions left">
    <a class="button primary" href="{}" target="_blank" rel="noopener noreferrer">Submit on AtCoder</a>
    <a class="button" href="{}" target="_blank" rel="noopener noreferrer">Open problem</a>
    <a class="button" href="../{}.html">Back to {}</a>
  </div>
</main>"#,
        html_escape(&p.slot.to_ascii_uppercase()),
        html_escape(&p.contest_id.to_ascii_uppercase()),
        html_escape(&p.problem_index),
        html_escape(&p.title),
        p.difficulty,
        html_escape(&p.problem_id),
        html_escape(&p.submit_url),
        html_escape(&p.url),
        html_escape(&p.slot),
        html_escape(&p.slot.to_ascii_uppercase()),
    );
    layout(&format!("{} reveal", p.slot.to_ascii_uppercase()), "../style.css", &body)
}

fn render_result(contest: &ContestFile) -> String {
    let rows = contest.problems.iter().map(|p| format!(r#"
<tr>
  <td>{}</td><td>{} {}</td><td>{}</td><td>{}</td>
  <td><a href="{}" target="_blank" rel="noopener noreferrer">Problem</a> · <a href="{}" target="_blank" rel="noopener noreferrer">Submit</a></td>
</tr>"#,
        html_escape(&p.slot.to_ascii_uppercase()),
        html_escape(&p.contest_id.to_ascii_uppercase()),
        html_escape(&p.problem_index),
        html_escape(&p.title),
        p.difficulty,
        html_escape(&p.url),
        html_escape(&p.submit_url),
    )).collect::<Vec<_>>().join("");
    layout("Results", "style.css", &format!(r#"
<main class="results">
  <p class="eyebrow">Daily Contest {}</p>
  <h1>Reveal all</h1>
  <table><thead><tr><th>Slot</th><th>Problem</th><th>Title</th><th>Difficulty</th><th>Link</th></tr></thead><tbody>{}</tbody></table>
  <a class="button" href="index.html">Back</a>
</main>"#, html_escape(&contest.date), rows))
}

fn layout(title: &str, css_path: &str, body: &str) -> String {
    format!(r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{}</title>
<link rel="stylesheet" href="{}">
</head>
<body>{}</body>
</html>"#, html_escape(title), css_path, body)
}

fn normalize_atcoder_urls(input: &str) -> String {
    input
        .replace("href=\"//", "href=\"https://")
        .replace("src=\"//", "src=\"https://")
        .replace("href=\"/", "href=\"https://atcoder.jp/")
        .replace("src=\"/", "src=\"https://atcoder.jp/")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const STYLE: &str = r#"
:root { color-scheme: light; --fg:#202124; --muted:#6b7280; --line:#d8dde3; --soft:#f6f8fa; --link:#0969da; }
* { box-sizing:border-box; }
body { margin:0; color:var(--fg); background:white; font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","Hiragino Kaku Gothic ProN","Yu Gothic",Meiryo,sans-serif; line-height:1.7; }
a { color:var(--link); }
.topbar { position:sticky; top:0; z-index:10; display:flex; gap:24px; align-items:center; justify-content:space-between; padding:12px 24px; border-bottom:1px solid var(--line); background:rgba(255,255,255,.96); backdrop-filter:blur(8px); }
.brand { color:var(--fg); text-decoration:none; font-weight:700; }
.topbar nav { display:flex; gap:6px; }
.nav-q { display:inline-flex; width:38px; height:34px; align-items:center; justify-content:center; border:1px solid var(--line); border-radius:7px; color:var(--fg); text-decoration:none; font-weight:600; }
.nav-q.current { background:var(--fg); color:white; }
.home,.reveal,.results { max-width:1000px; margin:0 auto; padding:56px 24px; }
.problem-wrap { max-width:900px; margin:0 auto; padding:36px 24px 72px; }
.eyebrow { color:var(--muted); text-transform:uppercase; letter-spacing:.08em; font-weight:700; font-size:.82rem; }
h1 { line-height:1.25; }
.muted { color:var(--muted); }
.problem-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin:32px 0; }
.problem-card { display:flex; flex-direction:column; gap:3px; padding:22px; border:1px solid var(--line); border-radius:10px; color:var(--fg); text-decoration:none; }
.problem-card:hover { background:var(--soft); }
.problem-card span { font-size:1.4rem; font-weight:750; }
.problem-card small { color:var(--muted); }
.problem-heading { margin-bottom:18px; }
.q-badge { display:inline-flex; padding:4px 11px; border-radius:999px; background:var(--fg); color:#fff; font-weight:800; }
.statement h2,.statement h3 { margin-top:2em; padding-bottom:.25em; border-bottom:1px solid var(--line); }
.statement pre { padding:14px 16px; background:var(--soft); border:1px solid var(--line); border-radius:6px; overflow:auto; line-height:1.45; }
.statement img { max-width:100%; height:auto; }
.statement table { border-collapse:collapse; max-width:100%; }
.statement th,.statement td { border:1px solid var(--line); padding:6px 10px; }
.actions { display:flex; justify-content:flex-end; gap:10px; margin-top:42px; }
.actions.left { justify-content:flex-start; }
.button { display:inline-block; padding:10px 16px; border:1px solid var(--line); border-radius:7px; color:var(--fg); text-decoration:none; background:white; font-weight:650; }
.button:hover { background:var(--soft); }
.button.primary { background:#1f6feb; border-color:#1f6feb; color:white; }
.button.danger { border-color:#cf222e; color:#cf222e; }
dl { margin:28px 0; max-width:560px; }
dl div { display:grid; grid-template-columns:130px 1fr; padding:10px 0; border-bottom:1px solid var(--line); }
dt { color:var(--muted); } dd { margin:0; font-weight:650; }
table { width:100%; border-collapse:collapse; margin:28px 0; }
th,td { padding:10px; text-align:left; border-bottom:1px solid var(--line); }
@media (max-width:640px) { .topbar { padding:10px 12px; gap:8px; } .topbar nav { overflow-x:auto; } .problem-wrap { padding:24px 14px 56px; } th:nth-child(3),td:nth-child(3) { display:none; } }
"#;


#[cfg(test)]
mod tests {
    use super::extract_statement;

    #[test]
    fn extracts_japanese_statement_and_rewrites_root_urls() {
        let html = r#"
        <html><body>
          <div id="task-statement">
            <span class="lang-ja"><h3>問題文</h3><p><var>N</var></p><img src="/img/a.png"></span>
            <span class="lang-en"><h3>Problem Statement</h3></span>
          </div>
        </body></html>"#;
        let out = extract_statement(html).unwrap();
        assert!(out.contains("問題文"));
        assert!(!out.contains("Problem Statement"));
        assert!(out.contains("https://atcoder.jp/img/a.png"));
    }
}
