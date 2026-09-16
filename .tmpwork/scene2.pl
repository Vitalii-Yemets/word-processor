undef $/; $_ = <STDIN>;
s~            "restricted" => \{~            // Word's General Options, off the File page: what a document
            // asks for the next time it is opened.
            "readonly" => {
                self.open_read_only_settings();
                if let Some(dialog) = self.dialog.as_mut() {
                    if let Some(crate::chrome::dialog::Field::Check { on, .. }) =
                        dialog.fields.get_mut(3)
                    {
                        *on = true;
                    }
                }
            }
            // And the question a document that asks is asked at the door.
            "reserved" => {
                self.document.set_write_protection(Some(
                    &wp_docx::readonly::WriteProtection::behind("word", b"0123456789abcdef"),
                ));
                self.asked_at_the_door(std::path::Path::new("Contract.docx"));
            }
            "restricted" => {~ or die "scene";
print;
