use crate::{
    bookmark::Bookmark,
    model::{Candidate, ContestFile, ContestProblemFile},
};
use anyhow::{bail, Result};
use chrono::{DateTime, FixedOffset};
use scraper::{Html, Selector};
use std::{fs, path::Path};

pub fn extract_statement(page_html: &str) -> Result<String> {
    let doc = Html::parse_document(page_html);
    let ja_selector = Selector::parse("#task-statement .lang-ja").unwrap();
    let task_selector = Selector::parse("#task-statement").unwrap();
    let statement = if let Some(node) = doc.select(&ja_selector).next() {
        node
    } else if let Some(node) = doc.select(&task_selector).next() {
        node
    } else {
        bail!("AtCoder task statement not found; page structure may have changed")
    };

    let mut html = statement.inner_html();
    // AtCoder places the score in a direct <p> immediately before the first .part.
    let mut children = statement.child_elements();
    if let (Some(score), Some(body)) = (children.next(), children.next()) {
        let has_score_structure = score.value().name() == "p"
            && body.value().name() == "div"
            && body
                .attr("class")
                .is_some_and(|classes| classes.split_whitespace().any(|class| class == "part"));
        if has_score_structure {
            html = html.replacen(&score.html(), "", 1);
        }
    }

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
        fs::write(
            dir.join("reveal").join(format!("q{}.html", i + 1)),
            render_reveal_page(&contest.date, p),
        )?;
    }
    fs::write(dir.join("contest.json"), serde_json::to_vec_pretty(&contest)?)?;
    Ok(contest)
}

fn render_index(contest: &ContestFile) -> String {
    let buttons = contest.problems.iter().enumerate().map(|(i, _)| {
        let slot = format!("q{}", i + 1);
        format!(r#"<a class="problem-card" href="{}.html"><span>Q{}</span><small>Open problem</small><small class="bookmark-indicator" data-date="{}" data-slot="{}" hidden>★ Bookmarked</small></a>"#,
            slot,
            i + 1,
            html_escape(&contest.date),
            slot,
        )
    }).collect::<Vec<_>>().join("\n");
    layout("Daily Contest", "style.css", &format!(r#"
<main class="home">
  <p class="eyebrow">Daily Contest</p>
  <h1>{}</h1>
  <p class="muted">Contest / problem index / difficulty are hidden until reveal.</p>
  <section class="problem-grid">{}</section>
  <div class="actions left">
    <a class="button danger" href="result.html">Reveal all problems</a>
    <a class="button bookmarks-link" href="/bookmarks">Bookmarks</a>
  </div>
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
  <div class="topbar-actions"><nav>{}</nav><a class="button compact bookmarks-link" href="/bookmarks">Bookmarks</a></div>
</header>
<main class="problem-wrap">
  <div class="problem-heading"><span class="q-badge">Q{}</span>{}</div>
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
window.MathJax = {{
  tex: {{ inlineMath: [['\\(', '\\)']], displayMath: [['\\[', '\\]']] }},
  options: {{
    // MathJax skips pre elements by default, but AtCoder uses them for input formats.
    skipHtmlTags: {{'[-]': ['pre']}}
  }}
}};
</script>
<script defer src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-chtml.js"></script>
"#, nav, q, bookmark_control(&contest.date, &format!("q{q}"), false), statement, q);
    layout(&format!("Q{q}"), "style.css", &body)
}

fn render_reveal_page(date: &str, p: &ContestProblemFile) -> String {
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
    {}
    <a class="button" href="../{}.html">Back to {}</a>
    <a class="button bookmarks-link" href="/bookmarks">Bookmarks</a>
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
        bookmark_control(date, &p.slot, false),
        html_escape(&p.slot),
        html_escape(&p.slot.to_ascii_uppercase()),
    );
    layout(&format!("{} reveal", p.slot.to_ascii_uppercase()), "../style.css", &body)
}

fn render_result(contest: &ContestFile) -> String {
    let rows = contest.problems.iter().map(|p| format!(r#"
<tr>
  <td>{}</td><td>{} {}</td><td>{}</td><td>{}</td>
  <td><div class="result-links"><span><a href="{}" target="_blank" rel="noopener noreferrer">Problem</a> · <a href="{}" target="_blank" rel="noopener noreferrer">Submit</a></span>{}</div></td>
</tr>"#,
        html_escape(&p.slot.to_ascii_uppercase()),
        html_escape(&p.contest_id.to_ascii_uppercase()),
        html_escape(&p.problem_index),
        html_escape(&p.title),
        p.difficulty,
        html_escape(&p.url),
        html_escape(&p.submit_url),
        bookmark_control(&contest.date, &p.slot, false),
    )).collect::<Vec<_>>().join("");
    layout("Results", "style.css", &format!(r#"
<main class="results">
  <p class="eyebrow">Daily Contest {}</p>
  <h1>Reveal all</h1>
  <table><thead><tr><th>Slot</th><th>Problem</th><th>Title</th><th>Difficulty</th><th>Link</th></tr></thead><tbody>{}</tbody></table>
  <div class="actions left"><a class="button" href="index.html">Back</a><a class="button bookmarks-link" href="/bookmarks">Bookmarks</a></div>
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
<link rel="stylesheet" href="/bookmark.css">
<script defer src="/bookmark.js"></script>
</head>
<body>{}</body>
</html>"#, html_escape(title), css_path, body)
}

fn bookmark_control(date: &str, slot: &str, bookmarked: bool) -> String {
    let state = if bookmarked { "true" } else { "false" };
    format!(
        r#"<span class="bookmark-control"><button type="button" class="button bookmark-button" data-date="{}" data-slot="{}" data-bookmarked="{}" aria-pressed="{}">{}</button><span class="bookmark-message" role="status" aria-live="polite"></span></span>"#,
        html_escape(date),
        html_escape(slot),
        state,
        state,
        if bookmarked { "★ Bookmarked" } else { "☆ Bookmark" },
    )
}

pub fn render_bookmarks_page(bookmarks: &[Bookmark], active_date: &str) -> String {
    let content = if bookmarks.is_empty() {
        r#"<p class="empty-state">No bookmarked problems yet.</p>"#.to_owned()
    } else {
        let rows = bookmarks
            .iter()
            .map(|bookmark| {
                format!(
                    r#"<tr>
  <td>{} {}</td>
  <td>{}</td>
  <td>{}</td>
  <td>{}</td>
  <td><div class="result-links"><span><a href="/{}/{}.html">HTML</a> · <a href="{}" target="_blank" rel="noopener noreferrer">Problem</a> · <a href="{}" target="_blank" rel="noopener noreferrer">Submit</a></span><button type="button" class="button bookmark-button bookmark-status" data-bookmarked="true" aria-pressed="true" disabled>★ Bookmarked</button></div></td>
</tr>"#,
                    html_escape(&bookmark.contest_id.to_ascii_uppercase()),
                    html_escape(&bookmark.problem_index),
                    html_escape(&bookmark.title),
                    bookmark.difficulty,
                    html_escape(&format_bookmarked_at(&bookmark.bookmarked_at)),
                    html_escape(&bookmark.source_date),
                    html_escape(&bookmark.source_slot),
                    html_escape(&bookmark.url),
                    html_escape(&bookmark.submit_url),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"<table><thead><tr><th>Problem</th><th>Title</th><th>Difficulty</th><th>Bookmarked</th><th>Links</th></tr></thead><tbody>{rows}</tbody></table>"#
        )
    };

    layout(
        "Bookmarks",
        &format!("/{}/style.css", html_escape(active_date)),
        &format!(
            r#"
<main class="results bookmarks-page">
  <p class="eyebrow">Daily AtCoder</p>
  <h1>Bookmarks</h1>
  {}
  <div class="actions left"><a class="button" href="/{}/">Back to daily contest</a></div>
</main>"#,
            content,
            html_escape(active_date),
        ),
    )
}

fn format_bookmarked_at(input: &str) -> String {
    let Ok(timestamp) = DateTime::parse_from_rfc3339(input) else {
        return input.to_owned();
    };
    let jst = FixedOffset::east_opt(9 * 3600).expect("valid JST offset");
    timestamp
        .with_timezone(&jst)
        .format("%Y-%m-%d %H:%M JST")
        .to_string()
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

pub const BOOKMARK_STYLE: &str = r#"
.topbar-actions { display:flex; align-items:center; gap:12px; min-width:0; }
.button.compact { padding:5px 10px; font-size:.9rem; }
.problem-heading { display:flex; align-items:center; justify-content:space-between; gap:16px; }
.bookmark-control { display:inline-flex; flex-direction:column; align-items:flex-start; gap:2px; }
.bookmark-button { cursor:pointer; white-space:nowrap; }
.bookmark-button[aria-pressed="true"] { border-color:#bf8700; background:#fff8c5; color:#633c01; }
.bookmark-button:disabled { cursor:wait; opacity:.65; }
.bookmark-status:disabled { cursor:default; opacity:1; }
.bookmark-message { min-height:1.2em; color:#cf222e; font-size:.78rem; line-height:1.2; }
.bookmark-indicator { color:#9a6700 !important; font-weight:700; }
.result-links { display:flex; flex-direction:column; align-items:flex-start; gap:8px; }
.empty-state { margin:28px 0; padding:24px; border:1px dashed var(--line); border-radius:8px; color:var(--muted); }
.bookmarks-page .actions { margin-top:28px; }
.actions { flex-wrap:wrap; }
@media (max-width:640px) {
  .topbar-actions { gap:6px; overflow-x:auto; }
  .button.compact { display:none; }
  .problem-heading { align-items:flex-start; }
  .bookmarks-page th:nth-child(2),.bookmarks-page td:nth-child(2),
  .bookmarks-page th:nth-child(4),.bookmarks-page td:nth-child(4) { display:none; }
}
"#;

pub const BOOKMARK_SCRIPT: &str = r#"
(() => {
  'use strict';

  const requestHeaders = { 'X-Daily-Atcoder': '1' };

  function pageProblem() {
    const match = location.pathname.match(/^\/(\d{4}-\d{2}-\d{2})\/(?:reveal\/)?(q\d+)\.html$/);
    return match ? { date: match[1], slot: match[2] } : null;
  }

  function createControl(date, slot) {
    const wrapper = document.createElement('span');
    wrapper.className = 'bookmark-control';
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'button bookmark-button';
    button.dataset.date = date;
    button.dataset.slot = slot;
    button.dataset.bookmarked = 'false';
    button.setAttribute('aria-pressed', 'false');
    button.textContent = '☆ Bookmark';
    const message = document.createElement('span');
    message.className = 'bookmark-message';
    message.setAttribute('role', 'status');
    message.setAttribute('aria-live', 'polite');
    wrapper.append(button, message);
    return wrapper;
  }

  function ensureLegacyControls() {
    const problem = pageProblem();
    if (problem && !document.querySelector('.bookmark-button')) {
      const target = location.pathname.includes('/reveal/')
        ? document.querySelector('.actions.left')
        : document.querySelector('.problem-heading');
      if (target) target.append(createControl(problem.date, problem.slot));
    }

    const result = location.pathname.match(/^\/(\d{4}-\d{2}-\d{2})\/result\.html$/);
    if (result && !document.querySelector('.bookmark-button')) {
      document.querySelectorAll('tbody tr').forEach((row) => {
        const slot = row.cells[0]?.textContent.trim().toLowerCase();
        const target = row.cells[row.cells.length - 1];
        if (slot && target) target.append(createControl(result[1], slot));
      });
    }

    const index = location.pathname.match(/^\/(\d{4}-\d{2}-\d{2})\/(?:index\.html)?$/);
    if (index && !document.querySelector('.bookmark-indicator')) {
      document.querySelectorAll('.problem-card').forEach((card) => {
        const match = card.getAttribute('href')?.match(/^(q\d+)\.html$/);
        if (!match) return;
        const indicator = document.createElement('small');
        indicator.className = 'bookmark-indicator';
        indicator.dataset.date = index[1];
        indicator.dataset.slot = match[1];
        indicator.hidden = true;
        indicator.textContent = '★ Bookmarked';
        card.append(indicator);
      });
    }

    if (!/^\/bookmarks\/?$/.test(location.pathname) && !document.querySelector('.bookmarks-link')) {
      const link = document.createElement('a');
      link.className = 'button bookmarks-link';
      link.href = '/bookmarks';
      link.textContent = 'Bookmarks';
      const target = document.querySelector('.topbar') || document.querySelector('.actions.left') || document.querySelector('.home');
      if (target) target.append(link);
    }
  }

  function endpoint(button) {
    return '/api/bookmarks/' + encodeURIComponent(button.dataset.date) + '/' + encodeURIComponent(button.dataset.slot);
  }

  function renderButton(button, bookmarked) {
    button.dataset.bookmarked = String(bookmarked);
    button.setAttribute('aria-pressed', String(bookmarked));
    button.textContent = bookmarked ? '★ Bookmarked' : '☆ Bookmark';
  }

  function messageFor(button, text) {
    const message = button.parentElement?.querySelector('.bookmark-message');
    if (message) message.textContent = text;
  }

  async function readState(button) {
    button.disabled = true;
    try {
      const response = await fetch(endpoint(button), { headers: requestHeaders, cache: 'no-store' });
      if (!response.ok) throw new Error('request failed');
      const data = await response.json();
      renderButton(button, data.bookmarked === true);
    } catch (_) {
      messageFor(button, 'Could not load bookmark.');
    } finally {
      button.disabled = false;
    }
  }

  async function toggle(button) {
    const wasBookmarked = button.dataset.bookmarked === 'true';
    button.disabled = true;
    messageFor(button, '');
    try {
      const response = await fetch(endpoint(button), {
        method: wasBookmarked ? 'DELETE' : 'POST',
        headers: requestHeaders,
      });
      if (!response.ok) throw new Error('request failed');
      const data = await response.json();
      renderButton(button, data.bookmarked === true);
    } catch (_) {
      renderButton(button, wasBookmarked);
      messageFor(button, 'Could not update bookmark.');
    } finally {
      button.disabled = false;
    }
  }

  async function refreshIndicator(indicator) {
    try {
      const url = '/api/bookmarks/' + encodeURIComponent(indicator.dataset.date) + '/' + encodeURIComponent(indicator.dataset.slot);
      const response = await fetch(url, { headers: requestHeaders, cache: 'no-store' });
      if (!response.ok) return;
      const data = await response.json();
      indicator.hidden = data.bookmarked !== true;
    } catch (_) {
      // The indicator is optional; leave it hidden when status cannot be loaded.
    }
  }

  document.addEventListener('DOMContentLoaded', () => {
    ensureLegacyControls();
    document.querySelectorAll('.bookmark-button').forEach((button) => {
      if (button.disabled) return;
      button.addEventListener('click', () => toggle(button));
      readState(button);
    });
    document.querySelectorAll('.bookmark-indicator').forEach(refreshIndicator);
  });
})();
"#;


#[cfg(test)]
mod tests {
    use super::{extract_statement, render_bookmarks_page, render_problem_page};
    use crate::{bookmark::Bookmark, model::ContestFile};

    #[test]
    fn extracts_japanese_statement_and_rewrites_root_urls() {
        let html = r#"
        <html><body>
          <div id="task-statement">
            <span class="lang-ja">
              <p><var>400</var></p>
              <div class="part">
                <h3>問題文</h3>
                <p>この段落は残す。</p>
                <img src="/img/a.png">
              </div>
            </span>
            <span class="lang-en"><h3>Problem Statement</h3></span>
          </div>
        </body></html>"#;
        let out = extract_statement(html).unwrap();
        assert!(out.contains("問題文"));
        assert!(!out.contains("Problem Statement"));
        assert!(!out.contains("<p><var>400</var></p>"));
        assert!(out.contains("この段落は残す"));
        assert!(out.contains("https://atcoder.jp/img/a.png"));
    }

    #[test]
    fn mathjax_processes_formulas_inside_input_format_pre_elements() {
        let contest = ContestFile {
            date: "2026-09-13".into(),
            problems: Vec::new(),
        };
        let page = render_problem_page(&contest, 0, "<pre><var>N</var></pre>");

        assert!(page.contains("skipHtmlTags: {'[-]': ['pre']}"));
        assert!(page.contains(r#"data-date="2026-09-13" data-slot="q1""#));
        assert!(!page.contains("data-problem-id"));
    }

    #[test]
    fn bookmarks_page_links_to_local_html_without_an_active_toggle() {
        let bookmark = Bookmark {
            problem_id: "abc123_d".into(),
            contest_id: "abc123".into(),
            problem_index: "D".into(),
            title: "Example".into(),
            difficulty: 1200,
            url: "https://example.com/problem".into(),
            submit_url: "https://example.com/submit".into(),
            bookmarked_at: "2026-09-20T00:00:00Z".into(),
            source_date: "2026-09-20".into(),
            source_slot: "q1".into(),
        };

        let page = render_bookmarks_page(&[bookmark], "2026-09-20");

        assert!(page.contains(r#"href="/2026-09-20/q1.html">HTML</a>"#));
        assert!(page.contains(r#"class="button bookmark-button bookmark-status""#));
        assert!(page.contains("disabled>★ Bookmarked</button>"));
        assert!(!page.contains("data-problem-id"));
        assert!(!page.contains("data-remove-row"));
        assert!(!page.contains(r#"class="button bookmarks-link""#));
    }
}
