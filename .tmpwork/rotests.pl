undef $/;
$_ = <STDIN>;

my $tests = <<'RUST';

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use crate::chrome::Command;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("One")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// An editor holding a document that asks not to be written, standing at
    /// the door with the question up.
    fn at_the_door(asked: WriteProtection) -> Editor {
        let mut editor = editor();
        editor.document.set_write_protection(Some(&asked));
        let opened = editor.asked_at_the_door(Path::new("reserved.docx"));
        assert!(opened.is_some(), "the document asked nothing");
        assert!(editor.dialog.is_some(), "nothing was asked at the door");
        editor
    }

    /// Types one letter and says whether it arrived.
    fn typing_arrives(editor: &mut Editor) -> bool {
        let before = editor.document.plain_text();
        editor.handle(Event::Char('x'));
        editor.document.plain_text() != before
    }

    #[test]
    fn a_document_that_asks_nothing_is_not_asked_about() {
        let mut editor = editor();
        assert!(editor.asked_at_the_door(Path::new("plain.docx")).is_none());
        assert!(!editor.is_read_only());
        assert!(editor.dialog.is_none());
    }

    #[test]
    fn a_recommendation_is_honoured_and_can_be_declined() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_read_only(), "the recommendation was not honoured");
        assert!(!typing_arrives(&mut editor), "a read-only document took typing");
        assert!(editor.status.contains("read-only"), "{}", editor.status);

        // And declined, which is what makes it a request.
        let mut editor = at_the_door(WriteProtection::recommended());
        if let Some(Field::Check { on, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ANSWER)
        {
            *on = false;
        }
        editor.finish_dialog(Answer::Accept);
        assert!(!editor.is_read_only());
        assert!(typing_arrives(&mut editor), "a document opened for writing refused typing");
    }

    #[test]
    fn cancelling_at_the_door_opens_it_read_only() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Cancel);
        assert!(editor.is_read_only(), "cancelling let it be written");
    }

    #[test]
    fn the_password_opens_it_for_writing_and_a_wrong_one_does_not() {
        let salt = b"0123456789abcdef";
        let mut editor = at_the_door(WriteProtection::behind("Fenchurch", salt));
        if let Some(Field::Secret { value, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ANSWER)
        {
            *value = "open sesame".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_read_only(), "a wrong password opened it for writing");
        assert!(editor.status.contains("not the password"), "{}", editor.status);

        let mut editor = at_the_door(WriteProtection::behind("Fenchurch", salt));
        if let Some(Field::Secret { value, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ANSWER)
        {
            *value = "Fenchurch".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert!(!editor.is_read_only(), "the right password did not open it");
        assert!(typing_arrives(&mut editor));
    }

    #[test]
    fn the_ribbon_and_the_caption_both_say_so() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Accept);

        assert!(editor.title.contains("(Read-Only)"), "the caption does not say: {}", editor.title);
        let state = editor.toolbar_state();
        assert!(
            !crate::chrome::is_enabled(Command::Format(wp_docx::CharacterFormat::Bold), &state),
            "the ribbon still offers to change a document that cannot be changed"
        );
        // And the reason given is this one rather than the restriction's.
        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        assert!(editor.status.contains("opened read-only"), "{}", editor.status);
    }

    #[test]
    fn the_file_page_sets_it_and_the_two_passwords_must_agree() {
        let mut editor = editor();
        editor.open_read_only_settings();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(RECOMMEND) {
            *on = true;
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(PASSWORD) {
            *value = "one".to_owned();
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(AGAIN) {
            *value = "another".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert_eq!(editor.document.write_protection(), None, "a mistyped password was written");
        assert!(editor.status.contains("not the same"), "{}", editor.status);
        assert!(editor.dialog.is_some(), "the dialog was not asked again");

        // The same answer typed the same way twice.
        let dialog = editor.dialog.as_mut().expect("asked again");
        assert!(
            matches!(dialog.fields.get(RECOMMEND), Some(Field::Check { on: true, .. })),
            "the tick was lost"
        );
        for row in [PASSWORD, AGAIN] {
            if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(row) {
                *value = "Fenchurch".to_owned();
            }
        }
        editor.finish_dialog(Answer::Accept);

        let asked = editor.document.write_protection().expect("nothing was written");
        assert!(asked.recommended);
        assert!(asked.opens_with("Fenchurch"));
    }

    #[test]
    fn the_file_page_is_the_way_back_out_as_well() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_read_only());

        // With no password, the line itself opens it for writing.
        editor.open_read_only_settings();
        assert!(!editor.is_read_only(), "the way out did not open it");
        assert!(typing_arrives(&mut editor));
    }

    #[test]
    fn the_line_on_the_file_page_says_which_of_the_four_things_is_true() {
        let mut editor = editor();
        assert!(editor.read_only_note().starts_with("Ask for"));

        editor.document.set_write_protection(Some(&WriteProtection::recommended()));
        assert!(editor.read_only_note().contains("asks to be opened read-only"));

        editor
            .document
            .set_write_protection(Some(&WriteProtection::behind("word", b"0123456789abcdef")));
        assert!(editor.read_only_note().starts_with("A password is needed"));

        editor.opened_read_only = true;
        assert!(editor.read_only_note().contains("is open read-only"));
    }
}
RUST

$_ .= $tests;
print;
