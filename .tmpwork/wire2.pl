undef $/;
$_ = <STDIN>;
my $what = $ARGV[0];

if ($what eq 'backstage') {
  s~        rows\.push\(super::sealing::signature_row\(&self\.document\)\);~        // Word's Always Open Read-Only, which is the other half of its
        // Protect Document menu and the only place in the program a document
        // can be asked to open read-only.
        rows.push(Row::new("Always Open Read-Only", self.read_only_note()));
        rows.push(super::sealing::signature_row(&self.document));~ or die "row";

  s~                // The first two lines are the password and the signatures;\n                // the rest are the properties, in the order the page listed\n                // them\.\n                match index\.checked_sub\(2\) \{\n                    None if index == 0 => self\.open_encryption\(\),\n                    None => self\.report_signatures\(\),\n                    Some\(property\) => self\.choose_property\(property\),\n                \}~                // The first three lines are the password, the read-only and
                // the signatures; the rest are the properties, in the order
                // the page listed them.
                match index.checked_sub(3) {
                    None if index == 0 => self.open_encryption(),
                    None if index == 1 => self.open_read_only_settings(),
                    None => self.report_signatures(),
                    Some(property) => self.choose_property(property),
                }~ or die "press";

  s~            // The password and the signatures are above the heading, because\n            // neither is one of the document\x27s properties\.\n            rows_heading_at: 2,~            // The password, the read-only and the signatures are above the
            // heading, because none of them is one of the document's
            // properties.
            rows_heading_at: 3,~ or die "heading";

  s~        assert_eq!\(contents\.rows\.len\(\), Field::ALL\.len\(\) \+ 2\);\n        assert_eq!\(contents\.rows\[0\]\.title, "Encrypt with Password"\);\n        assert_eq!\(contents\.rows\[1\]\.title, "Digital Signatures"\);\n        assert_eq!\(contents\.rows\[2\]\.title, "Title"\);\n        assert_eq!\(contents\.rows_heading_at, 2, "the properties heading is under both"\);~        assert_eq!(contents.rows.len(), Field::ALL.len() + 3);
        assert_eq!(contents.rows[0].title, "Encrypt with Password");
        assert_eq!(contents.rows[1].title, "Always Open Read-Only");
        assert_eq!(contents.rows[2].title, "Digital Signatures");
        assert_eq!(contents.rows[3].title, "Title");
        assert_eq!(contents.rows_heading_at, 3, "the properties heading is under all three");~ or die "test";
}

if ($what eq 'dispatch') {
  s~            Command::Save => \{\n                self\.save_now\(\);\n                self\.after_file_command\(\)\n            \}~            Command::Save => {
                // A document opened read-only is not saved over the file it
                // came from: that is exactly what it asked should not happen,
                // and Word offers Save As instead of refusing outright.
                if self.is_read_only() {
                    self.status =
                        crate::messages::t("This document was opened read-only: save a copy")
                            .to_owned();
                    self.save_as_now();
                } else {
                    self.save_now();
                }
                self.after_file_command()
            }~ or die "save";
}

print;
