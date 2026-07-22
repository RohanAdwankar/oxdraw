use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tiny_skia::{Pixmap, Transform};

const EXPECTED_DIR: &str = "tests/expected";
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
    let args: Vec<_> = env::args().skip(1).collect();
    let include_mmdc = args.iter().any(|arg| arg == "--mmdc");
    let revisions: Vec<_> = args
        .iter()
        .filter(|arg| arg.as_str() != "--mmdc")
        .cloned()
        .collect();
    let repo = PathBuf::from(git(None, &["rev-parse", "--show-toplevel"])?);
    let (before, after) = comparison(&repo, &revisions)?;
    let changed = if let Some(after) = &after {
        git(
            Some(&repo),
            &["diff", "--name-only", &before, after, "--", EXPECTED_DIR],
        )?
    } else {
        git(
            Some(&repo),
            &["diff", "--name-only", &before, "--", EXPECTED_DIR],
        )?
    };
    let paths: Vec<_> = changed
        .lines()
        .filter(|path| path.ends_with(".svg"))
        .map(str::to_owned)
        .collect();
    if paths.is_empty() {
        bail!("comparison changes no SVG goldens under {EXPECTED_DIR}");
    }

    let before_label = short(&before);
    let after_label = after.as_deref().map(short).unwrap_or("working-tree");
    let output_dir =
        env::temp_dir().join(format!("oxdraw-visual-diff-{before_label}-{after_label}"));
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir)?;
    }
    let source_dir = output_dir.join("source");
    fs::create_dir_all(&source_dir)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();

    let mut pairs = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let before_svg =
            git_file(&repo, &before, &path)?.unwrap_or_else(|| placeholder("not present"));
        let after_svg = if let Some(after) = &after {
            git_file(&repo, after, &path)?
        } else {
            worktree_file(&repo, &path)?
        }
        .unwrap_or_else(|| placeholder("not present"));
        let name = path
            .trim_start_matches(&format!("{EXPECTED_DIR}/"))
            .trim_end_matches(".svg")
            .replace("/", "__");
        fs::write(
            source_dir.join(format!("{:02}_{name}_before.svg", index + 1)),
            &before_svg,
        )?;
        fs::write(
            source_dir.join(format!("{:02}_{name}_after.svg", index + 1)),
            &after_svg,
        )?;
        let mmdc = include_mmdc
            .then(|| render_mmdc(&repo, &path, &source_dir, index, &name))
            .transpose()?;
        pairs.push((
            path,
            render_png(&before_svg, &options)?,
            render_png(&after_svg, &options)?,
            mmdc,
        ));
    }

    let svg = contact_sheet(&pairs, before_label, after_label);
    fs::write(output_dir.join("visual-diff.svg"), &svg)?;
    let png = output_dir.join("visual-diff.png");
    fs::write(&png, render_png(&svg, &options)?)?;

    println!("visual diff: {}", output_dir.display());
    println!("  {}", png.display());
    Ok(())
}

fn comparison(repo: &Path, args: &[String]) -> Result<(String, Option<String>)> {
    let resolve = |revision: &str| commit(repo, specified(revision));
    match args {
        [] => Ok((resolve("HEAD")?, None)),
        [range] if range.contains("...") => {
            let (left, right) = range.split_once("...").unwrap();
            let (left, right) = (resolve(left)?, resolve(right)?);
            Ok((
                git(Some(repo), &["merge-base", &left, &right])?,
                Some(right),
            ))
        }
        [range] if range.contains("..") => {
            let (left, right) = range.split_once("..").unwrap();
            Ok((resolve(left)?, Some(resolve(right)?)))
        }
        [revision] => {
            let after = resolve(revision)?;
            Ok((resolve(&format!("{after}^"))?, Some(after)))
        }
        [left, right] => Ok((resolve(left)?, Some(resolve(right)?))),
        _ => {
            bail!(
                "usage: view_visual_diff [--mmdc] [<commit> | <from>..<to> | <from>...<to> | <from> <to>]"
            )
        }
    }
}

fn specified(revision: &str) -> &str {
    if revision.is_empty() {
        "HEAD"
    } else {
        revision
    }
}

fn commit(repo: &Path, revision: &str) -> Result<String> {
    git(
        Some(repo),
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )
}

fn short(revision: &str) -> &str {
    &revision[..revision.len().min(12)]
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

fn worktree_file(repo: &Path, path: &str) -> Result<Option<String>> {
    let path = repo.join(path);
    path.exists()
        .then(|| fs::read_to_string(path))
        .transpose()
        .map_err(Into::into)
}

fn render_mmdc(
    repo: &Path,
    golden: &str,
    source_dir: &Path,
    index: usize,
    name: &str,
) -> Result<Vec<u8>> {
    let input = repo
        .join("tests/input")
        .join(Path::new(golden).file_name().unwrap())
        .with_extension("mmd");
    if !input.exists() {
        return placeholder_png("mmdc input missing");
    }
    let output = source_dir.join(format!("{:02}_{name}_mmdc.png", index + 1));
    let result = match Command::new("mmdc")
        .current_dir(repo)
        .args(["-i"])
        .arg(&input)
        .args(["-o"])
        .arg(&output)
        .args(["-b", "white"])
        .output()
    {
        Ok(result) => result,
        Err(error) => {
            fs::write(
                source_dir.join(format!("{:02}_{name}_mmdc.error.txt", index + 1)),
                error.to_string(),
            )?;
            return placeholder_png("mmdc unavailable");
        }
    };
    if !result.status.success() {
        fs::write(
            source_dir.join(format!("{:02}_{name}_mmdc.error.txt", index + 1)),
            &result.stderr,
        )?;
        return placeholder_png("mmdc failed");
    }
    fs::read(output).map_err(Into::into)
}

fn contact_sheet(
    pairs: &[(String, Vec<u8>, Vec<u8>, Option<Vec<u8>>)],
    before: &str,
    after: &str,
) -> String {
    let columns = if pairs.iter().any(|pair| pair.3.is_some()) {
        3
    } else {
        2
    };
    let page_width = 40 + CELL_WIDTH * columns;
    let height = HEADER_HEIGHT + ROW_HEIGHT * pairs.len() as u32;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{page_width}" height="{height}" viewBox="0 0 {page_width} {height}" font-family="Inter,system-ui,sans-serif">
<rect width="100%" height="100%" fill="white"/>
<text x="40" y="46" font-size="26" font-weight="600">Visual diff {before} → {after}</text>
"#
    );
    for (row, (path, before, after, mmdc)) in pairs.iter().enumerate() {
        let y = HEADER_HEIGHT + ROW_HEIGHT * row as u32;
        let mut images = vec![("before", before), ("after", after)];
        images.extend(mmdc.as_ref().map(|image| ("mmdc", image)));
        for (column, (label, source)) in images.into_iter().enumerate() {
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

fn placeholder_png(message: &str) -> Result<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    render_png(&placeholder(message), &options)
}

fn escape_xml(value: &str) -> String {
    value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}
