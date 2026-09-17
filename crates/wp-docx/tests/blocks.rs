//! Pieces of a document saved by name and put back later.

use wp_docx::blocks::{BuildingBlock, AUTO_TEXT, QUICK_PARTS};
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;

fn document(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn body(lines: &[&str]) -> Body {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    body
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_has_no_blocks_to_begin_with() {
    assert!(document(&["one"]).building_blocks().is_empty());
}

#[test]
fn a_block_saved_is_there_after_the_template_is_saved_and_opened() {
    let mut template = document(&["nothing in particular"]);
    let block = BuildingBlock::named("Signature");
    assert!(template.add_building_block(&block, &body(&["Yours faithfully,", "A Person"])));

    let reopened = round_trip(&template);
    let blocks = reopened.building_blocks();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].name, "Signature");
    assert_eq!(blocks[0].gallery, QUICK_PARTS);
    assert_eq!(blocks[0].category, "General");

    let saved = reopened.building_block_body("Signature").expect("the block");
    let text: Vec<String> = saved
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.plain_text(),
            Block::Table(_) => String::new(),
        })
        .collect();
    assert_eq!(text, vec!["Yours faithfully,", "A Person"]);
}

#[test]
fn a_block_goes_into_another_document_where_the_caret_is() {
    let mut template = document(&["template"]);
    template.add_building_block(&BuildingBlock::named("Signature"), &body(&["Yours faithfully,"]));

    let mut letter = document(&["Dear Sir", "", "The matter in hand."]);
    letter.set_caret(wp_docx::TextPosition::new(2, 0));
    assert!(letter.insert_building_block(&template, "Signature"));
    assert!(letter.plain_text().contains("Yours faithfully,"), "{}", letter.plain_text());
}

#[test]
fn a_block_that_is_not_there_puts_nothing_in() {
    let template = document(&["template"]);
    let mut letter = document(&["Dear Sir"]);
    let before = letter.plain_text();
    assert!(!letter.insert_building_block(&template, "Nothing"));
    assert_eq!(letter.plain_text(), before);
}

#[test]
fn a_block_saved_twice_under_one_name_is_one_block() {
    let mut template = document(&["template"]);
    template.add_building_block(&BuildingBlock::named("Signature"), &body(&["First"]));
    template.add_building_block(&BuildingBlock::named("Signature"), &body(&["Second"]));

    let reopened = round_trip(&template);
    assert_eq!(reopened.building_blocks().len(), 1, "two blocks share one name");
    let saved = reopened.building_block_body("Signature").expect("the block");
    let Some(Block::Paragraph(paragraph)) = saved.blocks.first() else { panic!("{saved:?}") };
    assert_eq!(paragraph.plain_text(), "Second", "the later one did not win");
}

#[test]
fn a_block_with_no_name_is_not_saved() {
    let mut template = document(&["template"]);
    assert!(!template.add_building_block(&BuildingBlock::named("   "), &body(&["nothing"])));
    assert!(template.building_blocks().is_empty());
}

#[test]
fn a_block_can_be_taken_away() {
    let mut template = document(&["template"]);
    template.add_building_block(&BuildingBlock::named("One"), &body(&["one"]));
    template.add_building_block(&BuildingBlock::named("Two"), &body(&["two"]));

    assert!(template.remove_building_block("One"));
    assert!(!template.remove_building_block("One"), "it was taken away twice");
    let left: Vec<String> =
        round_trip(&template).building_blocks().into_iter().map(|block| block.name).collect();
    assert_eq!(left, vec!["Two".to_owned()]);
}

#[test]
fn the_galleries_are_kept_apart() {
    let mut template = document(&["template"]);
    template.add_building_block(&BuildingBlock::named("Quick"), &body(&["quick"]));
    template.add_building_block(
        &BuildingBlock::named("Automatic").in_gallery(AUTO_TEXT),
        &body(&["automatic"]),
    );

    let reopened = round_trip(&template);
    let quick: Vec<String> =
        reopened.blocks_in(QUICK_PARTS).into_iter().map(|block| block.name).collect();
    let automatic: Vec<String> =
        reopened.blocks_in(AUTO_TEXT).into_iter().map(|block| block.name).collect();
    assert_eq!(quick, vec!["Quick".to_owned()]);
    assert_eq!(automatic, vec!["Automatic".to_owned()]);
}

#[test]
fn a_block_holding_a_table_comes_back_as_a_table() {
    let mut template = document(&["template"]);
    let mut body = Body::default();
    body.blocks.push(Block::Table(Box::new(wp_docx::model::Table::from_rows(vec![
        wp_docx::model::TableRow::text(&["first", "second"]),
        wp_docx::model::TableRow::text(&["a", "b"]),
    ]))));
    template.add_building_block(&BuildingBlock::named("Grid"), &body);

    let saved = round_trip(&template).building_block_body("Grid").expect("the block");
    assert!(matches!(saved.blocks.first(), Some(Block::Table(_))), "{saved:?}");
}

#[test]
fn the_part_is_written_where_the_format_says_and_named_as_what_it_is() {
    let mut template = document(&["template"]);
    template.add_building_block(&BuildingBlock::named("Signature"), &body(&["Yours"]));
    let bytes = template.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");

    assert!(package.part("word/glossary/document.xml").is_some(), "the part is somewhere else");
    assert_eq!(
        package.content_type("word/glossary/document.xml"),
        Some(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml"
        )
    );
    let relationships = package.relationships("word/document.xml").expect("the relationships");
    assert!(
        relationships.all().iter().any(|relationship| relationship.kind
            == "http://schemas.openxmlformats.org/officeDocument/2006/relationships/glossaryDocument"),
        "nothing points at the glossary"
    );
}

#[test]
fn a_block_can_be_refiled_without_being_saved_again() {
    let mut template = document(&["unused"]);
    let saved = BuildingBlock::named("Block 1");
    assert!(template.add_building_block(&saved, &body(["Yours faithfully,"].as_ref())));

    let wanted = BuildingBlock {
        name: "Sign-off".to_owned(),
        gallery: AUTO_TEXT.to_owned(),
        category: "Letters".to_owned(),
        description: "The end of a letter".to_owned(),
    };
    assert!(template.edit_building_block("Block 1", &wanted));

    let template = round_trip(&template);
    let blocks = template.building_blocks();
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].name, "Sign-off");
    assert_eq!(blocks[0].gallery, AUTO_TEXT);
    assert_eq!(blocks[0].category, "Letters");
    assert_eq!(blocks[0].description, "The end of a letter");

    // And what is in it is what was in it, which is the whole point of
    // refiling rather than saving again.
    let inside = template.building_block_body("Sign-off").expect("its content");
    assert_eq!(inside.blocks.len(), 1);
    assert!(template.building_block_body("Block 1").is_none(), "the old name still answers");
}

#[test]
fn refiling_one_onto_another_s_name_is_refused() {
    let mut template = document(&["unused"]);
    template.add_building_block(&BuildingBlock::named("First"), &body(["one"].as_ref()));
    template.add_building_block(&BuildingBlock::named("Second"), &body(["two"].as_ref()));

    let wanted = BuildingBlock::named("First");
    assert!(!template.edit_building_block("Second", &wanted), "two blocks of one name");
    assert_eq!(template.building_blocks().len(), 2);
    // Keeping its own name is not taking another's.
    assert!(template.edit_building_block("Second", &BuildingBlock::named("Second")));
}

#[test]
fn a_block_that_is_not_there_cannot_be_refiled_and_a_nameless_one_is_refused() {
    let mut template = document(&["unused"]);
    template.add_building_block(&BuildingBlock::named("First"), &body(["one"].as_ref()));
    assert!(!template.edit_building_block("Nothing", &BuildingBlock::named("Something")));
    assert!(!template.edit_building_block("First", &BuildingBlock::named("   ")));
}

#[test]
fn the_gallery_a_block_is_in_decides_which_menu_it_is_on() {
    let mut template = document(&["unused"]);
    template.add_building_block(&BuildingBlock::named("A quick part"), &body(["one"].as_ref()));
    template.add_building_block(
        &BuildingBlock::named("Some text").in_gallery(AUTO_TEXT),
        &body(["two"].as_ref()),
    );

    let template = round_trip(&template);
    assert_eq!(template.blocks_in(QUICK_PARTS).len(), 1);
    assert_eq!(template.blocks_in(AUTO_TEXT).len(), 1);
    assert_eq!(template.blocks_in(AUTO_TEXT)[0].name, "Some text");
}
