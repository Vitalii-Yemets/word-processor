undef $/; $_ = <STDIN>;
s~\n/// Whether a path is one a document opened read-only may be saved over\.\n///\n/// It is not: saving over the file a document was opened read-only from is\n/// exactly what the document asked should not happen, and Word offers Save As\n/// instead\. Somewhere else is another matter, and that is the copy\.\n\#\[must_use\]\npub\(super\) fn is_the_same_file\(file: Option<&PathBuf>, path: &Path\) -> bool \{\n    file\.is_some_and\(\|open\| open == path\)\n\}\n~~ or die "dead";
s~use std::path::\{Path, PathBuf\};~use std::path::Path;~ or die "import";
print;
