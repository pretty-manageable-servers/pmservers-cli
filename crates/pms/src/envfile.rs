use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};

fn unquote(v: &str) -> &str {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

pub fn parse_lines(text: &str) -> Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            bail!("line {}: expected KEY=VALUE", n + 1);
        };
        vars.insert(key.trim().to_owned(), unquote(value.trim()).to_owned());
    }
    Ok(vars)
}

/// The variables from `--env-file` first, then `--env`. The later one wins.
pub fn collect(pairs: &[String], file: Option<&Path>) -> Result<BTreeMap<String, String>> {
    let mut vars = match file {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            parse_lines(&text).with_context(|| path.display().to_string())?
        }
        None => BTreeMap::new(),
    };
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("--env {pair:?}: expected KEY=VALUE");
        };
        vars.insert(key.to_owned(), value.to_owned());
    }
    Ok(vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_files() {
        let v = parse_lines("# c\n\nA=1\nexport B=\"two words\"\nC='x=y'\n  D = 4 \n").unwrap();
        assert_eq!(v["A"], "1");
        assert_eq!(v["B"], "two words");
        assert_eq!(v["C"], "x=y");
        assert_eq!(v["D"], "4");
    }

    #[test]
    fn bad_line_names_its_number() {
        assert!(parse_lines("A=1\nnope\n")
            .unwrap_err()
            .to_string()
            .contains("line 2"));
    }

    #[test]
    fn flags_beat_the_file_and_need_equals() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join(".env");
        std::fs::write(&f, "A=file\nB=file\n").unwrap();
        let v = collect(&["A=flag".into(), "C=a=b".into()], Some(&f)).unwrap();
        assert_eq!(
            (v["A"].as_str(), v["B"].as_str(), v["C"].as_str()),
            ("flag", "file", "a=b")
        );
        assert!(collect(&["NOEQ".into()], None).is_err());
    }
}
