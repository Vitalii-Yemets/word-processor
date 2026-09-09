//! Putting a table or a picture into the document, and switching the theme.

use wp_docx::EMU_PER_INCH;
use wp_shell::Response;

use crate::chrome::{Mode, StyleSample, TableGrid, Theme};

use super::{Editor, DPI};

/// The kinds of picture the dialog offers, which are the ones the decoder reads
/// plus the ones a package can carry that Word will render even if this program
/// cannot yet.
const PICTURE_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter {
        label: "Pictures (*.png;*.jpg;*.jpeg)",
        pattern: "*.png;*.jpg;*.jpeg",
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
        let Some(path) = wp_shell::dialog::open_file("Insert Picture", PICTURE_FILTERS) else {
            return Response::Ignored;
        };

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot read {}: {error}", path.display()));
                return Response::Ignored;
            }
        };

        // A picture's own size is its pixels read at the screen's usual
        // ninety-six to the inch, which is the size Word gives it too. One that
        // is wider than the text is brought down to fit, because a photograph
        // from a camera is thousands of pixels across.
        let Ok(image) = wp_image::decode(&bytes) else {
            wp_shell::dialog::show_error(&format!(
                "{} is not a picture this program can read. PNG and JPEG are.",
                path.display()
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
            Ok(inserted) => self.edited(inserted, "Picture"),
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot insert the picture: {error}"));
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
    pub(super) fn style_gallery(&self) -> Vec<StyleSample> {
        let body_size = self.document.styles().resolve_run(None, &Default::default());
        let mut out = vec![StyleSample {
            id: None,
            name: "Normal".to_owned(),
            bold: false,
            italic: false,
            size: body_size.size_half_points as f32 / 2.0,
        }];

        for style in self.document.styles().all() {
            if style.kind != wp_docx::StyleKind::Paragraph || style.is_default {
                continue;
            }
            let label = style.name.clone().unwrap_or_else(|| style.id.clone());
            let resolved = self.document.styles().resolve_run(Some(&style.id), &Default::default());
            out.push(StyleSample {
                id: Some(style.id.clone()),
                name: title_case(&label),
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

/// Makes a style name look the way Word shows it: "heading 1" becomes
/// "Heading 1".
fn title_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut starting = true;
    for character in name.chars() {
        if starting {
            out.extend(character.to_uppercase());
        } else {
            out.push(character);
        }
        starting = character == ' ';
    }
    out
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
            "table" => {
                self.document.insert_table(3, 3);
                self.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
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
                if let Some(percent) = other.strip_prefix("zoom=") {
                    let wanted: f32 = percent
                        .parse()
                        .map_err(|_| format!("{percent:?} is not a number of per cent"))?;
                    self.set_zoom(wanted);
                    return Ok(());
                }
                let Some(name) = other.strip_prefix("tab=") else {
                    return Err(format!("unknown option {other:?}"));
                };
                let tab = Tab::ALL
                    .iter()
                    .find(|tab| tab.label().eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("no tab called {name:?}"))?;
                self.ribbon.tab = *tab;
            }
        }
        // Every option changes what the window looks like, and the window only
        // draws itself again when it is told something has changed. An option
        // that forgot to say so would be a picture of the window without it.
        self.needs_redraw = true;
        Ok(())
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
