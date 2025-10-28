pub fn escape_xml(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

pub const LAYOUT_BLOCK_START: &str = "%% oxdraw-layout";
pub const LAYOUT_BLOCK_END: &str = "%% oxdraw-layout-end";

pub fn split_source_and_overrides(source: &str) -> anyhow::Result<(String, crate::LayoutOverrides)> {
    let mut definition_lines = Vec::new();
    let mut layout_lines = Vec::new();
    let mut in_block = false;
    let mut found_block = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(LAYOUT_BLOCK_START) {
            if in_block {
                anyhow::bail!("nested '{}' sections are not supported", LAYOUT_BLOCK_START);
            }
            in_block = true;
            found_block = true;
            continue;
        }
        if trimmed.eq_ignore_ascii_case(LAYOUT_BLOCK_END) {
            if !in_block {
                anyhow::bail!(
                    "encountered '{}' without a matching start",
                    LAYOUT_BLOCK_END
                );
            }
            in_block = false;
            continue;
        }

        if in_block {
            if trimmed.is_empty() {
                continue;
            }
            let mut segment = line.trim_start();
            if let Some(rest) = segment.strip_prefix("%%") {
                segment = rest.trim_start();
            }
            layout_lines.push(segment.to_string());
        } else {
            definition_lines.push(line);
        }
    }

    if in_block {
        anyhow::bail!(
            "layout metadata block was not terminated with '{}'",
            LAYOUT_BLOCK_END
        );
    }

    let mut definition = definition_lines.join("\n");
    if source.ends_with('\n') {
        definition.push('\n');
    }

    let overrides = if found_block {
        let json = layout_lines.join("\n");
        if json.trim().is_empty() {
            crate::LayoutOverrides::default()
        } else {
            serde_json::from_str(&json)
                .with_context(|| "failed to parse embedded oxdraw layout block")?
        }
    } else {
        crate::LayoutOverrides::default()
    };

    Ok((definition, overrides))
}

use anyhow::Context;
