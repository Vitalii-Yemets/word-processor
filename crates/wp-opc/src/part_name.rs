//! Part names and how relationship targets resolve against them.
//!
//! The specification writes part names with a leading slash (`/word/document.xml`)
//! while the ZIP entry that holds one has no slash (`word/document.xml`). Both
//! forms appear in real documents — content type overrides use the first,
//! relationship targets are usually relative — so everything is normalized to the
//! archive form on the way in, and only converted back when writing.

use crate::Error;

/// Normalizes a part name to the form used for archive entries: no leading
/// slash, forward separators.
#[must_use]
pub fn normalize(name: &str) -> String {
    name.trim_start_matches('/').replace('\\', "/")
}

/// Checks a part name against the rules of ECMA-376 Part 2, §9.1.1.
pub fn validate(name: &str) -> Result<(), Error> {
    let invalid = |reason: &'static str| Error::InvalidPartName { name: name.to_owned(), reason };

    if name.is_empty() {
        return Err(invalid("a part name cannot be empty"));
    }
    if name.ends_with('/') {
        return Err(invalid("a part name cannot end with a slash"));
    }
    if name.contains("//") {
        return Err(invalid("a part name cannot contain an empty segment"));
    }
    for segment in name.split('/') {
        if segment == "." || segment == ".." {
            return Err(invalid("a part name cannot contain \".\" or \"..\""));
        }
        if segment.ends_with('.') {
            return Err(invalid("a segment cannot end with a dot"));
        }
    }
    Ok(())
}

/// The name of the part holding a given part's relationships.
///
/// Relationships live beside their owner in a `_rels` directory, with `.rels`
/// appended: `word/document.xml` keeps them in `word/_rels/document.xml.rels`.
/// The package's own relationships are in `_rels/.rels`.
#[must_use]
pub fn relationships_part_for(part: &str) -> String {
    let part = normalize(part);
    match part.rsplit_once('/') {
        Some((directory, file)) => format!("{directory}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Whether a part is itself a relationships part.
#[must_use]
pub fn is_relationships_part(name: &str) -> bool {
    let name = normalize(name);
    name.ends_with(".rels") && (name.starts_with("_rels/") || name.contains("/_rels/"))
}

/// Resolves a relationship target against the part that declares it.
///
/// A target is normally relative to the *directory* of its source part, so a
/// relationship in `word/document.xml` pointing at `media/image1.png` means
/// `word/media/image1.png`. A target starting with `/` is relative to the
/// package root instead.
pub fn resolve_target(source_part: &str, target: &str) -> Result<String, Error> {
    let invalid = || Error::InvalidTarget {
        source: source_part.to_owned(),
        target: target.to_owned(),
    };

    if target.is_empty() {
        return Err(invalid());
    }

    // Held for the whole function: the segment list borrows from it.
    let source = normalize(source_part);
    let mut segments: Vec<&str> = Vec::new();

    if !target.starts_with('/') {
        // Start from the source part's directory, not the part itself.
        if let Some((directory, _)) = source.rsplit_once('/') {
            segments.extend(directory.split('/'));
        }
    }

    for segment in target.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                // Climbing above the package root is how a crafted document
                // would try to name a file outside it.
                if segments.pop().is_none() {
                    return Err(invalid());
                }
            }
            other => segments.push(other),
        }
    }

    let resolved = segments.join("/");
    if resolved.is_empty() {
        return Err(invalid());
    }
    Ok(resolved)
}

/// The file extension of a part name, lowercased, if it has one.
#[must_use]
pub fn extension(name: &str) -> Option<String> {
    let file = name.rsplit('/').next()?;
    let (_, extension) = file.rsplit_once('.')?;
    if extension.is_empty() {
        None
    } else {
        Some(extension.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_the_two_forms_of_a_part_name() {
        assert_eq!(normalize("/word/document.xml"), "word/document.xml");
        assert_eq!(normalize("word/document.xml"), "word/document.xml");
    }

    #[test]
    fn finds_the_relationships_part() {
        assert_eq!(relationships_part_for("word/document.xml"), "word/_rels/document.xml.rels");
        assert_eq!(relationships_part_for("/word/document.xml"), "word/_rels/document.xml.rels");
        assert_eq!(
            relationships_part_for("word/header1.xml"),
            "word/_rels/header1.xml.rels"
        );
        // The package itself is addressed as an empty name.
        assert_eq!(relationships_part_for(""), "_rels/.rels");
    }

    #[test]
    fn recognizes_relationships_parts() {
        assert!(is_relationships_part("_rels/.rels"));
        assert!(is_relationships_part("word/_rels/document.xml.rels"));
        assert!(!is_relationships_part("word/document.xml"));
        assert!(!is_relationships_part("word/notrels/document.xml.rels"));
    }

    #[test]
    fn resolves_targets_relative_to_the_source_directory() {
        assert_eq!(
            resolve_target("word/document.xml", "media/image1.png").unwrap(),
            "word/media/image1.png"
        );
        assert_eq!(resolve_target("word/document.xml", "styles.xml").unwrap(), "word/styles.xml");
        assert_eq!(
            resolve_target("word/document.xml", "../docProps/core.xml").unwrap(),
            "docProps/core.xml"
        );
        // The package's own relationships live at the root.
        assert_eq!(resolve_target("", "word/document.xml").unwrap(), "word/document.xml");
    }

    #[test]
    fn resolves_absolute_targets_against_the_package_root() {
        assert_eq!(
            resolve_target("word/document.xml", "/word/styles.xml").unwrap(),
            "word/styles.xml"
        );
    }

    #[test]
    fn refuses_targets_that_climb_out_of_the_package() {
        // A document could otherwise name a file on the machine that opens it.
        for target in ["../../../etc/passwd", "/../outside", "..", "../.."] {
            assert!(
                resolve_target("word/document.xml", target).is_err(),
                "should have been refused: {target:?}"
            );
        }
    }

    #[test]
    fn validates_part_names() {
        for name in ["word/document.xml", "docProps/app.xml", "_rels/.rels"] {
            assert!(validate(name).is_ok(), "should be valid: {name:?}");
        }
        for name in ["", "word/", "word//document.xml", "word/../secret", "word/name./x"] {
            assert!(validate(name).is_err(), "should be invalid: {name:?}");
        }
    }

    #[test]
    fn extracts_extensions_case_insensitively() {
        assert_eq!(extension("word/document.xml").as_deref(), Some("xml"));
        assert_eq!(extension("word/media/IMAGE1.PNG").as_deref(), Some("png"));
        assert_eq!(extension("_rels/.rels").as_deref(), Some("rels"));
        assert_eq!(extension("word/noextension").as_deref(), None);
    }
}
