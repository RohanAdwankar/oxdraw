use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tiny_skia::{Pixmap, Transform};

const EXPECTED_DIR: &str = "tests/expected";
const PAIRS_PER_PAGE: usize = 4;
const PAGE_WIDTH: u32 = 2240;
const HEADER_HEIGHT: u32 = 80;
const ROW_HEIGHT: u32 = 940;
const CELL_WIDTH: u32 = 1100;
const IMAGE_WIDTH: u32 = 1000;
const IMAGE_HEIGHT: u32 = 850;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let revision = args
        .next()
        .context("usage: cargo run --bin view_visual_diff -- <commit>")?;
    if args.next().is_some() {
        bail!("expected one commit hash");
    }

    let repo = PathBuf::from(git(None, &["rev-parse", "--show-toplevel"])?);
    let commit = git(
        Some(&repo),
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    let parent = git(Some(&repo), &["rev-parse", &format!("{commit}^")])?;
    let changed = git(
        Some(&repo),
        &["diff", "--name-only", &parent, &commit, "--", EXPECTED_DIR],
    )?;
    let paths: Vec<_> = changed
        .lines()
        .filter(|path| path.ends_with(".svg"))
        .map(str::to_owned)
        .collect();
    if paths.is_empty() {
        bail!("commit {revision} changes no SVG goldens under {EXPECTED_DIR}");
    }

    let output_dir = env::temp_dir().join(format!("oxdraw-visual-diff-{}", &commit[..12]));
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir)?;
    }
    let source_dir = output_dir.join("source");
    fs::create_dir_all(&source_dir)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();

    let mut pairs = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let before = git_file(&repo, &parent, &path)?.unwrap_or_else(|| placeholder("not present"));
        let after = git_file(&repo, &commit, &path)?.unwrap_or_else(|| placeholder("not present"));
        let name = path
            .trim_start_matches(&format!("{EXPECTED_DIR}/"))
            .trim_end_matches(".svg")
            .replace("/", "__");
        fs::write(
            source_dir.join(format!("{:02}_{name}_before.svg", index + 1)),
            &before,
        )?;
        fs::write(
            source_dir.join(format!("{:02}_{name}_after.svg", index + 1)),
            &after,
        )?;
        pairs.push((
            path,
            render_png(&before, &options)?,
            render_png(&after, &options)?,
        ));
    }

    let page_count = pairs.len().div_ceil(PAIRS_PER_PAGE);
    let mut pages = Vec::new();
    for (page, chunk) in pairs.chunks(PAIRS_PER_PAGE).enumerate() {
        let svg = contact_sheet(chunk, &parent[..12], &commit[..12], page + 1, page_count);
        let stem = if page_count == 1 {
            "visual-diff".to_string()
        } else {
            format!("visual-diff-{:02}", page + 1)
        };
        fs::write(output_dir.join(format!("{stem}.svg")), &svg)?;
        let png = output_dir.join(format!("{stem}.png"));
        fs::write(&png, render_png(&svg, &options)?)?;
        pages.push(png);
    }

    println!("visual diff: {}", output_dir.display());
    for page in pages {
        println!("  {}", page.display());
    }
    Ok(())
}

fn git(repo: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    if let Some(repo) = repo {
        command.current_dir(repo);
    }
    let output = command.args(args).output()?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_file(repo: &Path, revision: &str, path: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["show", &format!("{revision}:{path}")])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(output.stdout)?))
}

fn contact_sheet(
    pairs: &[(String, Vec<u8>, Vec<u8>)],
    parent: &str,
    commit: &str,
    page: usize,
    page_count: usize,
) -> String {
    let height = HEADER_HEIGHT + ROW_HEIGHT * pairs.len() as u32;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{PAGE_WIDTH}" height="{height}" viewBox="0 0 {PAGE_WIDTH} {height}" font-family="Inter,system-ui,sans-serif">
<rect width="100%" height="100%" fill="white"/>
<text x="40" y="46" font-size="26" font-weight="600">Visual diff {parent} → {commit}</text>
<text x="2200" y="46" font-size="20" text-anchor="end">page {page}/{page_count}</text>
"#
    );
    for (row, (path, before, after)) in pairs.iter().enumerate() {
        let y = HEADER_HEIGHT + ROW_HEIGHT * row as u32;
        for (column, (label, source)) in [("before", before), ("after", after)]
            .into_iter()
            .enumerate()
        {
            let x = 20 + CELL_WIDTH * column as u32;
            let image_x = x + (CELL_WIDTH - IMAGE_WIDTH) / 2;
            let image_y = y + 60;
            let encoded = STANDARD.encode(source);
            svg.push_str(&format!(
                r##"<text x="{}" y="{}" font-size="22" font-weight="600" text-anchor="middle">{} — {label}</text>
<rect x="{}" y="{}" width="{IMAGE_WIDTH}" height="{IMAGE_HEIGHT}" fill="white" stroke="#d1d5db"/>
<image x="{}" y="{}" width="{IMAGE_WIDTH}" height="{IMAGE_HEIGHT}" preserveAspectRatio="xMidYMid meet" href="data:image/png;base64,{encoded}"/>
"##,
                x + CELL_WIDTH / 2,
                y + 38,
                escape_xml(path),
                image_x,
                image_y,
                image_x,
                image_y,
            ));
        }
    }
    svg.push_str("</svg>\n");
    svg
}

fn render_png(svg: &str, options: &resvg::usvg::Options) -> Result<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_str(svg, options)
        .map_err(|error| anyhow::anyhow!("failed to parse contact sheet: {error}"))?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        Pixmap::new(size.width(), size.height()).context("failed to allocate contact sheet")?;
    resvg::render(&tree, Transform::from_scale(1.0, 1.0), &mut pixmap.as_mut());
    pixmap
        .encode_png()
        .context("failed to encode contact sheet")
}

fn placeholder(message: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="850"><rect width="100%" height="100%" fill="#f3f4f6"/><text x="500" y="425" font-family="system-ui" font-size="36" text-anchor="middle">{}</text></svg>"##,
        escape_xml(message)
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}
