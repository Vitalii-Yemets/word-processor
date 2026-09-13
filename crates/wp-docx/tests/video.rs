//! A video from the web in a document: the frame, the address and the mark.

use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition, EMU_PER_INCH};

const ADDRESS: &str = "https://example.org/watch?v=1";

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_caret(TextPosition::new(0, 7));
    document
}

/// The frame of a video. What is in it does not matter here: this layer puts
/// the bytes in the package and never looks at them, and what a picture is a
/// picture of is the drawing layer's question.
fn frame() -> Vec<u8> {
    b"a frame of a video".to_vec()
}

fn with_video() -> Document {
    let mut document = document();
    assert!(document
        .insert_web_video(&frame(), "png", ADDRESS, EMU_PER_INCH * 2, EMU_PER_INCH * 9 / 8)
        .expect("putting the video in"));
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The picture in the first paragraph, if there is one.
fn picture_in(document: &Document) -> Option<wp_docx::model::Picture> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { return None };
    paragraph.runs.iter().find_map(|run| {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Picture(picture) => Some((**picture).clone()),
            _ => None,
        })
    })
}

#[test]
fn the_frame_goes_in_as_a_picture() {
    let document = with_video();
    let picture = picture_in(&document).expect("a picture");
    assert!(
        document.relationship_target(&picture.relationship).is_some(),
        "the frame is not in the package"
    );
}

#[test]
fn the_picture_says_it_stands_for_a_video() {
    let document = with_video();
    let picture = picture_in(&document).expect("a picture");
    assert!(picture.video, "nothing says this picture is a video");
}

#[test]
fn the_address_comes_back_off_the_drawing() {
    let mut document = with_video();
    // Beside the drawing, which is where a press on it puts the caret.
    document.set_caret(TextPosition::new(0, 7));
    assert_eq!(document.drawing_link_here().as_deref(), Some(ADDRESS));
}

#[test]
fn a_video_with_no_address_is_not_a_video() {
    let mut document = document();
    assert!(!document
        .insert_web_video(&frame(), "png", "  ", EMU_PER_INCH, EMU_PER_INCH)
        .expect("nothing"));
    assert!(picture_in(&document).is_none(), "a frame went in with nothing to play");
}

#[test]
fn a_video_is_one_undo() {
    let mut document = document();
    assert!(document
        .insert_web_video(&frame(), "png", ADDRESS, EMU_PER_INCH, EMU_PER_INCH)
        .expect("putting the video in"));
    assert!(picture_in(&document).is_some());

    assert!(document.undo());
    assert!(picture_in(&document).is_none(), "the frame is still in the text");
}

#[test]
fn an_ordinary_picture_is_not_marked_as_one() {
    let mut document = document();
    assert!(document
        .insert_picture(&frame(), "png", EMU_PER_INCH, EMU_PER_INCH)
        .expect("putting the picture in"));
    let picture = picture_in(&document).expect("a picture");
    assert!(!picture.video);
    assert_eq!(document.drawing_link_here(), None);
}
