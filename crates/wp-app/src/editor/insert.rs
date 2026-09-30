//! Putting a table or a picture into the document, and switching the theme.

use crate::messages::t;
use wp_docx::EMU_PER_INCH;
use wp_shell::App;
use wp_shell::Response;

use crate::chrome::{Mode, StyleSample, TableGrid, Theme};

use super::{Editor, DPI};

/// The kinds of picture the dialog offers, which are the ones the decoder reads
/// plus the ones a package can carry that Word will render even if this program
/// cannot yet.
pub(super) const PICTURE_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter {
        label: "Pictures (*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.emf;*.wmf)",
        pattern: "*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.emf;*.wmf",
    },
    wp_shell::dialog::FileFilter { label: "All files (*.*)", pattern: "*.*" },
];

impl Editor {
    /// Drops open the grid that asks how big a table should be.
    pub(super) fn open_table_grid(&mut self) -> Response {
        if self.table_grid.take().is_some() {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(crate::chrome::Command::InsertTable)
        else {
            return Response::Ignored;
        };
        self.table_grid = Some(TableGrid::new(left, top + 74.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts a table of the size the grid was swept to into the document.
    pub(super) fn choose_table(&mut self, rows: usize, columns: usize) -> Response {
        self.table_grid = None;
        let changed = self.document.insert_table(rows, columns);
        self.edited(changed, &format!("{columns}x{rows} table"))
    }

    /// Asks for a picture and puts it where the caret is.
    pub(super) fn insert_picture(&mut self) -> Response {
        let Some(path) = wp_shell::dialog::open_file(
            t("Insert Picture"),
            &super::files::readable(PICTURE_FILTERS),
        ) else {
            return Response::Ignored;
        };
        self.insert_picture_file(&path)
    }

    /// Puts the picture in a file where the caret is: what Insert Picture
    /// does once the file is chosen, and what a picture dropped on the page
    /// does.
    pub(super) fn insert_picture_file(&mut self, path: &std::path::Path) -> Response {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                wp_shell::dialog::show_error(&crate::messages::with(
                    "Cannot read {0}: {1}",
                    &[&path.display().to_string(), &error.to_string()],
                ));
                return Response::Ignored;
            }
        };

        // A picture's own size is its pixels read at the screen's usual
        // ninety-six to the inch, which is the size Word gives it too. One that
        // is wider than the text is brought down to fit, because a photograph
        // from a camera is thousands of pixels across.
        let Ok(image) = wp_image::decode(&bytes) else {
            wp_shell::dialog::show_error(&crate::messages::with(
                "{0} is not a picture this program can read. PNG, JPEG, BMP, GIF, TIFF and the metafiles are.",
                &[&path.display().to_string()],
            ));
            return Response::Ignored;
        };

        let per_pixel = EMU_PER_INCH / DPI as i64;
        let mut width = image.width.max(1) as i64 * per_pixel;
        let mut height = image.height.max(1) as i64 * per_pixel;
        let room = self.text_width_emu();
        if room > 0 && width > room {
            height = height * room / width;
            width = room;
        }

        let extension =
            path.extension().and_then(|value| value.to_str()).unwrap_or("png").to_owned();

        match self.document.insert_picture(&bytes, &extension, width, height) {
            Ok(inserted) => {
                self.choose_drawing_here();
                self.edited(inserted, "Picture")
            }
            Err(error) => {
                wp_shell::dialog::show_error(&crate::messages::with(
                    "Cannot insert the picture: {0}",
                    &[&error.to_string()],
                ));
                Response::Ignored
            }
        }
    }

    /// How wide the text is on the page, in the unit a drawing is measured in.
    pub(super) fn text_width_emu(&self) -> i64 {
        let metrics = wp_layout::PageMetrics::from_document(&self.document);
        let points = metrics.width - metrics.margin_left - metrics.margin_right;
        if points <= 0.0 {
            return 0;
        }
        (points as f64 / 72.0 * EMU_PER_INCH as f64) as i64
    }

    /// Swaps the light theme for the dark one, and back.
    ///
    /// The document is laid out again rather than merely repainted, because
    /// text the document calls "automatic" is a different colour in each — the
    /// colour is decided when the text is laid out, not when it is drawn.
    pub(super) fn toggle_theme(&mut self) -> Response {
        let wanted = self.theme.mode.other();
        self.theme = Theme::of(wanted);
        self.tell_desktop_the_theme();
        self.relayout();
        self.needs_redraw = true;
        self.remember_window();
        self.status = match wanted {
            Mode::Dark => "Dark mode".to_owned(),
            Mode::Light => "Light mode".to_owned(),
        };
        Response::Redraw
    }

    /// The styles the gallery offers: the document's own.
    ///
    /// Read out of the document rather than listed here, because a document
    /// brings its own styles and a gallery that offered a fixed seven would be
    /// wrong about every document but the ones this program made.
    ///
    /// A document that limits formatting to a selection of styles has the
    /// rest left out. Word does the same, and the alternative is a gallery
    /// where half the tiles do nothing when they are clicked.
    pub(super) fn style_gallery(&self) -> Vec<StyleSample> {
        let body_size = self.document.styles().resolve_run(None, &Default::default());
        let mut out = vec![StyleSample {
            id: None,
            name: crate::names::shown("Normal"),
            bold: false,
            italic: false,
            size: body_size.size_half_points as f32 / 2.0,
        }];

        let limited = self.document.formatting_is_limited();
        for style in self.document.styles().all() {
            if style.kind != wp_docx::StyleKind::Paragraph || style.is_default {
                continue;
            }
            if limited && style.locked {
                continue;
            }
            let label = style.name.clone().unwrap_or_else(|| style.id.clone());
            let resolved = self.document.styles().resolve_run(Some(&style.id), &Default::default());
            out.push(StyleSample {
                id: Some(style.id.clone()),
                name: crate::names::shown(&label),
                bold: resolved.bold,
                italic: resolved.italic,
                size: resolved.size_half_points as f32 / 2.0,
            });
        }

        // Ordered as Word orders its gallery: the body style, then the headings
        // in depth order, then everything else as the document listed it.
        out.sort_by_key(|sample| {
            let rank = match sample.id.as_deref() {
                None => 0,
                Some("Title") => 1,
                Some("Subtitle") => 2,
                Some(other) if other.starts_with("Heading") => {
                    3 + other[7..].parse::<u8>().unwrap_or(9)
                }
                Some(_) => 60,
            };
            (rank, sample.name.clone())
        });
        out
    }
}

impl Editor {
    /// What the style box says: the name of the paragraph's style as it is
    /// shown — Word's own in the interface's language, the person's own as
    /// they named it — and not the identifier the paragraph refers to it by.
    pub(super) fn style_name_here(&self) -> String {
        let name = self.document.style_here().map_or_else(
            || "Normal".to_owned(),
            |id| self.document.styles().get(&id).and_then(|style| style.name.clone()).unwrap_or(id),
        );
        crate::names::shown(&name)
    }
}

impl Editor {
    /// Sets one of the choices `--picture` can ask for.
    ///
    /// The interface can only be looked at through a rendered picture on a
    /// machine with no display, and every one of these is something that would
    /// otherwise never appear in one.
    pub fn set_view_option(&mut self, option: &str) -> Result<(), String> {
        use crate::chrome::ribbon::Tab;

        match option {
            "light" => {
                self.theme = Theme::of(Mode::Light);
                self.relayout();
            }
            "dark" => {
                self.theme = Theme::of(Mode::Dark);
                self.relayout();
            }
            "nonav" => self.show_navigation = false,
            "norulers" => self.show_rulers = false,
            "marks" => self.show_marks = true,
            "joined" => self.joined_pages = true,
            "find" => self.find_bar = Some(crate::chrome::findbar::FindBar::new(true)),
            "print" => {
                self.open_print();
            }
            "printselection" => {
                // A paragraph selected, so the Print page can be photographed
                // showing the selection laid out on its own.
                self.document.set_caret(wp_docx::TextPosition::new(4, 0));
                let end = self.document.paragraph_text(6).unwrap_or_default().len();
                self.document.extend_selection_to(wp_docx::TextPosition::new(6, end));
                self.open_print();
                if let Some(pane) = &mut self.print_pane {
                    pane.settings.which = crate::chrome::printpane::Which::Selection;
                }
                self.print_preview = self.layout_for_print(wp_layout::Device::screen());
            }
            "pageborder" => {
                // A border round the pages, which is the one thing on the
                // paper that is not put there by any of the text.
                let line = wp_docx::model::Border::line("double", 12, Some("2B579A"));
                let borders = wp_docx::pageborders::PageBorders::box_all(&line);
                self.document.set_page_borders_everywhere(&borders);
                self.relayout();
            }
            "paste" => {
                // A paragraph copied and put down again with the little button
                // showing and its menu open, which is the only way to look at
                // them on a machine with no clipboard and no pointer.
                let end = self.document.paragraph_text(4).unwrap_or_default().len();
                let landing = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(4, 0));
                self.document.extend_selection_to(wp_docx::TextPosition::new(4, end));
                let copied = self.document.copy_selection();
                let text = self.document.selected_text();

                self.document.set_caret(wp_docx::TextPosition::new(3, landing));
                self.put_down(&text, &copied, super::paste::PasteAs::KeepSource);
                self.relayout();
                self.open_paste_menu();
            }
            // A word corrected as it was typed, with the little box under it
            // open, for the same reason.
            "corrected" => {
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                for character in " teh cat".chars() {
                    self.type_character(character);
                }
                if let Some(made) = &mut self.corrected {
                    made.shown = true;
                }
                self.relayout();
                self.open_correction_options();
            }
            // The same word with the pointer resting on it: the thin bar
            // under its first letter, which comes before the box.
            "corrected-bar" => {
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                for character in " teh cat".chars() {
                    self.type_character(character);
                }
                if let Some(made) = &mut self.corrected {
                    made.bar = true;
                }
                self.relayout();
            }
            // A table typed as plus signs and hyphens and a heading typed as a
            // line entered twice, each made by the keystrokes that make it.
            "typed-formats" => {
                use wp_shell::{App, Event, Key, Modifiers};
                let enter = Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() };
                self.autocorrect.headings = true;
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.handle(enter.clone());
                for character in "Results".chars() {
                    self.type_character(character);
                }
                self.handle(enter.clone());
                self.handle(enter.clone());
                for character in "+------------+------------------------+".chars() {
                    self.type_character(character);
                }
                self.handle(enter);
                for character in "Name".chars() {
                    self.type_character(character);
                }
                self.relayout();
            }
            // Word's AutoFormat dialog, and what it asks after formatting a
            // plain text for review, over the text with its changes marked.
            "autoformat-dialog" => {
                self.open_autoformat();
            }
            "autoformat-review" => {
                use wp_docx::model::{Block, Body, Paragraph};
                let mut body = Body::default();
                for text in [
                    "Minutes of the meeting",
                    "",
                    "The chair said \"we're on the 3rd draft\" - and *nobody* argued.",
                    "The notes are at www.example.com for anyone who missed it.",
                    "- agree the budget",
                    "- book the hall",
                    "1. first reading",
                    "2. second reading",
                ] {
                    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
                }
                if let Ok(document) = wp_docx::Document::create(&body) {
                    self.set_document(document, None);
                    self.relayout();
                    self.run(super::Command::AutoFormat);
                    if let Some(dialog) = &mut self.dialog {
                        if let Some(crate::chrome::dialog::Field::Choice { current, .. }) =
                            dialog.fields.get_mut(1)
                        {
                            *current = 1;
                        }
                    }
                    self.finish_dialog(crate::chrome::dialog::Answer::Accept);
                }
            }
            // A word looked up in the other language, with the list open, for
            // the same reason again.
            "translate" => {
                self.ribbon.tab = crate::chrome::ribbon::Tab::Review;
                let (width, height) = (self.view_width, self.view_height);
                self.draw(width, height);
                let text = self.document.paragraph_text(3).unwrap_or_default();
                let at = text.find("text").unwrap_or(0);
                self.document.set_caret(wp_docx::TextPosition::new(3, at + 1));
                self.translate_selection();
            }
            // The Translator pane opened empty, turned round to German into
            // English, and a word typed into its box.
            "translator-typed" => {
                self.ribbon.tab = crate::chrome::ribbon::Tab::Review;
                self.open_translator(String::new(), None);
                if let Some((x, y)) = self.translator_pane_place(true) {
                    self.translator_press(x, y);
                }
                for character in "Hund".chars() {
                    self.translator_character(character);
                }
            }
            // The window on a screen of twice the density: everything twice
            // as many pixels across, sharp, and the same size to the eye.
            "hidpi" => {
                self.handle(wp_shell::Event::ScaleChanged { scale: 2.0 });
            }
            // Japanese being composed by an input method: the sounds typed so
            // far under a dotted line, the clause the person is choosing a
            // conversion for under a thick one, the rest converted under a
            // thin one, and the caret inside it.
            "ime" => {
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.type_character(' ');
                self.compose(String::new(), 0, Vec::new());
                let attributes = [
                    wp_shell::CompositionAttribute::Converted,
                    wp_shell::CompositionAttribute::Converted,
                    wp_shell::CompositionAttribute::Target,
                    wp_shell::CompositionAttribute::Target,
                    wp_shell::CompositionAttribute::Input,
                    wp_shell::CompositionAttribute::Input,
                    wp_shell::CompositionAttribute::Input,
                ];
                self.compose("日本語をにゅう".to_owned(), 7, attributes.to_vec());
            }
            // The interface in another language, and in the pseudo-language
            // that shows what has not been through the catalogue.
            "german" => {
                crate::messages::set_language("de");
                self.relayout();
            }
            "pseudo" => {
                crate::messages::set_language(crate::messages::PSEUDO);
                self.relayout();
            }
            // And the same window read right to left, which is what an
            // Arabic or a Hebrew interface is.
            "mirrored" => {
                crate::messages::set_language(crate::messages::PSEUDO_MIRRORED);
                self.relayout();
            }
            // The Document Recovery pane, as the first start after a crash
            // shows it: the copies a run that did not end left behind, with
            // one of them open.
            // A document carrying Visual Basic, with what one of its macros
            // says open in front of it. The project is built here because the
            // repository holds no Word file to take one from — see the
            // corpus, which is every machine's own.
            "macros" => {
                self.vba = wp_vba::Project::open(&wp_vba::example(&[(
                    "Module1",
                    "Attribute VB_Name = \"Module1\"\r\n\
                     Option Explicit\r\n\
                     \r\n\
                     ' Runs when the button on the ribbon is pressed.\r\n\
                     Public Sub StampToday()\r\n    \
                         Dim where As Range\r\n    \
                         Set where = ActiveDocument.Content\r\n    \
                         where.Collapse wdCollapseEnd\r\n    \
                         where.InsertAfter Format(Date, \"d MMMM yyyy\")\r\n\
                     End Sub\r\n\
                     \r\n\
                     Private Sub Unused()\r\n\
                     End Sub\r\n",
                )]))
                .ok();
                self.carries_macros = self.vba.is_some();
                // The bar that asks, as it is when a document is opened.
                self.info_bar = self.should_offer_macros().then(|| {
                    crate::chrome::infobar::InfoBar::new(crate::chrome::infobar::Because::Macros)
                });
                self.relayout();
                self.run(crate::chrome::Command::Macros);
                let at = self.popup.as_ref().and_then(|popup| {
                    (0..8).find(|index| {
                        popup.item(*index).is_some_and(|line| line.starts_with("Show:"))
                    })
                });
                if let Some(at) = at {
                    self.choose_macro(at);
                }
            }
            // Word's Visual Basic editor, on a document that carries a macro,
            // with a breakpoint in the margin and the macro stopped on it.
            "basic" => {
                self.vba = wp_vba::Project::open(&wp_vba::example(&[(
                    "Module1",
                    "Attribute VB_Name = \"Module1\"\r\n\
                     Option Explicit\r\n\
                     \r\n\
                     ' Stamps today's date at the end of the document.\r\n\
                     Public Sub StampToday()\r\n    \
                         Dim where As Object\r\n    \
                         Set where = ActiveDocument.Content\r\n    \
                         where.InsertAfter vbCrLf & Format(Date, \"d MMMM yyyy\")\r\n    \
                         MsgBox \"Stamped \" & ActiveDocument.Paragraphs.Count & \" paragraphs\"\r\n\
                     End Sub\r\n",
                )]))
                .ok();
                self.carries_macros = self.vba.is_some();
                self.open_basic();
                if let Some(pane) = &mut self.basic {
                    pane.breakpoints.insert(8);
                    pane.caret = (7, 4);
                }
                self.handle(wp_shell::Event::KeyDown {
                    key: wp_shell::Key::Function(5),
                    modifiers: wp_shell::Modifiers::default(),
                });
                for _ in 0..50 {
                    self.handle(wp_shell::Event::Tick);
                    if self.basic.as_ref().and_then(|pane| pane.stopped).is_some() {
                        break;
                    }
                }
            }
            // A macro's own form, up over the document, waiting to be
            // filled in: a label, a box, a tick, a list and two buttons,
            // where the form's author put them.
            "userform" => {
                use wp_vba::forms::{Control, Form, Kind};
                let mut form = Form::new("frmLetter");
                form.caption = "New letter".to_owned();
                form.width = 264.0;
                form.height = 150.0;
                form.controls.push(
                    Control::new("lblWho", Kind::Label, 12.0, 14.0, 60.0, 12.0)
                        .captioned("Addressed to"),
                );
                let mut who = Control::new("txtWho", Kind::TextBox, 78.0, 12.0, 168.0, 18.0);
                who.tab_index = 0;
                form.controls.push(who);
                form.controls.push(
                    Control::new("lblTone", Kind::Label, 12.0, 40.0, 60.0, 12.0)
                        .captioned("Opening"),
                );
                let mut tone = Control::new("cboTone", Kind::ComboBox, 78.0, 38.0, 168.0, 18.0);
                tone.tab_index = 1;
                form.controls.push(tone);
                let mut copy = Control::new("chkCopy", Kind::CheckBox, 78.0, 64.0, 168.0, 18.0)
                    .captioned("Keep a copy in the file");
                copy.tab_index = 2;
                form.controls.push(copy);
                let mut ok = Control::new("cmdOK", Kind::CommandButton, 108.0, 110.0, 66.0, 24.0)
                    .captioned("OK");
                ok.default = true;
                ok.tab_index = 3;
                form.controls.push(ok);
                let mut cancel =
                    Control::new("cmdCancel", Kind::CommandButton, 180.0, 110.0, 66.0, 24.0)
                        .captioned("Cancel");
                cancel.cancel = true;
                cancel.tab_index = 4;
                form.controls.push(cancel);

                self.vba = wp_vba::Project::open(&wp_vba::example_with_forms(
                    &[
                        (
                            "Module1",
                            "Public Sub NewLetter()\r\n    frmLetter.Show\r\nEnd Sub\r\n",
                        ),
                        (
                            "frmLetter",
                            "Private Sub UserForm_Initialize()\r\n\
                             \x20   cboTone.AddItem \"Dear\"\r\n\
                             \x20   cboTone.AddItem \"Hello\"\r\n\
                             \x20   cboTone.ListIndex = 0\r\n\
                             \x20   txtWho.Text = \"Ms Okafor\"\r\n\
                             \x20   chkCopy.Value = True\r\n\
                             End Sub\r\n\
                             Private Sub cmdOK_Click()\r\n\
                             \x20   Selection.TypeText cboTone.Text & \" \" & txtWho.Text & \",\"\r\n\
                             \x20   Me.Hide\r\n\
                             End Sub\r\n\
                             Private Sub cmdCancel_Click()\r\n\
                             \x20   Me.Hide\r\n\
                             End Sub\r\n",
                        ),
                    ],
                    &[form],
                ))
                .ok();
                self.carries_macros = self.vba.is_some();
                self.enabled_here = true;
                self.relayout();
                self.launch("Module1.NewLetter", Vec::new(), false);
            }
            // The XML Mapping pane beside a document that carries an order,
            // with the customer's name bound into a control in the text.
            "mapping" => {
                self.set_view_option("tab=developer")?;
                self.add_custom_xml_text(
                    "<o:order xmlns:o=\"http://example.com/order\" o:number=\"4471\">\r\n  \
                     <o:customer>Habgood &amp; Daughters</o:customer>\r\n  \
                     <o:item>Two dozen brass hinges</o:item>\r\n  \
                     <o:item>A tin of linseed oil</o:item>\r\n  \
                     <o:due>2026-10-01</o:due>\r\n</o:order>",
                );
                self.open_mapping();
                self.mapping_row = Some(2);
                let end = self.document.paragraph_text(3).unwrap_or_default().chars().count();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.document.type_text("  Order for ");
                self.choose_mapped_control(1);
                self.relayout();
            }
            // And a macro's message box, with Word's Yes and No on it.
            "msgbox" => {
                self.vba = wp_vba::Project::open(&wp_vba::example(&[(
                    "Module1",
                    "Public Sub Ask()\r\n\
                     \x20   If MsgBox(\"Stamp today's date at the end?\", vbYesNo + vbQuestion, \"Stamp\") = vbYes Then\r\n\
                     \x20       ActiveDocument.Content.InsertAfter Format(Date, \"d MMMM yyyy\")\r\n\
                     \x20   End If\r\n\
                     End Sub\r\n",
                )]))
                .ok();
                self.carries_macros = self.vba.is_some();
                self.enabled_here = true;
                self.relayout();
                self.launch("Module1.Ask", Vec::new(), false);
            }
            "recovery" => {
                let entries = vec![
                    super::autorecover::Recovered {
                        name: "Annual report.docx".to_owned(),
                        original: Some(std::path::PathBuf::from(
                            "/home/somebody/Annual report.docx",
                        )),
                        saved: "2026-09-16T09:41:00Z".to_owned(),
                        copy: std::path::PathBuf::from("/tmp/recovery/1-1.docx"),
                    },
                    super::autorecover::Recovered {
                        name: "Letter to the bank.docx".to_owned(),
                        original: None,
                        saved: "2026-09-16T09:12:00Z".to_owned(),
                        copy: std::path::PathBuf::from("/tmp/recovery/1-2.docx"),
                    },
                ];
                self.show_recovered(entries);
                if let Some(pane) = &mut self.recovery {
                    pane.opened = Some(0);
                }
                self.relayout();
            }
            // The File Conversion dialog over a text file whose bytes do not
            // say what they are, and the one for saving as text.
            "textopen" => {
                let (bytes, _) = wp_text::Encoding::CodePage(1251).encode(
                    "Привет, мир!
Это текстовый файл в кодировке Windows-1251.
",
                    false,
                );
                self.open_text_path(std::path::Path::new("letter.txt"), bytes);
            }
            "textsave" => {
                self.begin_text_save(std::path::Path::new("letter.txt"));
            }
            // Word's Templates and Add-ins, off the Developer tab.
            "templates" => {
                self.open_templates_dialog();
            }
            // Word's Convert File, over a file of rich text, as "Confirm file
            // format conversion on open" asks it.
            "convert" => {
                self.ask_conversion(
                    std::path::Path::new("letter.rtf"),
                    b"{\\rtf1\\ansi Dear reader\\par}".to_vec(),
                    super::conversion::Kind::Rtf,
                );
            }
            "table" => {
                self.document.insert_table(3, 3);
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
                self.relayout();
            }
            // The button that puts a row in, beside the line it is about, with
            // the pointer where a person would have to put it to see it.
            "tablespot" => {
                self.document.insert_table(3, 3);
                self.relayout();
                if let Some((_, cell)) = self.placed_cell(1, 0) {
                    let (origin_x, origin_y) = self.page_origin(0);
                    let top = self.content_top() + origin_y - self.scroll_down();
                    self.pointer_x = origin_x + cell.x - 12.0;
                    self.pointer_y = top + cell.y;
                }
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
                self.relayout();
            }
            // A bulleted list in a narrow cell, whose indent leaves less room
            // than the word needs: the word is cut rather than drawn across the
            // border of the cell.
            "cellbullet" => {
                self.document.insert_table(4, 4);
                self.relayout();
                if let Some((paragraph, _)) = self.document.cell_paragraphs(3, 0) {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.run(crate::chrome::Command::Bullets);
                    self.document.type_text("укецукецуке");
                }
                if let Some((paragraph, _)) = self.document.cell_paragraphs(0, 1) {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text("Averylongwordwithnospacesatall");
                }
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
                self.relayout();
            }
            // A block of cells taken by dragging across them, which Word shows
            // as the cells themselves and not as the text in them.
            "cellblock" => {
                self.document.insert_table(3, 3);
                self.relayout();
                for row in 0..3 {
                    for column in 0..3 {
                        let Some((paragraph, _)) = self.document.cell_paragraphs(row, column)
                        else {
                            continue;
                        };
                        self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                        self.document.type_text(&format!("Cell {row}{column}"));
                    }
                }
                self.relayout();
                if let Some((paragraph, _)) = self.document.cell_paragraphs(0, 0) {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.cell_anchor = Some((0, 0));
                    let (x, y) = self.cell_middle_for_scene(1, 1);
                    self.extend_cell_drag(x, y);
                }
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
                self.relayout();
            }
            "fittedtable" => {
                // A table fitted to what is in it, which is the only way to see
                // that the columns are the answer to the text rather than to
                // the file. Each cell holds a different word, so each column
                // comes out a different width.
                self.document.insert_table(3, 3);
                for (paragraph, word) in
                    [(1, "One"), (2, "A longer one"), (3, "Mid"), (4, "Two"), (5, "Short")]
                {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text(word);
                }
                self.document.set_caret(wp_docx::TextPosition::new(1, 0));
                self.choose_autofit(0);
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
                self.relayout();
            }
            "cellalignment" => {
                // A tall row with its three cells sitting at the top, in the
                // middle and at the foot of it — the other half of the question
                // Word's nine alignment buttons answer.
                self.document.insert_table(1, 3);
                for (paragraph, text, which) in [
                    (1, "Top", wp_docx::table_properties::CellAlignment::Top),
                    (2, "Middle", wp_docx::table_properties::CellAlignment::Middle),
                    (3, "Bottom", wp_docx::table_properties::CellAlignment::Bottom),
                ] {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text(text);
                    // One cell at a time: the button sets the whole row, and a
                    // picture of three rows alike would show nothing.
                    self.document.set_cell_alignment(which);
                }
                self.document.set_caret(wp_docx::TextPosition::new(1, 0));
                self.document.set_table_row_height(Some(1400), true);
                self.relayout();
            }
            "formula" => {
                // A table with a total under a column of figures, which is what
                // a formula is for, and the dialog that puts one there.
                self.document.insert_table(4, 2);
                for (paragraph, text) in [
                    (1, "Item"),
                    (2, "Cost"),
                    (3, "Pens"),
                    (4, "3.00"),
                    (5, "Paper"),
                    (6, "4.50"),
                    (7, "Total"),
                ] {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text(text);
                }
                self.document.set_caret(wp_docx::TextPosition::new(8, 0));
                self.relayout();
                self.open_formula();
            }
            "formulaanswer" => {
                // The same table with the formula put in, which is the only way
                // to see the total drawn in the cell.
                self.set_view_option("formula")?;
                if let Some(dialog) = self.dialog.take() {
                    self.asking = None;
                    self.apply_formula(&dialog);
                }
                self.relayout();
            }
            "sort" => {
                // Word's Sort on a table of names and figures, which is what
                // it is for. The dialog cannot be photographed without one.
                self.document.insert_table(4, 2);
                for (paragraph, text) in [
                    (1, "Name"),
                    (2, "Score"),
                    (3, "Pear"),
                    (4, "10"),
                    (5, "Apple"),
                    (6, "9"),
                    (7, "Cherry"),
                    (8, "24"),
                ] {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text(text);
                }
                self.document.set_caret(wp_docx::TextPosition::new(1, 0));
                self.relayout();
                self.open_sort();
            }
            "spacedtable" => {
                // A table whose cells are held apart, with a good deal of room
                // inside them as well. The only way to see that the spacing is
                // a geometry and not a number: each cell is drawn with a border
                // of its own and the paper shows between them.
                self.document.insert_table(3, 3);
                for (paragraph, word) in
                    [(1, "One"), (2, "Two"), (3, "Three"), (4, "Four"), (5, "Five"), (6, "Six")]
                {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text(word);
                }
                self.document.set_caret(wp_docx::TextPosition::new(1, 0));
                self.document.set_table_cell_spacing(Some(120));
                self.document.set_table_cell_margins(wp_docx::model::CellMargins {
                    top: Some(120),
                    start: Some(160),
                    bottom: Some(120),
                    end: Some(160),
                });
                self.relayout();
            }
            "turnedtable" => {
                // A table with its headings turned, which is what a narrow
                // column is for. One turned each way, so both are in the
                // picture, and a row of ordinary text under them to show the
                // row is as tall as the turned text is long.
                self.document.insert_table(2, 3);
                for (paragraph, word) in [
                    (1, "Turned down"),
                    (2, "Turned up"),
                    (3, "Across"),
                    (4, "One"),
                    (5, "Two"),
                    (6, "Three"),
                ] {
                    self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                    self.document.type_text(word);
                }
                self.document.set_caret(wp_docx::TextPosition::new(1, 0));
                self.turn_cell_text();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.turn_cell_text();
                self.turn_cell_text();
                self.document.set_caret(wp_docx::TextPosition::new(1, 0));
                // Fitted to its contents as well, which is what a table of
                // turned headings is for: the columns come out as narrow as the
                // headings are deep rather than as long as they are.
                self.choose_autofit(0);
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
                self.relayout();
            }
            "styledtable" => {
                // A table with one of Word's styles on it, which is the only
                // way to look at what the Table Style Options do.
                self.document.insert_table(4, 3);
                self.choose_table_style(4);
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableDesign;
                self.relayout();
            }
            "furniture" => {
                self.document
                    .set_furniture(
                        wp_docx::furniture::Furniture::Header,
                        wp_docx::furniture::Preset::Text,
                        wp_docx::model::Alignment::Center,
                        "Quarterly report",
                    )
                    .map_err(|error| error.to_string())?;
                self.document
                    .set_furniture(
                        wp_docx::furniture::Furniture::Footer,
                        wp_docx::furniture::Preset::PageOfTotal,
                        wp_docx::model::Alignment::Center,
                        "",
                    )
                    .map_err(|error| error.to_string())?;
                self.relayout();
            }
            "borders" => {
                self.document.set_borders_here(&wp_docx::model::ParagraphBorders::box_all());
                self.document.set_shading_here(Some("2B579A"));
                self.relayout();
            }
            "pagebordersdialog" => {
                self.open_page_borders();
            }
            "pagebordersart" => {
                // The same dialog with a pattern picked out of the Art gallery,
                // which is the only way to see that the widths beside it have
                // changed to the unit art is measured in.
                self.open_page_borders();
                let row = wp_docx::art::DRAWN
                    .iter()
                    .position(|(_, name)| *name == "checkered")
                    .unwrap_or_default()
                    + 1;
                if let Some(dialog) = &mut self.dialog {
                    if let Some(crate::chrome::dialog::Field::Choice { current, .. }) =
                        dialog.fields.get_mut(super::pagebordersdialog::ART)
                    {
                        *current = row;
                    }
                }
                self.page_borders_changed();
            }
            "wordcount" => {
                self.open_word_count();
            }
            "pagesetup" => {
                self.open_page_setup();
            }
            "bookmark" => {
                self.start_bookmark();
            }
            "paragraphdialog" => {
                self.open_paragraph_dialog();
            }
            "paragraphbreaks" => {
                self.open_paragraph_dialog();
                self.dialog_key(wp_shell::Key::Tab, false, true);
            }
            "tabsdialog" => {
                self.open_tabs_dialog();
            }
            "stylespane" => {
                self.toggle_styles_pane();
            }
            "newstyle" => {
                self.toggle_styles_pane();
                self.open_new_style();
            }
            "inspector" => {
                self.open_style_inspector();
            }
            "symbols" => {
                self.open_symbol_dialog();
            }
            "special" => {
                self.open_symbol_dialog();
                self.dialog_key(wp_shell::Key::Tab, false, true);
            }
            // Word's Layout, on a shape: three tabs of where it sits, what the
            // text does about it, and how big it is.
            // A drawing with room either side of it, which is where text runs
            // down both sides: Word's own default wrapping.
            "bothsides" => {
                let anchor = wp_docx::anchor::Anchor {
                    wrap: wp_docx::anchor::Wrap::Square,
                    horizontal: wp_docx::anchor::Placement::Offset(1_828_800),
                    vertical: wp_docx::anchor::Placement::Offset(228_600),
                    ..wp_docx::anchor::Anchor::default()
                };
                let shape = wp_docx::shapes::Shape::preset("rect", 126.0, 108.0).floating(anchor);
                self.document.set_caret(wp_docx::TextPosition::new(3, 0));
                self.document.insert_shape(&shape);
                self.relayout();
            }
            "layout" | "layoutwrap" | "layoutsize" => {
                let shape = wp_docx::shapes::Shape::preset("rect", 144.0, 72.0);
                self.document.insert_shape(&shape);
                self.relayout();
                if let Some(at) = self.document.drawing_place_here() {
                    self.chosen_drawings = vec![at];
                }
                self.open_layout_dialog();
                // The other two tabs, reached the way the keyboard reaches
                // them: a picture of the first tab shows a third of a dialog.
                let tabs = match option {
                    "layoutwrap" => 1,
                    "layoutsize" => 2,
                    _ => 0,
                };
                for _ in 0..tabs {
                    self.dialog_key(wp_shell::Key::Tab, false, true);
                }
            }
            "tableprops" => {
                self.document.insert_table(3, 3);
                self.relayout();
                self.open_table_properties();
            }
            // A table with every cell filled in and its top row merged: the
            // merged cell holds what all three held, one paragraph each.
            "mergedrow" => {
                self.fill_a_table_for_a_picture();
                self.select_cells((0, 0), (0, 2));
                self.run(crate::chrome::Command::MergeCells);
                self.relayout();
            }
            // Word's Split Cells, over two cells of such a table taken
            // together, which is when it offers to merge them first.
            "splitcells" => {
                self.fill_a_table_for_a_picture();
                self.select_cells((1, 1), (0, 1));
                self.relayout();
                self.run(crate::chrome::Command::SplitCells);
            }
            // Such a table with the caret in its first cell and Shift+Down
            // pressed twice, as a person presses it: the first column taken,
            // and drawn as the selection cell by cell.
            "shiftdown" => {
                self.fill_a_table_for_a_picture();
                for _ in 0..2 {
                    self.handle(wp_shell::Event::KeyDown {
                        key: wp_shell::Key::Down,
                        modifiers: wp_shell::Modifiers {
                            shift: true,
                            ..wp_shell::Modifiers::default()
                        },
                    });
                }
            }
            "options" => {
                self.open_options();
            }
            // The Save page of Options: how often a copy of the work is
            // taken, and where it goes.
            "savepage" => {
                self.open_options();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(3);
                }
            }
            // The page where the language of the interface is chosen.
            "languagepage" => {
                self.open_options();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(2);
                }
            }
            // The Trust Center: what macros may do, and the two lists that
            // say yes before that is asked. With `settings=` before it, the
            // lists hold what that file holds.
            "trustpage" => {
                self.open_options();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(super::optionsdialog::TAB_TRUST_PAGE);
                }
            }
            "quickaccess" | "customribbon" => {
                // The two pages of Options that are two lists side by side,
                // which is the only part of that dialog a picture of the first
                // tab does not show.
                self.open_options();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(if option == "quickaccess" {
                        super::ribbondialog::QUICK_PAGE
                    } else {
                        super::ribbondialog::RIBBON_PAGE
                    });
                }
            }
            "floatingpicture" => {
                // A picture off the line with the text flowing round it, which
                // is the one thing a picture could not do until now. The
                // picture is made here rather than read from a file: a proof
                // should need nothing beside the program.
                let mut canvas = wp_raster::Canvas::filled(120, 90, wp_raster::Color::WHITE);
                for y in 0..90i32 {
                    for x in 0..120i32 {
                        let shade = (x * 2) as u8;
                        canvas.fill_rect(x, y, 1, 1, wp_raster::Color::rgb(shade, 0x70, 0xC0));
                    }
                }
                let bytes = wp_raster::encode_png(&canvas);
                self.document.set_caret(wp_docx::TextPosition::new(4, 0));
                let _ = self.document.insert_picture(&bytes, "png", 1_143_000, 857_250);

                // Beside the picture, and then floated with the text wrapped
                // square round it.
                self.document.set_caret(wp_docx::TextPosition::new(4, 1));
                let anchor = wp_docx::anchor::Anchor {
                    wrap: wp_docx::anchor::Wrap::Square,
                    horizontal: wp_docx::anchor::Placement::Aligned("right".to_owned()),
                    vertical: wp_docx::anchor::Placement::Offset(0),
                    ..wp_docx::anchor::Anchor::default()
                };
                self.document.set_anchor_here(Some(&anchor));
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.relayout();
            }
            "drawtable" => {
                // A table with a line drawn down one cell and another rubbed
                // out between two. The only way to see that the two pens act
                // on the cells and not on the text.
                use super::borderpainter::TablePen;
                self.document.insert_table(3, 3);
                self.ribbon.tab = Tab::TableLayout;
                self.relayout();
                wp_shell::App::draw(self, 1400, 900);

                self.toggle_table_pen(TablePen::Draw);
                if let Some((x, y)) = self.middle_of_cell(4) {
                    self.table_pen_press(x, y);
                    self.table_pen_release(x, y + 40);
                }
                self.toggle_table_pen(TablePen::Erase);
                if let Some((x, y)) = self.left_edge_of_cell(7) {
                    self.table_pen_press(x, y);
                }
                self.relayout();
            }
            "borderpen" => {
                // A table with three of its edges ruled by the pen, and the
                // gallery open over it. The only way to see that the pen puts
                // a line where it is dragged and nowhere else.
                self.document.insert_table(3, 3);
                self.ribbon.tab = Tab::TableDesign;
                self.relayout();
                wp_shell::App::draw(self, 1400, 900);

                self.choose_border_style(13);
                for cell in [6usize, 7, 8] {
                    let Some((x, y)) = self.top_edge_of_cell(cell) else { continue };
                    self.paint_border_at(x, y);
                }
                self.open_border_styles();
            }
            "borderstyles" => {
                // A paragraph for each of the kinds of line Word lists, and a
                // shadowed box round the pages. The only way to see that the
                // list of styles is a list of choices rather than one line
                // written down twenty-five ways.
                use wp_docx::model::{Border, ParagraphBorders};
                for (at, style) in
                    ["double", "triple", "dotDash", "wave", "threeDEmboss"].into_iter().enumerate()
                {
                    self.document.set_caret(wp_docx::TextPosition::new(at * 2 + 3, 0));
                    let line = Border::line(style, 12, Some("2B579A"));
                    self.document.set_borders_here(&ParagraphBorders::box_all_of(&line));
                }
                let line = Border::line("single", 12, Some("C00000")).with_effect(true, false);
                self.document.set_page_borders_everywhere(
                    &wp_docx::pageborders::PageBorders::box_all(&line),
                );
                self.relayout();
            }
            "similar" => {
                // Every stretch set the way the first heading is set, all
                // selected at once. The only way to see that a selection can
                // now be made of more than one piece.
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.select_similar();
                self.relayout();
            }
            "showmarkup" => {
                // A document where somebody reformatted a line while changes
                // were being recorded, with Word's Show Markup menu open over
                // it. The only way to see both the mark and the three switches.
                self.document.set_tracking_changes(true);
                self.document.set_caret(wp_docx::TextPosition::new(4, 0));
                let end = self.document.paragraph_text(4).unwrap_or_default().len();
                self.document.move_caret(wp_docx::TextPosition::new(4, end), true);
                self.document.set_format(wp_docx::CharacterFormat::Bold, true);
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.ribbon.tab = Tab::Review;
                self.relayout();
                // Drawn before the menu is opened, because a menu hangs under a
                // button and a button that has never been drawn is nowhere.
                wp_shell::App::draw(self, 1400, 900);
                self.open_markup_menu();
            }
            "overlap" => {
                // Two drawings on top of one another, the second brought in
                // front of the text. The only way to see that the order the
                // anchors carry is the order they are drawn in.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                for (name, fill, offset) in
                    [("Behind", "C00000", 0i64), ("In front", "70AD47", 457_200)]
                {
                    let shape = wp_docx::shapes::Shape {
                        name: name.to_owned(),
                        width_emu: 1_828_800,
                        height_emu: 914_400,
                        fill: wp_docx::fills::Fill::solid(fill),
                        text: vec![wp_docx::model::Paragraph::text(name)],
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset(offset),
                            vertical: Placement::Offset(offset),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.insert_shape(&shape);
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                }
                self.relayout();
            }
            "severaldrawings" | "aligned" | "alignmenu" => {
                // Three drawings chosen at once, which is what Align is for.
                // "aligned" lines them up; "alignmenu" drops the menu open.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                for (name, across, down) in [
                    ("One", 0i64, 0i64),
                    ("Two", 1_371_600, 685_800),
                    ("Three", 2_743_200, 1_371_600),
                ] {
                    let shape = wp_docx::shapes::Shape {
                        name: name.to_owned(),
                        width_emu: 914_400,
                        height_emu: 548_640,
                        fill: wp_docx::fills::Fill::solid("4472C4"),
                        text: vec![wp_docx::model::Paragraph::text(name)],
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset(across),
                            vertical: Placement::Offset(down),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();

                let all: Vec<wp_docx::TextPosition> =
                    self.drawings_facing().into_iter().map(|drawing| drawing.at).collect();
                for (at, place) in all.into_iter().enumerate() {
                    if at == 0 {
                        self.choose_drawing_at(place);
                    } else {
                        self.also_choose_drawing_at(place);
                    }
                }
                self.ribbon.tab = crate::chrome::ribbon::Tab::Layout;
                if option == "aligned" {
                    let row = crate::editor::align::ROWS
                        .iter()
                        .position(|(label, _)| *label == "Align Left")
                        .unwrap_or_default();
                    self.choose_align(row);
                }
                self.relayout();
                if option == "alignmenu" {
                    // The menu hangs under its button, and where the button is
                    // is only known once the tab it is on has been drawn.
                    let (width, height) = (self.view_width, self.view_height);
                    self.draw(width, height);
                    self.open_align();
                }
            }
            "chosendrawing" => {
                // A drawing chosen, which is the only way to see the eight
                // handles: they are drawn round the drawing that is selected
                // and round nothing else. The picture beside it is there to
                // show that the handles belong to one drawing and not to
                // whatever the caret happens to be near.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                let shape = wp_docx::shapes::Shape {
                    name: "Rectangle".to_owned(),
                    width_emu: 1_828_800,
                    height_emu: 1_143_000,
                    fill: wp_docx::fills::Fill::solid("4472C4"),
                    text: vec![wp_docx::model::Paragraph::text("Chosen")],
                    anchor: Some(Anchor {
                        wrap: Wrap::Square,
                        horizontal: Placement::Offset(457_200),
                        vertical: Placement::Offset(228_600),
                        ..Anchor::default()
                    }),
                    ..wp_docx::shapes::Shape::default()
                };
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.insert_shape(&shape);
                self.choose_drawing_here();
                self.relayout();
            }
            "turned" | "rotatemenu" => {
                // Drawings at angles: shapes turned by a quarter, by an eighth
                // and mirrored, and a picture turned with them — the only way
                // to see that the geometry, the words inside it and the pixels
                // all go round together. "rotatemenu" drops the menu open over
                // the same page.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::floating::Turned;
                for (name, rotation, across, mirrored) in [
                    ("Straight", 0, 0i64, false),
                    ("Quarter", Turned::WHOLE / 4, 1_371_600, false),
                    ("Eighth", Turned::WHOLE / 8, 2_743_200, false),
                    ("Mirror", Turned::WHOLE / 8, 4_114_800, true),
                ] {
                    let shape = wp_docx::shapes::Shape {
                        name: name.to_owned(),
                        width_emu: 1_143_000,
                        height_emu: 685_800,
                        fill: wp_docx::fills::Fill::solid("4472C4"),
                        text: vec![wp_docx::model::Paragraph::text(name)],
                        rotation,
                        flipped_across: mirrored,
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset(across),
                            vertical: Placement::Offset(0),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }

                // And a picture, turned an eighth of a turn by the same
                // command a person would use: the pixels have to follow the
                // box or the box is a lie.
                let mut canvas = wp_raster::Canvas::filled(120, 90, wp_raster::Color::WHITE);
                for y in 0..90i32 {
                    for x in 0..120i32 {
                        let shade = (x * 2) as u8;
                        canvas.fill_rect(x, y, 1, 1, wp_raster::Color::rgb(shade, 0x70, 0xC0));
                    }
                }
                let bytes = wp_raster::encode_png(&canvas);
                self.document.set_caret(wp_docx::TextPosition::new(6, 0));
                let _ = self.document.insert_picture(&bytes, "png", 1_143_000, 857_250);
                let at = wp_docx::TextPosition::new(6, 0);
                self.document.set_drawing_turn_at(
                    at,
                    Turned { rotation: Turned::WHOLE / 8, ..Turned::default() },
                );
                self.relayout();

                self.choose_drawing_at(at);
                self.ribbon.tab = crate::chrome::ribbon::Tab::Layout;
                if option == "rotatemenu" {
                    // The menu hangs under its button, and where the button is
                    // is only known once the tab it is on has been drawn.
                    let (width, height) = (self.view_width, self.view_height);
                    self.draw(width, height);
                    self.open_rotate();
                }
            }
            "grouped" | "groupmenu" => {
                // Three drawings made one: the handles round the group and not
                // round each of them is the whole of what a group looks like.
                // "groupmenu" drops the menu open over the same page.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::group::Rect;
                for (name, fill, across, down) in [
                    ("One", "4472C4", 0i64, 0i64),
                    ("Two", "ED7D31", 1_143_000, 457_200),
                    ("Three", "70AD47", 2_286_000, 0),
                ] {
                    let shape = wp_docx::shapes::Shape {
                        name: name.to_owned(),
                        width_emu: 914_400,
                        height_emu: 685_800,
                        fill: wp_docx::fills::Fill::solid(fill),
                        text: vec![wp_docx::model::Paragraph::text(name)],
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset(across),
                            vertical: Placement::Offset(down),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();

                // Grouped through the document rather than through the command,
                // so the scene does not depend on what happens to be chosen.
                let places: Vec<(wp_docx::TextPosition, Rect)> = self
                    .drawings_facing()
                    .into_iter()
                    .map(|drawing| {
                        let scale = self.pixels_per_inch() / 72.0;
                        let emu = |pixels: f32| {
                            (f64::from(pixels) / f64::from(scale)
                                * wp_docx::shapes::EMU_PER_POINT as f64)
                                .round() as i64
                        };
                        (
                            drawing.at,
                            Rect {
                                x: emu(drawing.left),
                                y: emu(drawing.top),
                                width: emu(drawing.width),
                                height: emu(drawing.height),
                            },
                        )
                    })
                    .collect();
                if let Some(at) = self.document.group_drawings(&places) {
                    self.relayout();
                    self.choose_drawing_at(at);
                }
                self.ribbon.tab = crate::chrome::ribbon::Tab::Layout;
                if option == "groupmenu" {
                    // The menu hangs under its button, and where the button is
                    // is only known once the tab it is on has been drawn.
                    let (width, height) = (self.view_width, self.view_height);
                    self.draw(width, height);
                    self.open_grouping();
                }
            }
            // The block arrows alone, big enough to see whether each is the
            // shape it is named after.
            "arrows" => {
                let arrows: Vec<wp_layout::geometry::Preset> = wp_layout::geometry::Preset::all()
                    .into_iter()
                    .filter(|preset| {
                        let word = preset.word();
                        word.contains("Arrow")
                            || word.contains("arrow")
                            || word == "homePlate"
                            || word == "chevron"
                    })
                    .collect();
                self.shape_grid(&arrows, 6);
            }
            // And the flowchart shapes alone. Most of them are the same box
            // with one edge changed, so the only way to tell whether one is
            // right is to look at it beside the others.
            // Three shapes from the gallery with words in them, under the
            // Red colour scheme: the fill, the line and the words all follow
            // the theme, because none of them was ever a colour.
            "themed" => {
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::model::Paragraph;

                let red = wp_docx::gallery::COLOR_SCHEMES
                    .iter()
                    .find(|scheme| scheme.name == "Red")
                    .expect("the Red scheme");
                let _ = self.document.set_theme(&self.document.theme().with_colors(red));
                for (index, (preset, words)) in
                    [("rect", "Accent"), ("ellipse", "Shade"), ("roundRect", "Words")]
                        .iter()
                        .enumerate()
                {
                    let mut shape = wp_docx::shapes::Shape::preset(preset, 108.0, 72.0);
                    shape.text = vec![Paragraph::text(words)];
                    shape.anchor = Some(Anchor {
                        wrap: Wrap::None,
                        horizontal: Placement::Offset(index as i64 * 1_600_200),
                        vertical: Placement::Offset(0),
                        ..Anchor::default()
                    });
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();
            }
            // The Draw tab with the pen in hand: a stroke that swells with
            // the pen's pressure, a highlighter over the words, and a stroke
            // being drawn under the pointer.
            "draw" => {
                use wp_docx::anchor::{Anchor, Placement, Relative, Wrap};
                use wp_docx::ink::{Ink, Stroke};

                let floats = |across: i64, down: i64| Anchor {
                    wrap: Wrap::None,
                    horizontal_from: Relative::Page,
                    horizontal: Placement::Offset(across),
                    vertical_from: Relative::Paragraph,
                    vertical: Placement::Offset(down),
                    allow_overlap: true,
                    ..Anchor::default()
                };
                // A wave whose pressure rises and falls along it.
                let wave: Vec<(i64, i64)> = (0..=60)
                    .map(|step| {
                        let along = step as f64 / 60.0;
                        (
                            (along * 2_400_000.0) as i64,
                            (200_000.0 + (along * 12.0).sin() * 150_000.0) as i64,
                        )
                    })
                    .collect();
                let pressure: Vec<f32> = (0..=60)
                    .map(|step| 0.15 + 0.85 * ((step as f32 / 60.0) * core::f32::consts::PI).sin())
                    .collect();
                let pressed = Ink {
                    strokes: vec![Stroke {
                        colour: "C00000".to_owned(),
                        width_emu: 36_000,
                        transparency: 0,
                        flat: false,
                        points: wave,
                        pressure,
                    }],
                };
                let highlight = Ink {
                    strokes: vec![Stroke {
                        colour: "FFFF00".to_owned(),
                        width_emu: 216_000,
                        transparency: 128,
                        flat: true,
                        points: vec![(0, 0), (1_800_000, 0)],
                        pressure: Vec::new(),
                    }],
                };
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                let _ = self.document.insert_ink_floating(&pressed, &floats(1_500_000, 500_000));
                self.document.set_caret(wp_docx::TextPosition::new(3, 0));
                let _ = self.document.insert_ink_floating(&highlight, &floats(1_100_000, 60_000));
                self.relayout();
                self.choose_tab(crate::chrome::ribbon::Tab::Draw);
                self.take_pen(super::inking::PenKind::Pen);
                let (origin_x, origin_y) = self.page_origin(0);
                let top = self.content_top() + origin_y - self.scroll_down();
                let points: Vec<(f32, f32)> = (0..=40)
                    .map(|step| {
                        let along = step as f32 / 40.0;
                        (origin_x + 420.0 + along * 160.0, top + 330.0 + (along * 9.0).sin() * 18.0)
                    })
                    .collect();
                let (x, y) = (points[0].0 as i32, points[0].1 as i32);
                self.ink_press(x, y);
                for (x, y) in &points[1..] {
                    self.ink_move(*x as i32, *y as i32, true);
                }
            }
            // The same justified paragraph twice: with the document set to
            // hyphenate on its own, the words are broken where English breaks
            // them and the lines are even; without, the spaces gape.
            "hyphenation" => {
                let words = "The responsibility of a typographer is the understanding \
                    of the paragraph: information and knowledge are set with \
                    extraordinary care, and the development of a photograph, a \
                    telephone or a government is nothing to the development of \
                    the international university and its beautiful algorithms.";
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.type_text(words);
                self.document.press_enter();
                self.document.type_text(words);
                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(20.0);
                self.document.set_alignment_here(wp_docx::model::Alignment::Both);
                self.document.clear_selection();
                // The second paragraph keeps its words whole, which is what
                // the paragraph's own setting is for.
                self.document.set_caret(wp_docx::TextPosition::new(3, 0));
                let _ = self.document.set_paragraph_format(&wp_docx::model::ParagraphProperties {
                    no_hyphenation: Some(true),
                    ..wp_docx::model::ParagraphProperties::default()
                });
                self.document.set_automatic_hyphenation(true);
                self.document.set_hyphenation_zone(Some(360));
                self.relayout();
            }
            // The newer kinds of colour glyph, from the fonts the build image
            // has fontTools write: a tree of paints of every kind, then the
            // layers of the older kind, an Apple picture and three SVG
            // drawings, each in a line of its own and large enough to see.
            "colour" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                for (family, text) in [
                    ("WP Colour One", "\u{E000}\u{E001}\u{E002}\u{E003}\u{E004}\u{E005}"),
                    ("WP Colour One", "\u{E006}\u{E007}\u{E008}\u{E009}\u{E00A}\u{E00B}\u{E010}"),
                    ("WP Colour Pictures", "\u{E000}\u{E001}\u{E002}"),
                    ("WP Colour Drawings", "\u{E000}\u{E001}\u{E002}"),
                ] {
                    let from = self.document.caret();
                    self.document.type_text(text);
                    let to = self.document.caret();
                    self.document.set_caret(from);
                    self.document.extend_selection_to(to);
                    self.document.set_font(family);
                    self.document.set_size(40.0);
                    self.document.set_caret(to);
                    self.document.press_enter();
                }
                self.relayout();
            }
            // A section written down the page: Japanese standing upright in
            // columns that go from right to left, with a Latin word lying on
            // its side, a year set across the column, and a gloss set as two
            // lines in one.
            "vertical" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.type_text("縦書きの文章は、右から左へ列が進む。");
                self.document.press_enter();
                self.document.type_text("漢字とかなは立ち、Latin は横に寝る。");
                self.document.press_enter();
                self.document.type_text("令和");
                let from = self.document.caret();
                self.document.type_text("12");
                let to = self.document.caret();
                self.document.type_text("年の春、「括弧」も向きを変える……。");
                self.document.set_caret(from);
                self.document.extend_selection_to(to);
                let _ = self.document.set_character_format(&wp_docx::model::RunProperties {
                    east_asian_layout: Some(wp_docx::eastasian::EastAsianLayout {
                        horizontal_in_vertical: true,
                        fit_in_line: true,
                        ..wp_docx::eastasian::EastAsianLayout::default()
                    }),
                    ..wp_docx::model::RunProperties::default()
                });
                self.document.clear_selection();
                self.document.press_enter();
                self.document.type_text("割注は");
                let from = self.document.caret();
                self.document.type_text("二行に分けて");
                let to = self.document.caret();
                self.document.type_text("組む。");
                self.document.set_caret(from);
                self.document.extend_selection_to(to);
                let _ = self.document.set_character_format(&wp_docx::model::RunProperties {
                    east_asian_layout: Some(wp_docx::eastasian::EastAsianLayout {
                        two_lines_in_one: true,
                        brackets: wp_docx::eastasian::CombineBrackets::Round,
                        ..wp_docx::eastasian::EastAsianLayout::default()
                    }),
                    ..wp_docx::model::RunProperties::default()
                });
                self.document.clear_selection();
                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(22.0);
                self.document.clear_selection();
                self.document.set_text_direction(wp_docx::model::TextDirection::Down);
                self.relayout();
            }
            "flowchart" => {
                let shapes: Vec<wp_layout::geometry::Preset> = wp_layout::geometry::Preset::all()
                    .into_iter()
                    .filter(|preset| preset.word().starts_with("flowChart"))
                    .collect();
                self.shape_grid(&shapes, 6);
            }
            // Two boxes with a connector fastened between them, drawn twice:
            // the second pair has its right-hand box somewhere else, and the
            // connector follows it without being told to. Both connectors are
            // saved with the same useless box of their own.
            "connected" => {
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::joins::{Join, Joins};
                use wp_docx::lines::{EndKind, LineEnd};

                let floats = |across: i64, down: i64| Anchor {
                    wrap: Wrap::None,
                    horizontal: Placement::Offset(across),
                    vertical: Placement::Offset(down),
                    ..Anchor::default()
                };
                let mut boxes = Vec::new();
                for (pair, (across, down)) in
                    [(1_600_200i64, 0i64), (2_057_400, 457_200), (-1_600_200, 685_800)]
                        .into_iter()
                        .enumerate()
                {
                    let step = pair as i64 * 1_371_600;
                    let first = wp_docx::shapes::Shape {
                        name: format!("First {pair}"),
                        id: pair as u32 * 10 + 1,
                        preset: "roundRect".to_owned(),
                        width_emu: 1_143_000,
                        height_emu: 457_200,
                        fill: wp_docx::fills::Fill::solid("4472C4"),
                        outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                        outline_emu: 9_525,
                        anchor: Some(floats(1_828_800, step)),
                        ..wp_docx::shapes::Shape::default()
                    };
                    let second = wp_docx::shapes::Shape {
                        name: format!("Second {pair}"),
                        id: pair as u32 * 10 + 2,
                        anchor: Some(floats(1_828_800 + across, step + down)),
                        ..first.clone()
                    };
                    let connector = wp_docx::shapes::Shape {
                        name: format!("Connector {pair}"),
                        id: pair as u32 * 10 + 3,
                        preset: "bentConnector3".to_owned(),
                        width_emu: 228_600,
                        height_emu: 228_600,
                        fill: wp_docx::fills::Fill::None,
                        outline: Some(wp_docx::colour::Colour::rgb("C00000")),
                        outline_emu: 19_050,
                        tail_end: LineEnd { kind: EndKind::Triangle, ..LineEnd::default() },
                        joins: Joins {
                            start: Some(Join { shape: pair as u32 * 10 + 1, site: 3 }),
                            end: Some(Join { shape: pair as u32 * 10 + 2, site: 1 }),
                        },
                        // Saved somewhere useless on purpose: what it is
                        // fastened to is what says where it goes.
                        anchor: Some(floats(4_572_000, 4_572_000)),
                        ..first.clone()
                    };
                    boxes.push(first);
                    boxes.push(second);
                    boxes.push(connector);
                }
                for shape in &boxes {
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(shape);
                }
                self.relayout();
            }
            // The lines and connectors, each with an arrowhead on its tail, and
            // then one line drawn with each of the six things the format can
            // put at the end of one.
            "connectors" => {
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::lines::{EndKind, LineEnd};
                use wp_layout::geometry::Preset;

                let lines: Vec<Preset> =
                    Preset::all().into_iter().filter(|preset| !preset.is_closed()).collect();
                let ends = [
                    EndKind::None,
                    EndKind::Triangle,
                    EndKind::Stealth,
                    EndKind::Diamond,
                    EndKind::Oval,
                    EndKind::Arrow,
                ];
                let cells: Vec<(Preset, EndKind)> = lines
                    .iter()
                    .map(|preset| (*preset, EndKind::Triangle))
                    .chain(ends.into_iter().map(|kind| (Preset::StraightConnector, kind)))
                    .collect();

                const CELL: i64 = 914_400;
                let across = 4usize;
                let rows = cells.len().div_ceil(across) as i64;
                let sheet = wp_docx::shapes::Shape {
                    name: "Sheet".to_owned(),
                    preset: "rect".to_owned(),
                    width_emu: across as i64 * CELL,
                    height_emu: rows * CELL,
                    fill: wp_docx::fills::Fill::solid("FFFFFF"),
                    outline: None,
                    anchor: Some(Anchor {
                        wrap: Wrap::None,
                        horizontal: Placement::Offset(0),
                        vertical: Placement::Offset(0),
                        ..Anchor::default()
                    }),
                    ..wp_docx::shapes::Shape::default()
                };
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.insert_shape(&sheet);

                for (index, (preset, kind)) in cells.iter().enumerate() {
                    let shape = wp_docx::shapes::Shape {
                        name: preset.label().to_owned(),
                        preset: preset.word().to_owned(),
                        width_emu: 685_800,
                        height_emu: 457_200,
                        // A line has no inside, so it has no fill and is drawn
                        // thick enough to see what is at the end of it.
                        fill: wp_docx::fills::Fill::None,
                        outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                        outline_emu: 28_575,
                        tail_end: LineEnd { kind: *kind, ..LineEnd::default() },
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset((index % across) as i64 * CELL + 114_300),
                            vertical: Placement::Offset((index / across) as i64 * CELL + 228_600),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();
            }
            // The callouts: the four bubbles and the twelve with a leader. A
            // callout is drawn partly outside its own box, so these are given
            // more room than the other scenes give a shape.
            "callouts" => {
                use wp_layout::geometry::Preset;
                let shapes: Vec<Preset> =
                    Preset::all().into_iter().filter(|preset| preset.is_callout()).collect();
                self.shape_grid(&shapes, 4);
            }
            // One shape at a time with its handle moved: four rounded
            // rectangles from square to a stadium, four stars from a deep dip
            // to none, four arrows with the head growing, and four pies opening
            // round. What this shows is that the shape follows the file.
            "handles" => {
                use wp_layout::geometry::Preset;
                let run = |preset: Preset, name: &str, values: [i32; 4]| {
                    values
                        .into_iter()
                        .map(|value| (preset, vec![(name.to_owned(), value)]))
                        .collect::<Vec<_>>()
                };
                let mut cells = run(Preset::RoundedRectangle, "adj", [0, 8_000, 25_000, 50_000]);
                cells.extend(run(Preset::Star, "adj", [8_000, 19_098, 30_000, 45_000]));
                cells.extend(run(Preset::Arrow, "adj2", [15_000, 35_000, 50_000, 90_000]));
                cells.extend(run(
                    Preset::Pie,
                    "adj2",
                    [2_700_000, 8_100_000, 16_200_000, 21_000_000],
                ));
                self.handle_grid(&cells, 4);
            }
            // One shape drawn with each of the effects the format can put on
            // one: the shadow under it, the shadow inside it, the glow, the
            // soft edge and the reflection, and one with none of them to
            // compare them against.
            "effects" => {
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::shapeeffects::{Effects, Glow, Reflection, Shadow};

                let shadow = |distance: i64, blur: i64| Shadow {
                    colour: wp_docx::colour::Colour::rgb("000000"),
                    alpha: 45_000,
                    blur_emu: blur,
                    distance_emu: distance,
                    direction: 2_700_000,
                };
                let all = [
                    Effects::default(),
                    Effects { outer_shadow: Some(shadow(152_400, 152_400)), ..Effects::default() },
                    Effects { inner_shadow: Some(shadow(101_600, 101_600)), ..Effects::default() },
                    Effects {
                        glow: Some(Glow {
                            colour: wp_docx::colour::Colour::rgb("FF0000"),
                            alpha: 70_000,
                            radius_emu: 228_600,
                        }),
                        ..Effects::default()
                    },
                    Effects { soft_edge_emu: 152_400, ..Effects::default() },
                    Effects {
                        reflection: Some(Reflection {
                            blur_emu: 25_400,
                            start_alpha: 60_000,
                            end_alpha: 300,
                            end_at: 55_000,
                            distance_emu: 0,
                        }),
                        ..Effects::default()
                    },
                ];

                const CELL: i64 = 1_371_600;
                let sheet = wp_docx::shapes::Shape {
                    name: "Sheet".to_owned(),
                    preset: "rect".to_owned(),
                    width_emu: 3 * CELL,
                    height_emu: 2 * CELL + 457_200,
                    fill: wp_docx::fills::Fill::solid("FFFFFF"),
                    outline: None,
                    anchor: Some(Anchor {
                        wrap: Wrap::None,
                        horizontal: Placement::Offset(0),
                        vertical: Placement::Offset(0),
                        ..Anchor::default()
                    }),
                    ..wp_docx::shapes::Shape::default()
                };
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.insert_shape(&sheet);

                for (index, effects) in all.into_iter().enumerate() {
                    let shape = wp_docx::shapes::Shape {
                        name: format!("Effect {index}"),
                        preset: "roundRect".to_owned(),
                        width_emu: 914_400,
                        height_emu: 685_800,
                        fill: wp_docx::fills::Fill::solid("4472C4"),
                        outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                        outline_emu: 9_525,
                        effects,
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset((index % 3) as i64 * CELL + 228_600),
                            vertical: Placement::Offset((index / 3) as i64 * CELL + 228_600),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();
            }
            // A language written without spaces between its words, and what
            // the program can say about where they are.
            "japanese" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.type_text(
                    "\u{79C1}\u{306F}\u{30AC}\u{30E9}\u{30B9}\u{3092}\u{98DF}\u{3079}\u{3089}\
                     \u{308C}\u{307E}\u{3059}\u{3002}\u{305D}\u{308C}\u{306F}\u{79C1}\u{3092}\
                     \u{50B7}\u{3064}\u{3051}\u{307E}\u{305B}\u{3093}\u{3002}",
                );
                self.document.press_enter();
                self.document.type_text("Rust \u{3067}\u{66F8}\u{304F}: a line of both.");

                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(20.0);
                self.document.set_language("ja-JP");
                self.document.clear_selection();
                // What a double click inside the katakana word selects, drawn
                // by selecting exactly what the rules say it is: the word, and
                // not the one character a click on a kanji would take.
                if let Some(text) = self.document.paragraph_text(2) {
                    let word = wp_segment::word_at(&text, 8);
                    self.document.set_caret(wp_docx::TextPosition::new(2, word.start));
                    self.document.extend_selection_to(wp_docx::TextPosition::new(2, word.end));
                }
                self.relayout();
            }
            // The same letters put into capitals, in three languages that
            // disagree about what a capital is.
            "casing" => {
                use wp_docx::page::CaseChange;

                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                for (language, word) in [
                    ("tr-TR", "istanbul"),
                    ("en-GB", "istanbul"),
                    ("el-GR", "\u{03AC}\u{03BD}\u{03B8}\u{03C1}\u{03C9}\u{03C0}\u{03BF}\u{03C2}"),
                    ("de-DE", "stra\u{00DF}e"),
                ] {
                    let start = self.document.caret();
                    self.document.type_text(word);
                    let end = self.document.caret();
                    self.document.set_caret(start);
                    self.document.extend_selection_to(end);
                    self.document.set_language(language);
                    // Word's Aa button: the text itself is rewritten.
                    self.document.change_case(CaseChange::Upper);
                    self.document.clear_selection();
                    self.document.type_text(&format!("  {word} ({language})"));
                    self.document.press_enter();
                }

                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(20.0);
                self.document.clear_selection();
                self.relayout();
            }
            // A word with its reading set over it, every way the file can ask
            // for the two to be lined up.
            "ruby" => {
                use wp_docx::ruby::{Align, Ruby};

                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                for (align, label) in [
                    (Align::DistributeSpace, " spread  "),
                    (Align::Center, " centred  "),
                    (Align::Left, " left  "),
                    (Align::Right, " right  "),
                ] {
                    let mut ruby = Ruby::over("\u{6F22}\u{5B57}", "\u{304B}\u{3093}\u{3058}", 44);
                    ruby.properties.align = align;
                    self.document.insert_ruby(&ruby);
                    self.document.type_text(label);
                }
                // And one the other way about: a short word under a long
                // reading, where the room is shared out the other way.
                let mut ruby = Ruby::over("Nagoya", "\u{540D}\u{53E4}\u{5C4B}", 44);
                ruby.properties.align = Align::DistributeSpace;
                self.document.insert_ruby(&ruby);

                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(22.0);
                self.document.clear_selection();
                self.relayout();
            }
            // The optional hyphen: nothing at all in the middle of a line, a
            // hyphen at the end of one.
            "hyphens" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                let word = "hy\u{00AD}phen\u{00AD}a\u{00AD}tion";
                self.document.type_text(&format!(
                    "The same paragraph twice. {word} {word} {word} {word} {word} {word}."
                ));
                self.document.press_enter();
                let plain = "hyphenation";
                self.document.type_text(&format!(
                    "The same paragraph twice. {plain} {plain} {plain} {plain} {plain} {plain}."
                ));

                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(20.0);
                self.document.clear_selection();
                self.relayout();
            }
            // The nine scripts written on Devanagari's plan: a line for each,
            // with a vowel sign written to the left or in two parts, a cluster
            // of two consonants, and a syllable with a hook where the script
            // has one — the words for the language's own name last.
            "indic" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                for line in [
                    // Bengali: ki, kko, rka, bangla.
                    "\u{0995}\u{09BF} \u{0995}\u{09CD}\u{0995}\u{09CB} \u{09B0}\u{09CD}\u{0995} \u{09AC}\u{09BE}\u{0982}\u{09B2}\u{09BE}",
                    // Gurmukhi: ki, kva, punjabi.
                    "\u{0A15}\u{0A3F} \u{0A15}\u{0A4D}\u{0A35} \u{0A2A}\u{0A70}\u{0A1C}\u{0A3E}\u{0A2C}\u{0A40}",
                    // Gujarati: ki, kka, rka, gujarati.
                    "\u{0A95}\u{0ABF} \u{0A95}\u{0ACD}\u{0A95} \u{0AB0}\u{0ACD}\u{0A95} \u{0A97}\u{0AC1}\u{0A9C}\u{0AB0}\u{0ABE}\u{0AA4}\u{0AC0}",
                    // Oriya: ki, kko, rka, odia.
                    "\u{0B15}\u{0B3F} \u{0B15}\u{0B4D}\u{0B15}\u{0B4B} \u{0B30}\u{0B4D}\u{0B15} \u{0B13}\u{0B21}\u{0B3C}\u{0B3F}\u{0B06}",
                    // Tamil: ki, ko, kau, tamil.
                    "\u{0B95}\u{0BBF} \u{0B95}\u{0BCA} \u{0B95}\u{0BCC} \u{0BA4}\u{0BAE}\u{0BBF}\u{0BB4}\u{0BCD}",
                    // Telugu: ki, kka, kai, telugu.
                    "\u{0C15}\u{0C3F} \u{0C15}\u{0C4D}\u{0C15} \u{0C15}\u{0C48} \u{0C24}\u{0C46}\u{0C32}\u{0C41}\u{0C17}\u{0C41}",
                    // Kannada: ki, kka, ko, kannada.
                    "\u{0C95}\u{0CBF} \u{0C95}\u{0CCD}\u{0C95} \u{0C95}\u{0CCB} \u{0C95}\u{0CA8}\u{0CCD}\u{0CA8}\u{0CA1}",
                    // Malayalam: ki, kka, ko, kra, malayalam.
                    "\u{0D15}\u{0D3F} \u{0D15}\u{0D4D}\u{0D15} \u{0D15}\u{0D4B} \u{0D15}\u{0D4D}\u{0D30} \u{0D2E}\u{0D32}\u{0D2F}\u{0D3E}\u{0D33}\u{0D02}",
                    // Sinhala: ki, ke, ko, ksha with the joiner, sinhala.
                    "\u{0D9A}\u{0DD2} \u{0D9A}\u{0DDA} \u{0D9A}\u{0DDC} \u{0D9A}\u{0DCA}\u{200D}\u{0DC2} \u{0DC3}\u{0DD2}\u{0D82}\u{0DC4}\u{0DBD}",
                ] {
                    self.document.type_text(line);
                    self.document.press_enter();
                }
                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(28.0);
                self.document.clear_selection();
                self.relayout();
            }
            // The scripts that are not drawn the way they are stored, and the
            // one that is written without spaces between its words.
            "scripts" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                // Devanagari: a vowel sign written to the left, a cluster of
                // two consonants, and a syllable that begins with the hook.
                self.document.type_text("कि क्क र्क र्कि हिन्दी");
                self.document.press_enter();
                // Thai: a sentence with no spaces in it, which can only wrap
                // if the breaking rules know where a syllable begins.
                // Long enough that it has to wrap, which it can only do if
                // the breaking rules know where a Thai syllable begins.
                let sentence = "\u{0E09}\u{0E31}\u{0E19}\u{0E01}\u{0E34}\u{0E19}\u{0E01}\
                     \u{0E23}\u{0E30}\u{0E08}\u{0E01}\u{0E44}\u{0E14}\u{0E49}\u{0E01}\u{0E34}\
                     \u{0E19}\u{0E41}\u{0E25}\u{0E49}\u{0E27}\u{0E44}\u{0E21}\u{0E48}\u{0E40}\
                     \u{0E08}\u{0E47}\u{0E1A}"
                    .repeat(3);
                self.document.type_text(&sentence);
                self.document.press_enter();
                // And the stack: a tall consonant, a vowel over it, a tone
                // mark over that.
                self.document.type_text(
                    "\u{0E1B}\u{0E34}\u{0E48} \u{0E01}\u{0E34}\u{0E48} \u{0E19}\u{0E49}\u{0E33}",
                );

                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(36.0);
                self.document.clear_selection();
                self.relayout();
            }
            // What a font says to do when a mark lands on a letter. The i
            // loses its dot, because the font's own rule says so and two dots
            // on one letter is not what anybody wrote.
            "composing" => {
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.type_text("i\u{0307} i\u{0301} in fi");
                let end = self.document.caret();
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.extend_selection_to(end);
                self.document.set_size(72.0);
                self.document.clear_selection();
                self.relayout();
            }
            // A video from the web: its frame, drawn with the play sign over
            // it that says what it stands for.
            "video" => {
                // The frame is made here rather than fetched: a proof should
                // need nothing beside the program, and this program does not
                // talk to the network.
                let mut canvas =
                    wp_raster::Canvas::filled(320, 180, wp_raster::Color::rgb(0x1F, 0x28, 0x38));
                for y in 0..180i32 {
                    for x in 0..320i32 {
                        let shade = (24 + (x + y) / 6) as u8;
                        canvas.fill_rect(x, y, 1, 1, wp_raster::Color::rgb(shade, shade + 8, 0x50));
                    }
                }
                let bytes = wp_raster::encode_png(&canvas);

                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                let _ = self.document.insert_web_video(
                    &bytes,
                    "png",
                    "https://example.org/watch",
                    wp_docx::EMU_PER_INCH * 3,
                    wp_docx::EMU_PER_INCH * 27 / 16,
                );
                self.relayout();
            }
            // Ink: strokes read back out of the part they were written to,
            // drawn as the pen drew them.
            "ink" => {
                use wp_docx::ink::{Ink, Stroke};

                let pen = |colour: &str, width: i64, points: Vec<(i64, i64)>| Stroke {
                    colour: colour.to_owned(),
                    width_emu: width,
                    transparency: 0,
                    flat: false,
                    points,
                    pressure: Vec::new(),
                };

                // A tick, in two strokes of a blue pen.
                let tick = Ink {
                    strokes: vec![
                        pen("0070C0", 27_000, vec![(0, 180_000), (110_000, 300_000)]),
                        pen("0070C0", 27_000, vec![(110_000, 300_000), (330_000, 0)]),
                    ],
                };

                // A line drawn by hand, which is where a pen shows: hundreds
                // of points, and the band has to follow every turn of them.
                let mut wave = Vec::new();
                for step in 0..=120 {
                    let along = step as f64 / 120.0;
                    let x = (along * 1_800_000.0) as i64;
                    let y = (180_000.0 + (along * core::f64::consts::PI * 6.0).sin() * 120_000.0)
                        as i64;
                    wave.push((x, y));
                }
                let hand = Ink { strokes: vec![pen("C00000", 18_000, wave)] };

                // A highlighter over a word written in pencil: the words under
                // it have to show through.
                let mut scribble = Vec::new();
                for step in 0..=60 {
                    let along = step as f64 / 60.0;
                    let x = (along * 900_000.0) as i64;
                    let y =
                        (150_000.0 + (along * core::f64::consts::PI * 4.0).cos() * 90_000.0) as i64;
                    scribble.push((x, y));
                }
                let marked = Ink {
                    strokes: vec![
                        pen("3B3B3B", 14_000, scribble),
                        Stroke {
                            colour: "FFFF00".to_owned(),
                            width_emu: 220_000,
                            transparency: 110,
                            flat: true,
                            points: vec![(30_000, 150_000), (870_000, 150_000)],
                            pressure: Vec::new(),
                        },
                    ],
                };

                for ink in [&tick, &hand, &marked] {
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    let _ = self.document.insert_ink(ink);
                }
                self.relayout();
            }
            // A diagram of each arrangement, drawn out of the parts the
            // document keeps it in rather than out of the model that wrote it.
            "diagrams" => {
                use wp_docx::diagram::Arrangement;

                let room = self.text_width_emu() / 2;
                for arrangement in Arrangement::ALL.iter().rev() {
                    let items: Vec<String> = ["Plan", "Draw", "Check", "Ship", "Review"]
                        .iter()
                        .map(|item| (*item).to_owned())
                        .collect();
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    let _ = self.document.insert_diagram(*arrangement, &items, room);
                }
                self.relayout();
                self.reveal_caret();
            }
            // A diagram chosen, with its tab up and the Text Pane open on
            // its words: the third line is being typed into.
            "textpane" => {
                use wp_docx::diagram::{Arrangement, Node};

                let room = self.text_width_emu() * 2 / 3;
                let tree = vec![Node {
                    text: "Head office".to_owned(),
                    children: vec![
                        Node { text: "North".to_owned(), children: vec![Node::new("Leeds")] },
                        Node::new("South"),
                        Node::new("Overseas"),
                    ],
                }];
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                let _ = self.document.insert_diagram_of(Arrangement::Hierarchy, &tree, room);
                self.relayout();
                self.choose_drawing_here();
                self.note_diagram_chosen();
                self.open_text_pane();
                self.diagram_line = 2;
                self.diagram_caret = 5;
                self.reveal_caret();
            }
            // Two series drawn every way a chart can be drawn, each with a key
            // naming the series and the number written on every point.
            "charts" => {
                use wp_docx::chart::{Chart, Kind, Labels, Legend, LegendPosition, Series};

                for kind in Kind::ALL {
                    let point = |values: Vec<f64>| Series {
                        values,
                        xs: if kind.plots_points() { vec![1.0, 2.0, 4.0] } else { Vec::new() },
                        sizes: if *kind == Kind::Bubble { vec![1.0, 3.0, 2.0] } else { Vec::new() },
                        ..Series::default()
                    };
                    let chart = Chart {
                        kind: *kind,
                        title: format!("{} chart", kind.label()),
                        categories: vec!["North".to_owned(), "South".to_owned(), "East".to_owned()],
                        series: vec![
                            Series { name: "Last year".to_owned(), ..point(vec![3.0, 5.0, 4.0]) },
                            Series { name: "This year".to_owned(), ..point(vec![4.0, 2.0, 6.0]) },
                        ],
                        legend: Some(Legend::at(LegendPosition::Bottom)),
                        labels: Labels::values(),
                        vary_colors: kind.is_round(),
                        hole: 50,
                        ..Chart::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    let _ = self.document.insert_chart(
                        &chart,
                        wp_docx::EMU_PER_INCH * 3,
                        wp_docx::EMU_PER_INCH * 2,
                    );
                }
                self.relayout();
                self.reveal_caret();
            }
            // What the rest of a chart is: stacked columns with a table of
            // the numbers under them and money on the axis, a pie with the
            // shares on its slices and its key at the top, and a combination
            // of columns and a line on a second axis.
            "morecharts" => {
                use wp_docx::chart::{
                    Axis, Chart, DataTable, Grouping, Kind, Labels, Legend, LegendPosition, Series,
                };

                let categories = vec!["Q1".to_owned(), "Q2".to_owned(), "Q3".to_owned()];
                let stacked = Chart {
                    kind: Kind::Column,
                    grouping: Grouping::Stacked,
                    title: "Revenue by quarter".to_owned(),
                    categories: categories.clone(),
                    series: vec![
                        Series {
                            name: "Hardware".to_owned(),
                            values: vec![1200.0, 1500.0, 900.0],
                            ..Series::default()
                        },
                        Series {
                            name: "Services".to_owned(),
                            values: vec![800.0, 950.0, 1400.0],
                            fill: Some("70AD47".to_owned()),
                            ..Series::default()
                        },
                    ],
                    legend: Some(Legend::at(LegendPosition::Right)),
                    data_table: Some(DataTable::default()),
                    value_axis: Axis {
                        number_format: Some("\"$\"#,##0".to_owned()),
                        ..Axis::default()
                    },
                    ..Chart::default()
                };
                let pie = Chart {
                    kind: Kind::Pie,
                    title: "Share of sales".to_owned(),
                    categories: vec!["North".to_owned(), "South".to_owned(), "Overseas".to_owned()],
                    series: vec![Series {
                        name: "Sales".to_owned(),
                        values: vec![55.0, 40.0, 5.0],
                        ..Series::default()
                    }],
                    legend: Some(Legend::at(LegendPosition::Top)),
                    labels: Labels {
                        category: true,
                        percent: true,
                        leader_lines: true,
                        ..Labels::default()
                    },
                    vary_colors: true,
                    ..Chart::default()
                };
                let combined = Chart {
                    kind: Kind::Column,
                    title: "Units and margin".to_owned(),
                    categories,
                    series: vec![
                        Series {
                            name: "Units".to_owned(),
                            values: vec![120.0, 150.0, 90.0],
                            ..Series::default()
                        },
                        Series {
                            name: "Margin".to_owned(),
                            values: vec![0.31, 0.35, 0.28],
                            kind: Some(Kind::Line),
                            secondary: true,
                            labels: Some(Labels {
                                value: true,
                                number_format: Some("0%".to_owned()),
                                ..Labels::default()
                            }),
                            ..Series::default()
                        },
                    ],
                    legend: Some(Legend::at(LegendPosition::Bottom)),
                    ..Chart::default()
                };
                for chart in [stacked, pie, combined] {
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    let _ = self.document.insert_chart(
                        &chart,
                        wp_docx::EMU_PER_INCH * 3,
                        wp_docx::EMU_PER_INCH * 2,
                    );
                }
                self.relayout();
                self.reveal_caret();
            }
            // What makes a shape solid: a bevel, a depth, and both together,
            // beside the same shape drawn flat.
            "solid" => {
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::depth::{Bevel, Depth, Scene};

                let bevel =
                    || Bevel { width_emu: 114_300, height_emu: 76_200, kind: "circle".to_owned() };
                let turned = Scene {
                    camera: "orthographicFront".to_owned(),
                    // A quarter of the way round to the right and a little
                    // forwards, which is where a depth begins to show.
                    longitude: 1_800_000,
                    latitude: 900_000,
                    light: "threePt".to_owned(),
                    light_from: "t".to_owned(),
                    ..Scene::default()
                };
                let all = [
                    (Depth::default(), Scene::default(), "Flat"),
                    (
                        Depth {
                            bevel_top: Some(bevel()),
                            material: "plastic".to_owned(),
                            ..Depth::default()
                        },
                        Scene::default(),
                        "Bevel",
                    ),
                    (Depth { extrusion_emu: 457_200, ..Depth::default() }, turned.clone(), "Depth"),
                    (
                        Depth {
                            bevel_top: Some(bevel()),
                            extrusion_emu: 457_200,
                            extrusion_colour: Some(wp_docx::colour::Colour::rgb("2F528F")),
                            material: "metal".to_owned(),
                            ..Depth::default()
                        },
                        turned,
                        "Both",
                    ),
                ];

                const CELL: i64 = 1_600_200;
                let sheet = wp_docx::shapes::Shape {
                    name: "Sheet".to_owned(),
                    preset: "rect".to_owned(),
                    width_emu: 2 * CELL,
                    height_emu: 2 * CELL,
                    fill: wp_docx::fills::Fill::solid("FFFFFF"),
                    outline: None,
                    anchor: Some(Anchor {
                        wrap: Wrap::None,
                        horizontal: Placement::Offset(0),
                        vertical: Placement::Offset(0),
                        ..Anchor::default()
                    }),
                    ..wp_docx::shapes::Shape::default()
                };
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.insert_shape(&sheet);

                for (index, (depth, scene, name)) in all.into_iter().enumerate() {
                    let shape = wp_docx::shapes::Shape {
                        name: name.to_owned(),
                        preset: "roundRect".to_owned(),
                        width_emu: 1_028_700,
                        height_emu: 800_100,
                        fill: wp_docx::fills::Fill::solid("4472C4"),
                        outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                        outline_emu: 9_525,
                        depth,
                        scene,
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset((index % 2) as i64 * CELL + 285_750),
                            vertical: Placement::Offset((index / 2) as i64 * CELL + 342_900),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();
            }
            // One shape taken hold of, so that the yellow handles it can be
            // changed by are there to look at.
            "held" => {
                use wp_docx::anchor::{Anchor, Placement, Wrap};

                let shape = wp_docx::shapes::Shape {
                    name: "Held".to_owned(),
                    preset: "roundRect".to_owned(),
                    width_emu: 2_286_000,
                    height_emu: 1_143_000,
                    fill: wp_docx::fills::Fill::solid("4472C4"),
                    outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                    outline_emu: 9_525,
                    anchor: Some(Anchor {
                        wrap: Wrap::None,
                        horizontal: Placement::Offset(457_200),
                        vertical: Placement::Offset(228_600),
                        ..Anchor::default()
                    }),
                    ..wp_docx::shapes::Shape::default()
                };
                self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                self.document.insert_shape(&shape);
                self.relayout();
                self.choose_drawing_here();
            }
            // And the stars and banners: the run of the gallery from the first
            // explosion to the last wave.
            "banners" => {
                use wp_layout::geometry::Preset;
                let shapes: Vec<Preset> = Preset::all()
                    .into_iter()
                    .skip_while(|preset| *preset != Preset::Explosion1)
                    .take_while(|preset| *preset != Preset::Line)
                    .collect();
                self.shape_grid(&shapes, 5);
            }
            "gallery" => {
                // Every shape the program can draw, laid out in rows: the only
                // way to look at the whole gallery at once and see which of
                // them is wrong.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                let all = wp_layout::geometry::Preset::all();
                let across = 8usize;
                for (index, preset) in all.iter().enumerate() {
                    let column = index % across;
                    let row = index / across;
                    let shape = wp_docx::shapes::Shape {
                        name: preset.label().to_owned(),
                        preset: preset.word().to_owned(),
                        width_emu: 685_800,
                        height_emu: 548_640,
                        fill: wp_docx::fills::Fill::solid("4472C4"),
                        outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                        outline_emu: 9_525,
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset(column as i64 * 800_100),
                            vertical: Placement::Offset(row as i64 * 640_080),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();
            }
            "fills" => {
                // Shapes filled every way the format allows: one colour, the
                // three kinds of gradient, and a row of hatchings. The only way
                // to see that a shade is a shade rather than an average of one.
                use wp_docx::anchor::{Anchor, Placement, Wrap};
                use wp_docx::colour::Colour;
                use wp_docx::fills::{Direction, Fill, Gradient, Pattern};

                let run = |from: &str, to: &str, direction: Direction| {
                    Fill::Gradient(Gradient {
                        stops: vec![(0, Colour::rgb(from)), (100_000, Colour::rgb(to))],
                        direction,
                    })
                };
                let hatch = |name: &str| {
                    Fill::Pattern(Pattern {
                        name: name.to_owned(),
                        foreground: Colour::rgb("1F3864"),
                        background: Colour::rgb("FFFFFF"),
                    })
                };

                let fills = [
                    Fill::solid("4472C4"),
                    run("4472C4", "FFFFFF", Direction::Linear(5_400_000)),
                    run("4472C4", "FFFFFF", Direction::Linear(0)),
                    run("4472C4", "FFFFFF", Direction::Linear(2_700_000)),
                    run("4472C4", "FFFFFF", Direction::Radial),
                    run("4472C4", "FFFFFF", Direction::Rectangular),
                    hatch("ltUpDiag"),
                    hatch("diagCross"),
                    hatch("pct25"),
                    hatch("horz"),
                    hatch("smCheck"),
                    hatch("wdDnDiag"),
                ];

                for (index, fill) in fills.iter().enumerate() {
                    let column = index % 6;
                    let row = index / 6;
                    let shape = wp_docx::shapes::Shape {
                        name: format!("Fill {index}"),
                        width_emu: 1_028_700,
                        height_emu: 685_800,
                        fill: fill.clone(),
                        outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                        outline_emu: 9_525,
                        anchor: Some(Anchor {
                            wrap: Wrap::None,
                            horizontal: Placement::Offset(column as i64 * 1_143_000),
                            vertical: Placement::Offset(row as i64 * 800_100),
                            ..Anchor::default()
                        }),
                        ..wp_docx::shapes::Shape::default()
                    };
                    self.document.set_caret(wp_docx::TextPosition::new(2, 0));
                    self.document.insert_shape(&shape);
                }
                self.relayout();
            }
            "customised" => {
                // A ribbon somebody has changed: a group switched off, a group
                // moved to the front, a command added to another, and a fourth
                // button on the toolbar. The only way to see that what the
                // dialog says reaches what is drawn.
                use crate::chrome::Command;
                let home = crate::chrome::ribbon::Tab::Home;
                let custom = &mut self.ribbon.custom;
                custom.set_hidden(home, "Clipboard", true);
                custom.move_group(home, "Editing", true);
                custom.add_to_group(home, "Font", Command::AddBookmark);
                custom.add_to_quick(Command::Print);
            }
            "ribbongroups" => {
                // The same page with the first tab folded open, which is the
                // only way to see the groups, their tick boxes and what is
                // under them.
                self.open_options();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(super::ribbondialog::RIBBON_PAGE);
                    dialog.focus_field(super::ribbondialog::RIBBON_TREE);
                    dialog.key(wp_shell::Key::Space, false, false);
                }
            }
            "autocorrect" => {
                self.open_autocorrect();
            }
            "autoformat" => {
                self.open_autocorrect();
                // The As You Type tab, so the other half can be photographed
                // too.
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(2);
                }
            }
            // The equation strip with names typed into it, each made its
            // character as it was finished.
            "equation-typed" => {
                self.run(super::Command::Equation);
                for character in r"E = \sum \alpha_i \le \infty".chars() {
                    self.type_into_find(character);
                }
            }
            "math-autocorrect" => {
                self.open_autocorrect();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(1);
                }
            }
            // The same, answered with Review Changes: the marks themselves.
            "autoformat-marked" => {
                self.set_view_option("autoformat-review")?;
                self.finish_dialog(crate::chrome::dialog::Answer::Named("Review Changes"));
            }
            "autoformat-tab" => {
                self.open_autocorrect();
                if let Some(dialog) = &mut self.dialog {
                    dialog.show_tab(3);
                }
            }
            "exceptions" => {
                self.open_autocorrect();
                self.autocorrect_dialog_button(super::autocorrectdialog::EXCEPTIONS);
            }
            "fontdialog" => {
                self.open_font_dialog();
            }
            // A form made of every control the Developer tab offers, so that
            // the tab and the controls can both be looked at.
            "form" => {
                self.set_view_option("tab=developer")?;
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.document.type_text("  ");
                for which in 0..6 {
                    self.insert_content_control(which);
                    self.document.type_text("  ");
                }
                self.relayout();
            }
            // Design Mode on a form: every control wearing its tags, the
            // placeholders grey, and one control answered.
            "designmode" => {
                self.set_view_option("tab=developer")?;
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.document.type_text("  Name: ");
                self.document.insert_control(
                    wp_docx::controls::ControlKind::PlainText,
                    "Surname",
                    &[],
                );
                self.document.type_text("  Town: ");
                self.document.insert_control(
                    wp_docx::controls::ControlKind::PlainText,
                    "Town",
                    &[],
                );
                self.document.type_text("  Sent by ");
                self.document.insert_control(
                    wp_docx::controls::ControlKind::DropDown,
                    "Carrier",
                    &["Post".to_owned(), "Courier".to_owned()],
                );
                let town = self.document.controls()[1].start;
                self.document.set_control_text(town, "Ludlow");
                self.toggle_design_mode();
                self.relayout();
            }
            // The three that hold more than words: a picture control with
            // its placeholder, a repeating section round two paragraphs with
            // the plus at its corner, and a gallery control waiting for a
            // choice.
            "morecontrols" => {
                self.set_view_option("tab=developer")?;
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.document.type_text("  ");
                self.insert_content_control(6);
                self.document.set_selections(&[(
                    wp_docx::TextPosition::new(4, 0),
                    wp_docx::TextPosition::new(5, 0),
                )]);
                self.insert_repeating_section();
                let end = self.document.paragraph_text(6).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(6, end));
                self.insert_gallery_control();
                self.document.set_caret(wp_docx::TextPosition::new(4, 0));
                self.relayout();
            }
            // What the Quick Parts button drops open onto.
            "quickparts" => {
                self.run(crate::chrome::Command::QuickParts);
            }
            // The ways a mail merge can end, with a list of people behind
            // them so that the button is willing to drop open at all.
            "finishmenu" => {
                self.recipients = wp_docx::merge::Recipients::parse(
                    b"Name,E-mail
Ada Lovelace,ada@example.com
Grace Hopper,grace@example.com
",
                );
                self.run(crate::chrome::Command::FinishMerge);
            }
            // And who the merge is for, which is the one dialog with a tick
            // against every row.
            "recipients" => {
                self.recipients = wp_docx::merge::Recipients::parse(
                    b"Name,Town,E-mail
Ada Lovelace,London,ada@example.com
Grace Hopper,New York,grace@example.com
Alan Turing,Wilmslow,alan@example.com
Katherine Johnson,Hampton,katherine@example.com
",
                );
                self.open_recipient_list();
            }
            // The two things the Compare button drops open onto, which is
            // the only way to see that the button does drop open.
            "comparemenu" => {
                self.run(crate::chrome::Command::Compare);
            }
            // Word's Compare, with the two documents it came from beside
            // the result: the same document compared with a changed copy of
            // itself, which is the only way to photograph the view on a
            // machine with no files to compare.
            "compared" => {
                let mut revised = self.document.clone();
                revised.set_caret(wp_docx::TextPosition::new(3, 0));
                revised.clear_selection();
                let end = revised.paragraph_text(3).unwrap_or_default().len();
                revised.extend_selection_to(wp_docx::TextPosition::new(3, end));
                revised.delete_selection();
                revised.type_text("A paragraph the other version says differently.");

                let before = self.document.clone();
                self.document.compare_with(&revised, "Compare");
                self.show_comparison(&before, &revised, std::path::Path::new("Revised.docx"), None);
                self.relayout();
            }
            // Word's Encrypt with Password, off the File page.
            "encrypt" => {
                self.open_encryption();
            }
            // Word's Restrict Editing pane, which stands beside the document
            // rather than over it.
            "restrict" => {
                self.open_protection();
            }
            // The same pane once something is being enforced, which is a
            // different pane: what this person may do, and where.
            "restricting" => {
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.document.move_caret(wp_docx::TextPosition::new(0, 9), true);
                self.document.allow_everyone();
                self.open_protection();
                self.choose_restrict_mode(0);
                self.start_enforcing();
                self.finish_dialog(crate::chrome::dialog::Answer::Accept);
                self.relayout();
            }
            // The formatting half, which Word keeps in a dialog of its own
            // because it is a list of every style the document has.
            "limits" => {
                self.open_protection();
                self.open_formatting_limits();
                self.limit_to_three_styles();
            }
            // And what a document under it looks like: the gallery with the
            // styles nobody allowed left out, and the refusal.
            "limited" => {
                self.open_protection();
                self.open_formatting_limits();
                self.limit_to_three_styles();
                self.finish_dialog(crate::chrome::dialog::Answer::Accept);
                self.start_enforcing();
                self.finish_dialog(crate::chrome::dialog::Answer::Accept);
                self.run(crate::chrome::Command::Format(wp_docx::CharacterFormat::Bold));
            }
            // Word's General Options, off the File page: what a document
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
            // A restricted document with one stretch anybody may edit, which
            // is the shading Word paints and the thing a person needs to see.
            // The Protect group of the Review tab, where the exception is made.
            "protectgroup" => {
                self.ribbon.tab = crate::chrome::ribbon::Tab::Review;
            }
            // Word's Signatures: what the document carries, and what it can
            // be signed with.
            "signatures" => {
                self.open_signatures();
            }
            // The same pane with a line somebody has been asked to sign on,
            // which is the half of it that says what is left to do.
            "signaturerequest" => {
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.document.insert_signature_line(&wp_docx::signature::Signer {
                    name: String::from("Ada Lovelace"),
                    title: String::from("Director"),
                    id: String::from("9a3f"),
                });
                self.relayout();
                self.open_signatures();
            }
            // Word's Compare dialog: what counts as a difference, and where
            // the answer goes.
            "comparedialog" => {
                let mut body = wp_docx::model::Body::default();
                body.blocks.push(wp_docx::model::Block::Paragraph(
                    wp_docx::model::Paragraph::text("The revised copy"),
                ));
                let bytes = wp_docx::Document::create(&body)
                    .and_then(|document| document.save())
                    .unwrap_or_default();
                if let Ok(revised) = wp_docx::Document::open(&bytes) {
                    self.to_compare = Some((
                        std::path::PathBuf::from("Contract (Agnes).docx"),
                        Box::new(revised),
                    ));
                    self.open_compare_dialog();
                }
            }
            // Word's Manage Styles: the defaults a document starts from.
            "managestyles" => {
                self.open_manage_styles();
            }
            // And its second tab, the Organizer.
            "organizer" => {
                self.open_manage_styles();
                self.dialog_key(wp_shell::Key::Tab, false, true);
            }
            // Word's Content Control Properties, on a drop-down.
            "controlproperties" => {
                self.insert_content_control(4);
                let at = self.document.controls()[0].start;
                self.document.set_caret(at);
                self.run(crate::chrome::Command::ControlProperties);
            }
            // The gallery of style sets, on the Design tab where it lives.
            "stylesets" => {
                self.ribbon.tab = crate::chrome::ribbon::Tab::Design;
                // Drawn again before the gallery is asked for: the menu hangs
                // under its button, and a ribbon that has not drawn the tab
                // does not know where the button is.
                self.needs_redraw = true;
                let (width, height) = (self.view_width, self.view_height);
                self.draw(width, height);
                self.run(crate::chrome::Command::StyleSet);
            }
            // And the Lines set applied, so that what one does can be seen.
            "setapplied" => {
                self.choose_style_set(1);
                self.relayout();
            }
            // The bar across the top of a document that asked to be opened
            // read-only.
            "infobar" => {
                self.document
                    .set_write_protection(Some(&wp_docx::readonly::WriteProtection::recommended()));
                self.asked_at_the_door(std::path::Path::new("Contract.docx"));
                self.finish_dialog(crate::chrome::dialog::Answer::Accept);
                self.relayout();
            }
            // Edit Anyway pressed on a signed document: the question asked
            // before the signatures come off.
            "signedquestion" => {
                self.ask_to_take_the_signatures_off();
            }
            // The window asked to close with a change in the document: Word's
            // question about saving it, which the close waits on.
            "savequestion" => {
                self.document.mark_modified();
                self.update_title();
                let _ = self.handle(wp_shell::Event::Closing);
            }
            // Word's General Options, which holds both passwords.
            "generaloptions" => {
                self.open_read_only_settings();
            }
            // Word's Mail Merge Recipients, with the four buttons under it.
            "recipientlist" => {
                self.recipients = wp_docx::merge::Recipients::parse(
                    b"Last Name,City
Ogg,Lancre
Nitt,Ankh-Morpork
ogg,lancre
,Genua
",
                );
                self.open_recipient_list();
            }
            // And Type a New List, which makes one without a file.
            "newlist" => {
                self.type_a_new_list();
            }
            // Word's Building Blocks Organizer, with something saved in it.
            "blockorganizer" => {
                if let Some(mut template) = self.own_template() {
                    for (name, gallery, category) in [
                        ("Yours faithfully", wp_docx::blocks::QUICK_PARTS, "Letters"),
                        ("Our address", wp_docx::blocks::AUTO_TEXT, "General"),
                    ] {
                        let block = wp_docx::blocks::BuildingBlock {
                            name: name.to_owned(),
                            gallery: gallery.to_owned(),
                            category: category.to_owned(),
                            description: String::new(),
                        };
                        let mut body = wp_docx::model::Body::default();
                        body.blocks.push(wp_docx::model::Block::Paragraph(
                            wp_docx::model::Paragraph::text(name),
                        ));
                        template.add_building_block(&block, &body);
                    }
                    self.save_own_template(&template);
                }
                self.open_organizer();
            }
            "exception" => {
                self.document.set_caret(wp_docx::TextPosition::new(4, 0));
                let end = self.document.paragraph_text(4).unwrap_or_default().len();
                self.document.extend_selection_to(wp_docx::TextPosition::new(4, end));
                self.run(crate::chrome::Command::AllowEveryone);
                self.document.set_protection(Some(&wp_docx::protection::Protection::new(
                    wp_docx::protection::EditMode::ReadOnly,
                )));
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.run(crate::chrome::Command::Format(wp_docx::CharacterFormat::Bold));
                self.relayout();
            }
            "restricted" => {
                // The document already protected, so the strip along the
                // bottom and the refusal can be seen rather than described.
                self.open_protection();
                self.choose_restrict_mode(0);
                self.start_enforcing();
                self.finish_dialog(crate::chrome::dialog::Answer::Accept);
                self.open_protection();
                self.run(crate::chrome::Command::Format(wp_docx::CharacterFormat::Bold));
            }
            "fontadvanced" => {
                self.open_font_dialog();
                // The second tab, so the half of the dialog that is not the
                // first can be photographed too.
                self.dialog_key(wp_shell::Key::Tab, false, true);
            }
            "gridlines" => self.show_gridlines = true,
            "palette" => {
                self.palette = Some(crate::chrome::palette::Palette::new(
                    crate::chrome::palette::Kind::Text,
                    380.0,
                    140.0,
                ))
            }
            "grid" => self.table_grid = Some(TableGrid::new(180.0, 140.0)),
            "contents" => {
                // A table of contents at the top of the document, to see the
                // page numbers put against the right margin with dots.
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.run(crate::chrome::Command::InsertContents);
            }
            "linenumbers" => {
                // Numbers down the margin, to see where they land.
                self.document.set_line_numbers(Some(wp_docx::appearance::LineNumbers::default()));
                self.relayout();
            }
            "pagenumbers" => {
                // The gallery of page-number designs: the second of the two
                // questions the Page Number button asks, and the one that had
                // nothing behind it until this program drew its own.
                self.ribbon.tab = crate::chrome::ribbon::Tab::Insert;
                self.needs_redraw = true;
                self.paint(self.view_width, self.view_height);
                self.open_page_number_designs(super::designs::Place::Bottom);
            }
            // A number standing in the margin beside the text, which is not a
            // running head at all but a box the header carries.
            "pagenumbermargin" => {
                self.page_number_place = super::designs::Place::Margins;
                self.choose_page_number_design(1);
                self.relayout();
            }
            "pagenumberdrawn" => {
                // Two of the designs on the page itself: the band behind the
                // number at the foot, and the rule under the one at the head.
                // A gallery that offered an arrangement the page could not
                // draw would be offering a picture of one.
                self.page_number_place = super::designs::Place::Bottom;
                self.choose_page_number_design(7);
                self.page_number_place = super::designs::Place::Top;
                self.choose_page_number_design(8);
                self.relayout();
            }
            "numbering" => {
                // The menu that says how the section numbers its pages.
                self.ribbon.tab = crate::chrome::ribbon::Tab::Insert;
                self.paint(self.view_width, self.view_height);
                self.open_page_numbering();
            }
            "headerfooter" => {
                // The tab that appears while a header is being edited.
                self.document
                    .set_furniture(
                        wp_docx::furniture::Furniture::Header,
                        wp_docx::furniture::Preset::Text,
                        wp_docx::model::Alignment::Center,
                        "Quarterly report",
                    )
                    .map_err(|error| error.to_string())?;
                self.relayout();
                self.edit_furniture(wp_docx::furniture::Furniture::Header);
            }
            "tabmenu" => {
                // The menu a double click on a tab stop opens.
                use wp_docx::model::{TabAlignment, TabLeader, TabStop};
                self.document.set_tab_stops_here(&[TabStop {
                    position: 2880,
                    alignment: TabAlignment::End,
                    leader: TabLeader::Dot,
                }]);
                self.relayout();
                self.open_tab_stop_menu(0, 400, self.ruler_top() as i32 + 15);
            }
            "tabs" => {
                // A stop of each kind, to see the markers the ruler draws.
                use wp_docx::model::{TabAlignment, TabLeader, TabStop};
                let stops: Vec<TabStop> = [
                    (1440, TabAlignment::Start),
                    (2880, TabAlignment::Center),
                    (4320, TabAlignment::End),
                    (5760, TabAlignment::Decimal),
                    (7200, TabAlignment::Bar),
                ]
                .into_iter()
                .map(|(position, alignment)| TabStop {
                    position,
                    alignment,
                    leader: TabLeader::None,
                })
                .collect();
                self.document.set_tab_stops_here(&stops);
                self.relayout();
            }
            "statusmenu" => {
                // The menu the right button opens on the strip along the
                // bottom.
                let y = self.view_height as i32 - 12;
                self.open_status_menu(300, y);
            }
            "keytips" => {
                // The letters Alt puts over the ribbon, at the tab level.
                self.toggle_key_tips();
            }
            "keytips2" => {
                // And a step in: the letters over one tab's commands.
                self.toggle_key_tips();
                self.press_key_tip('h');
            }
            "menu" => {
                // The menu the right button opens, over the middle of the page.
                self.open_context_menu(560, 300);
            }
            // The same menu over a measurement, with the Actions tab's
            // additional actions switched on.
            "menu-actions" => {
                self.autocorrect.actions.enabled = true;
                let end = self.document.paragraph_text(3).unwrap_or_default().len();
                self.document.set_caret(wp_docx::TextPosition::new(3, end));
                self.document.type_text(" A pipe 5 inches long.");
                self.relayout();
                let text = self.document.paragraph_text(3).unwrap_or_default();
                let at = text.find("5 inches").unwrap_or(0) + 1;
                if let Some((x, y, _, height)) =
                    self.caret_rect_at(wp_docx::TextPosition::new(3, at))
                {
                    self.open_context_menu(x as i32, (y + height / 2.0) as i32);
                }
            }
            // The outline with its first heading folded, for a picture of a
            // folded one: after `view=outline`.
            "collapse-first" => self.fold_first_heading(),
            "tip" => {
                // A button with the pointer resting on it, which is the only
                // way to see what a tip looks like without one.
                self.hovered = Some(crate::chrome::Command::Highlight);
                self.show_tip();
            }
            "minibar" => {
                // Some text taken and the bar floating over it, which is the
                // only way to look at the thing without a pointer to hand.
                self.document.set_caret(wp_docx::TextPosition::new(0, 0));
                self.document.extend_selection_to(wp_docx::TextPosition::new(0, 12));
                self.show_mini_bar(520, 380);
            }
            other => {
                // A settings file to look at the window with, in place of
                // nothing remembered: what a person's own file would put on
                // the screen — the lists of the Trust Center, say — shown
                // without the picture depending on whoever ran it.
                if let Some(path) = other.strip_prefix("settings=") {
                    let text = std::fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {path}: {error}"))?;
                    self.apply_settings(crate::settings::Settings::parse(&text));
                    return Ok(());
                }
                // One of the View tab's ways of looking at the document.
                if let Some(name) = other.strip_prefix("view=") {
                    let view = match name {
                        "print" => super::views::View::Print,
                        "web" => super::views::View::Web,
                        "draft" => super::views::View::Draft,
                        "outline" => super::views::View::Outline,
                        "read" => super::views::View::Reading,
                        unknown => return Err(format!("no view called {unknown:?}")),
                    };
                    self.set_view(view);
                    return Ok(());
                }
                if let Some(percent) = other.strip_prefix("zoom=") {
                    let wanted: f32 = percent
                        .parse()
                        .map_err(|_| format!("{percent:?} is not a number of per cent"))?;
                    self.set_zoom(wanted);
                    return Ok(());
                }
                // One of Word's art borders round the pages, by the name the
                // file uses for it. The only way to look at a pattern without a
                // screen, and the only way to look at more than one of them
                // without opening the dialog twenty-nine times.
                if let Some(name) = other.strip_prefix("artborder=") {
                    if !wp_docx::art::is_art(name) {
                        return Err(format!("{name:?} is not one of Word's art borders"));
                    }
                    let line = wp_docx::model::Border::line(name, 20, None);
                    let borders = wp_docx::pageborders::PageBorders::box_all(&line);
                    self.document.set_page_borders_everywhere(&borders);
                    self.relayout();
                    return Ok(());
                }

                // One of the menus a ribbon arrow drops. The tab it is on has
                // to be open already, which is why this comes after `tab=`.
                if let Some(name) = other.strip_prefix("menu=") {
                    let choice = match name {
                        "bullets" => crate::chrome::Choice::BulletLibrary,
                        "numbers" => crate::chrome::Choice::NumberLibrary,
                        "levels" => crate::chrome::Choice::MultilevelLibrary,
                        "spacing" => crate::chrome::Choice::LineSpacing,
                        "documentspacing" => crate::chrome::Choice::DocumentSpacing,
                        "case" => crate::chrome::Choice::LetterCase,
                        "pagenumber" => crate::chrome::Choice::PageNumberPlace,
                        "select" => crate::chrome::Choice::Selecting,
                        "autofit" => crate::chrome::Choice::AutoFit,
                        "notes" => crate::chrome::Choice::NoteJump,
                        "accept" => crate::chrome::Choice::Accepting,
                        "reject" => crate::chrome::Choice::Rejecting,
                        "tracking" => crate::chrome::Choice::Tracking,
                        unknown => return Err(format!("no menu called {unknown:?}")),
                    };
                    self.open_ribbon_menu(choice);
                    return Ok(());
                }

                // The File tab is the backstage, and its places are the only
                // part of the program a picture cannot otherwise reach: each
                // of them fills the window, so only one can be photographed at
                // a time.
                if let Some(name) = other.strip_prefix("file=") {
                    let place = crate::chrome::backstage::Place::ALL
                        .iter()
                        .find(|place| {
                            place.has_a_page()
                                && place.label().replace(' ', "").eq_ignore_ascii_case(name)
                        })
                        .ok_or_else(|| format!("no place called {name:?} has a page"))?;
                    self.open_backstage(*place);
                    return Ok(());
                }
                let Some(name) = other.strip_prefix("tab=") else {
                    return Err(format!("unknown option {other:?}"));
                };
                let tab = Tab::ALL
                    .iter()
                    .find(|tab| tab.label().eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("no tab called {name:?}"))?;
                // Through the same door a press goes through, so that `tab=file`
                // shows what pressing File shows rather than an empty ribbon.
                self.choose_tab(*tab);
            }
        }
        // Every option changes what the window looks like, and the window only
        // draws itself again when it is told something has changed. An option
        // that forgot to say so would be a picture of the window without it.
        self.needs_redraw = true;
        Ok(())
    }

    /// A three by three table at the caret with every cell saying where it
    /// is, the Layout tab showing, and the caret in the first cell: what the
    /// pictures of merging and splitting start from.
    fn fill_a_table_for_a_picture(&mut self) {
        self.document.insert_table(3, 3);
        self.relayout();
        for row in 0..3 {
            for column in 0..3 {
                let Some((paragraph, _)) = self.document.cell_paragraphs(row, column) else {
                    continue;
                };
                self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
                self.document.type_text(&format!("Cell {row}{column}"));
            }
        }
        if let Some((paragraph, _)) = self.document.cell_paragraphs(0, 0) {
            self.document.set_caret(wp_docx::TextPosition::new(paragraph, 0));
        }
        self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
        self.relayout();
    }
}

impl Editor {
    /// Tells the desktop which way round the window's colours go.
    ///
    /// The rounded corners, the line round the window and the caption that
    /// shows while it is dragged are drawn by the desktop, not by this program,
    /// and it draws them in the system's colours until it is told otherwise. It
    /// only shows when the window is not maximised, because a maximised window
    /// has no border — which is why this was easy to miss.
    ///
    /// Sent whenever the theme has changed since it was last sent, so the first
    /// paint sends it and every paint after one costs a comparison.
    pub(super) fn tell_desktop_the_theme(&mut self) {
        if self.frame_told == Some(self.theme.mode) {
            return;
        }
        let parts = |colour: wp_raster::Color| (colour.red, colour.green, colour.blue);
        wp_shell::set_frame_appearance(
            self.theme.mode == Mode::Dark,
            parts(self.theme.ribbon_edge),
            parts(self.theme.title_bar),
        );
        self.frame_told = Some(self.theme.mode);
    }
}

impl Editor {
    /// Lays out a run of shapes on a blank sheet, for looking at.
    ///
    /// Square cells, because a shape takes its proportions from the box it is
    /// given: a wide, short box makes Word draw a two-headed arrow as a
    /// diamond, and that is correct arithmetic which says nothing about the
    /// shape. And a sheet under the lot, because the body text showing through
    /// the gaps between the shapes hides the very edges being looked at.
    fn shape_grid(&mut self, presets: &[wp_layout::geometry::Preset], across: usize) {
        let cells: Vec<(wp_layout::geometry::Preset, Vec<(String, i32)>)> =
            presets.iter().map(|preset| (*preset, Vec::new())).collect();
        self.handle_grid(&cells, across);
    }

    /// The same, with a handle moved on each shape: the only way to see that a
    /// shape follows the value the document gives it rather than the one the
    /// format falls back on.
    fn handle_grid(
        &mut self,
        cells: &[(wp_layout::geometry::Preset, Vec<(String, i32)>)],
        across: usize,
    ) {
        use wp_docx::anchor::{Anchor, Placement, Wrap};

        /// One cell, and the shape inside it: an inch, with a tenth of an inch
        /// of air round the shape so that two neighbours never touch.
        const CELL: i64 = 914_400;
        const SHAPE: i64 = 800_100;

        let rows = cells.len().div_ceil(across) as i64;
        // The sheet first: the newest drawing at one place is drawn last and
        // so on top, so the sheet has to go in before what stands on it.
        let sheet = wp_docx::shapes::Shape {
            name: "Sheet".to_owned(),
            preset: "rect".to_owned(),
            width_emu: across as i64 * CELL,
            height_emu: rows * CELL,
            fill: wp_docx::fills::Fill::solid("FFFFFF"),
            outline: None,
            anchor: Some(Anchor {
                wrap: Wrap::None,
                horizontal: Placement::Offset(0),
                vertical: Placement::Offset(0),
                ..Anchor::default()
            }),
            ..wp_docx::shapes::Shape::default()
        };
        self.document.set_caret(wp_docx::TextPosition::new(2, 0));
        self.document.insert_shape(&sheet);

        for (index, (preset, adjusts)) in cells.iter().enumerate() {
            let shape = wp_docx::shapes::Shape {
                name: preset.label().to_owned(),
                preset: preset.word().to_owned(),
                adjusts: adjusts.clone(),
                width_emu: SHAPE,
                height_emu: SHAPE,
                fill: wp_docx::fills::Fill::solid("4472C4"),
                outline: Some(wp_docx::colour::Colour::rgb("1F3864")),
                outline_emu: 9_525,
                anchor: Some(Anchor {
                    wrap: Wrap::None,
                    horizontal: Placement::Offset((index % across) as i64 * CELL),
                    vertical: Placement::Offset((index / across) as i64 * CELL),
                    ..Anchor::default()
                }),
                ..wp_docx::shapes::Shape::default()
            };
            self.document.set_caret(wp_docx::TextPosition::new(2, 0));
            self.document.insert_shape(&shape);
        }
        self.relayout();
    }
}
