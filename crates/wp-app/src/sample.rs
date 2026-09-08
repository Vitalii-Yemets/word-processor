//! The document shown when the program is started without a file.
//!
//! Not a placeholder: it is the manual, and it is written in the program it
//! describes, so every claim in it is one the reader can check on the spot.

use wp_docx::model::{Alignment, Block, Body, Paragraph, Run, RunContent};

pub fn welcome_document() -> Body {
    let mut body = Body::default();

    body.blocks.push(Block::Paragraph(
        Paragraph::text("Word Processor").with_style("Title").with_alignment(Alignment::Center),
    ));
    body.blocks.push(Block::Paragraph(
        Paragraph::from_runs(vec![Run::text("Every pixel on this page was drawn by this program")
            .italic()
            .colored("595959")])
        .with_alignment(Alignment::Center),
    ));

    body.blocks.push(Block::Paragraph(Paragraph::text("Try typing").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Click anywhere in the text to put the caret there, then type. Enter splits a \
         paragraph, Backspace joins one onto the last, and Ctrl+S saves.",
    )));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Drag across the text to select it, or hold Shift while clicking or using the arrow \
         keys. Ctrl+A selects everything, Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste, and \
         Ctrl+Z takes back whatever you did last - by the word, not by the keystroke. Ctrl+Y \
         puts it back.",
    )));

    body.blocks.push(Block::Paragraph(Paragraph::text("Formatting").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Ctrl+B, Ctrl+I and Ctrl+U set bold, italic and underline on whatever is selected. With \
         nothing selected they apply to the next thing you type, which is how you turn bold on \
         and then write the word - the strip along the bottom shows what is currently on.",
    )));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Ctrl+L, Ctrl+E, Ctrl+R and Ctrl+J align a paragraph to the left, the centre, the right \
         or both margins. Ctrl+Alt+1, 2 and 3 make it a heading, and Ctrl+Shift+N takes the \
         style off again.",
    )));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "An edit goes through the document model, which changes only the nodes it must. A file \
         opened here keeps everything this program does not model - a chart, a content control, \
         somebody else's tracked change - even after it has been typed in and saved.",
    )));

    body.blocks.push(Block::Paragraph(Paragraph::text("Lists").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Lists are read from the document's numbering definitions: what marks an item, how it is \
         counted, and how far it is indented all come from there.",
    )));
    for (text, level) in [
        ("A bulleted item", 0),
        ("Another one, at the same level", 0),
        ("A deeper item, with a mark of its own", 1),
    ] {
        body.blocks
            .push(Block::Paragraph(Paragraph::text(text).in_list(wp_docx::BULLET_LIST, level)));
    }
    for (text, level) in [
        ("Numbered items count in reading order", 0),
        ("So this one is the second", 0),
        ("A sub-item starts again at its own level", 1),
        ("And carries on from there", 1),
        ("Back out, and the count continues", 0),
    ] {
        body.blocks
            .push(Block::Paragraph(Paragraph::text(text).in_list(wp_docx::NUMBERED_LIST, level)));
    }

    body.blocks.push(Block::Paragraph(Paragraph::text("Tab stops").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Tab moves to the next stop, half an inch apart, which is where the marks on the ruler \
         are. The columns below are made of nothing but tabs.",
    )));
    for (key, value) in [("Ctrl+B", "Bold"), ("Ctrl+I", "Italic"), ("Ctrl+Alt+1", "Heading 1")] {
        body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run {
            content: vec![
                RunContent::Text(key.to_owned()),
                RunContent::Tab,
                RunContent::Text(value.to_owned()),
            ],
            ..Run::default()
        }])));
    }

    body.blocks
        .push(Block::Paragraph(Paragraph::text("What is drawn here").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "The archive was unpacked, the XML parsed, the styles resolved, the fonts read from this \
         machine, the glyph outlines rasterized and the window filled - all by code in this \
         project, with no third-party libraries of any kind.",
    )));

    body.blocks
        .push(Block::Paragraph(Paragraph::text("What is not here yet").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Arabic and the Indic scripts draw their isolated letter forms, because the shaping \
         engine that joins them is a later stage. Tables are laid out as their paragraphs, \
         without cells or borders. There is no ribbon yet, so formatting is reached from the \
         keyboard rather than from a toolbar.",
    )));

    body
}
