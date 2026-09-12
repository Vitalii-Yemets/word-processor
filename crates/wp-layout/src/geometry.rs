//! The outlines of the shapes a document can hold.
//!
//! # Why the shapes are named and not drawn
//!
//! A document does not carry the outline of a star. It carries the word
//! `star5` and a box to fit it in — a *preset geometry* — and every program
//! that opens the document is expected to know what that means. There are about
//! 180 of them in the format.
//!
//! So this is that knowledge: the ones a person actually reaches for, each
//! turned into a path inside whatever box it was given. A preset this does not
//! know is drawn as the rectangle it occupies, which is wrong but is visibly a
//! shape in the right place and the right size, rather than nothing at all.
//!
//! # How an outline is drawn without a stroker
//!
//! By filling a ring. The outer edge is the shape; the inner edge is the same
//! shape a little smaller, wound the other way round. Filled by the nonzero
//! rule, the two together leave a band — which is the outline. It costs nothing
//! and needs no stroking algorithm, and for the shapes here it is exact where
//! the edges are straight and close where they curve.

use wp_raster::{Command, Path, Point};

/// How closely a curve is followed. Four segments to the quarter is smooth at
/// any size a shape is drawn on a page.
const ARC_STEPS: usize = 8;

/// The shapes this program can draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    Star,
    /// An arrow pointing right, which is the one an arrow usually is.
    Arrow,
    /// A straight line from one corner of the box to the other.
    Line,
    /// A speech bubble.
    Callout,

    // The rectangle gallery: one shape with its corners treated four ways.
    SnipOneCorner,
    SnipTwoSame,
    SnipTwoDiagonal,
    SnipAndRound,
    RoundOneCorner,
    RoundTwoSame,
    RoundTwoDiagonal,

    // The rest of the regular polygons, and the shapes made of straight lines.
    Heptagon,
    Octagon,
    Decagon,
    Dodecagon,
    Trapezoid,
    Parallelogram,
    Cross,
    LShape,
    HalfFrame,

    // The ones made of arcs.
    Pie,
    Chord,
    Arc,
    Donut,
    NoSymbol,
    BlockArc,
    Can,
    Teardrop,
    Frame,
    Plaque,
    Moon,
    Heart,

    // Word's block arrows. An arrow is a shaft with a head on it, and which
    // sides the heads are on is the whole of the difference between most of
    // these. See [`crate::arrows`].
    LeftArrow,
    UpArrow,
    DownArrow,
    LeftRightArrow,
    UpDownArrow,
    QuadArrow,
    LeftRightUpArrow,
    NotchedArrow,
    StripedArrow,
    HomePlate,
    Chevron,
    BentArrow,
    BentUpArrow,
    LeftUpArrow,
    UturnArrow,
    CurvedRightArrow,
    CurvedLeftArrow,
    CurvedUpArrow,
    CurvedDownArrow,
    RightArrowCallout,
    LeftArrowCallout,
    UpArrowCallout,
    DownArrowCallout,
    LeftRightArrowCallout,
    QuadArrowCallout,
    CircularArrow,
    SwooshArrow,

    // Word's flowchart shapes. Most of these are a rectangle with one edge
    // changed, and eight of them have a line drawn inside the shape rather
    // than round it. See [`crate::flowchart`].
    FlowProcess,
    FlowAlternateProcess,
    FlowDecision,
    FlowData,
    FlowPredefinedProcess,
    FlowInternalStorage,
    FlowDocument,
    FlowMultidocument,
    FlowTerminator,
    FlowPreparation,
    FlowManualInput,
    FlowManualOperation,
    FlowConnector,
    FlowOffpageConnector,
    FlowCard,
    FlowPunchedTape,
    FlowSummingJunction,
    FlowOr,
    FlowCollate,
    FlowSort,
    FlowExtract,
    FlowMerge,
    FlowStoredData,
    FlowDelay,
    FlowSequentialStorage,
    FlowMagneticDisk,
    FlowDirectStorage,
    FlowDisplay,

    // Word's stars and banners. The stars differ in how many points they have
    // and how far in the dips go; the rest are a banner of one shape or
    // another. See [`crate::banners`].
    Explosion1,
    Explosion2,
    Star4,
    Star6,
    Star7,
    Star8,
    Star10,
    Star12,
    Star16,
    Star24,
    Star32,
    UpRibbon,
    DownRibbon,
    CurvedUpRibbon,
    CurvedDownRibbon,
    VerticalScroll,
    HorizontalScroll,
    Wave,
    DoubleWave,
}

/// Every preset this program draws: the name the format knows it by, and the
/// name a person is shown when picking one.
///
/// One table rather than two matches. A shape added to only one of them is a
/// shape that draws and cannot be named, or is named and draws as a rectangle —
/// and neither says so.
const NAMED: &[(Preset, &str, &str)] = &[
    (Preset::Rectangle, "rect", "Rectangle"),
    (Preset::RoundedRectangle, "roundRect", "Rounded Rectangle"),
    (Preset::SnipOneCorner, "snip1Rect", "Snip Single Corner Rectangle"),
    (Preset::SnipTwoSame, "snip2SameRect", "Snip Same Side Corner Rectangle"),
    (Preset::SnipTwoDiagonal, "snip2DiagRect", "Snip Diagonal Corner Rectangle"),
    (Preset::SnipAndRound, "snipRoundRect", "Snip and Round Single Corner Rectangle"),
    (Preset::RoundOneCorner, "round1Rect", "Round Single Corner Rectangle"),
    (Preset::RoundTwoSame, "round2SameRect", "Round Same Side Corner Rectangle"),
    (Preset::RoundTwoDiagonal, "round2DiagRect", "Round Diagonal Corner Rectangle"),
    (Preset::Ellipse, "ellipse", "Oval"),
    (Preset::Triangle, "triangle", "Isosceles Triangle"),
    (Preset::RightTriangle, "rtTriangle", "Right Triangle"),
    (Preset::Diamond, "diamond", "Diamond"),
    (Preset::Trapezoid, "trapezoid", "Trapezoid"),
    (Preset::Parallelogram, "parallelogram", "Parallelogram"),
    (Preset::Pentagon, "pentagon", "Regular Pentagon"),
    (Preset::Hexagon, "hexagon", "Hexagon"),
    (Preset::Heptagon, "heptagon", "Heptagon"),
    (Preset::Octagon, "octagon", "Octagon"),
    (Preset::Decagon, "decagon", "Decagon"),
    (Preset::Dodecagon, "dodecagon", "Dodecagon"),
    (Preset::Cross, "plus", "Cross"),
    (Preset::LShape, "lShape", "L Shape"),
    (Preset::HalfFrame, "halfFrame", "Half Frame"),
    (Preset::Frame, "frame", "Frame"),
    (Preset::Pie, "pie", "Pie"),
    (Preset::Chord, "chord", "Chord"),
    (Preset::Arc, "arc", "Arc"),
    (Preset::Donut, "donut", "Donut"),
    (Preset::NoSymbol, "noSmoking", "\"No\" Symbol"),
    (Preset::BlockArc, "blockArc", "Block Arc"),
    (Preset::Can, "can", "Can"),
    (Preset::Teardrop, "teardrop", "Teardrop"),
    (Preset::Plaque, "plaque", "Plaque"),
    (Preset::Moon, "moon", "Moon"),
    (Preset::Heart, "heart", "Heart"),
    // The block arrows, in the order Word's gallery shows them.
    (Preset::Arrow, "rightArrow", "Right Arrow"),
    (Preset::LeftArrow, "leftArrow", "Left Arrow"),
    (Preset::UpArrow, "upArrow", "Up Arrow"),
    (Preset::DownArrow, "downArrow", "Down Arrow"),
    (Preset::LeftRightArrow, "leftRightArrow", "Left-Right Arrow"),
    (Preset::UpDownArrow, "upDownArrow", "Up-Down Arrow"),
    (Preset::QuadArrow, "quadArrow", "Quad Arrow"),
    (Preset::LeftRightUpArrow, "leftRightUpArrow", "Left-Right-Up Arrow"),
    (Preset::BentArrow, "bentArrow", "Bent Arrow"),
    (Preset::UturnArrow, "uturnArrow", "U-Turn Arrow"),
    (Preset::LeftUpArrow, "leftUpArrow", "Left-Up Arrow"),
    (Preset::BentUpArrow, "bentUpArrow", "Bent-Up Arrow"),
    (Preset::CurvedRightArrow, "curvedRightArrow", "Curved Right Arrow"),
    (Preset::CurvedLeftArrow, "curvedLeftArrow", "Curved Left Arrow"),
    (Preset::CurvedUpArrow, "curvedUpArrow", "Curved Up Arrow"),
    (Preset::CurvedDownArrow, "curvedDownArrow", "Curved Down Arrow"),
    (Preset::StripedArrow, "stripedRightArrow", "Striped Right Arrow"),
    (Preset::NotchedArrow, "notchedRightArrow", "Notched Right Arrow"),
    (Preset::HomePlate, "homePlate", "Pentagon"),
    (Preset::Chevron, "chevron", "Chevron"),
    (Preset::RightArrowCallout, "rightArrowCallout", "Right Arrow Callout"),
    (Preset::LeftArrowCallout, "leftArrowCallout", "Left Arrow Callout"),
    (Preset::UpArrowCallout, "upArrowCallout", "Up Arrow Callout"),
    (Preset::DownArrowCallout, "downArrowCallout", "Down Arrow Callout"),
    (Preset::LeftRightArrowCallout, "leftRightArrowCallout", "Left-Right Arrow Callout"),
    (Preset::QuadArrowCallout, "quadArrowCallout", "Quad Arrow Callout"),
    (Preset::CircularArrow, "circularArrow", "Circular Arrow"),
    (Preset::SwooshArrow, "swooshArrow", "Swoosh Arrow"),
    // The flowchart shapes, in the order Word's gallery shows them.
    (Preset::FlowProcess, "flowChartProcess", "Flowchart: Process"),
    (Preset::FlowAlternateProcess, "flowChartAlternateProcess", "Flowchart: Alternate Process"),
    (Preset::FlowDecision, "flowChartDecision", "Flowchart: Decision"),
    (Preset::FlowData, "flowChartInputOutput", "Flowchart: Data"),
    (Preset::FlowPredefinedProcess, "flowChartPredefinedProcess", "Flowchart: Predefined Process"),
    (Preset::FlowInternalStorage, "flowChartInternalStorage", "Flowchart: Internal Storage"),
    (Preset::FlowDocument, "flowChartDocument", "Flowchart: Document"),
    (Preset::FlowMultidocument, "flowChartMultidocument", "Flowchart: Multidocument"),
    (Preset::FlowTerminator, "flowChartTerminator", "Flowchart: Terminator"),
    (Preset::FlowPreparation, "flowChartPreparation", "Flowchart: Preparation"),
    (Preset::FlowManualInput, "flowChartManualInput", "Flowchart: Manual Input"),
    (Preset::FlowManualOperation, "flowChartManualOperation", "Flowchart: Manual Operation"),
    (Preset::FlowConnector, "flowChartConnector", "Flowchart: Connector"),
    (Preset::FlowOffpageConnector, "flowChartOffpageConnector", "Flowchart: Off-page Connector"),
    (Preset::FlowCard, "flowChartPunchedCard", "Flowchart: Card"),
    (Preset::FlowPunchedTape, "flowChartPunchedTape", "Flowchart: Punched Tape"),
    (Preset::FlowSummingJunction, "flowChartSummingJunction", "Flowchart: Summing Junction"),
    (Preset::FlowOr, "flowChartOr", "Flowchart: Or"),
    (Preset::FlowCollate, "flowChartCollate", "Flowchart: Collate"),
    (Preset::FlowSort, "flowChartSort", "Flowchart: Sort"),
    (Preset::FlowExtract, "flowChartExtract", "Flowchart: Extract"),
    (Preset::FlowMerge, "flowChartMerge", "Flowchart: Merge"),
    (Preset::FlowStoredData, "flowChartOnlineStorage", "Flowchart: Stored Data"),
    (Preset::FlowDelay, "flowChartDelay", "Flowchart: Delay"),
    (
        Preset::FlowSequentialStorage,
        "flowChartMagneticTape",
        "Flowchart: Sequential Access Storage",
    ),
    (Preset::FlowMagneticDisk, "flowChartMagneticDisk", "Flowchart: Magnetic Disk"),
    (Preset::FlowDirectStorage, "flowChartMagneticDrum", "Flowchart: Direct Access Storage"),
    (Preset::FlowDisplay, "flowChartDisplay", "Flowchart: Display"),
    // The stars and banners, in the order Word's gallery shows them.
    (Preset::Explosion1, "irregularSeal1", "Explosion 1"),
    (Preset::Explosion2, "irregularSeal2", "Explosion 2"),
    (Preset::Star4, "star4", "4-Point Star"),
    (Preset::Star, "star5", "5-Point Star"),
    (Preset::Star6, "star6", "6-Point Star"),
    (Preset::Star7, "star7", "7-Point Star"),
    (Preset::Star8, "star8", "8-Point Star"),
    (Preset::Star10, "star10", "10-Point Star"),
    (Preset::Star12, "star12", "12-Point Star"),
    (Preset::Star16, "star16", "16-Point Star"),
    (Preset::Star24, "star24", "24-Point Star"),
    (Preset::Star32, "star32", "32-Point Star"),
    (Preset::UpRibbon, "ribbon2", "Up Ribbon"),
    (Preset::DownRibbon, "ribbon", "Down Ribbon"),
    (Preset::CurvedUpRibbon, "ellipseRibbon2", "Curved Up Ribbon"),
    (Preset::CurvedDownRibbon, "ellipseRibbon", "Curved Down Ribbon"),
    (Preset::VerticalScroll, "verticalScroll", "Vertical Scroll"),
    (Preset::HorizontalScroll, "horizontalScroll", "Horizontal Scroll"),
    (Preset::Wave, "wave", "Wave"),
    (Preset::DoubleWave, "doubleWave", "Double Wave"),
    (Preset::Line, "line", "Line"),
    (Preset::Callout, "wedgeRectCallout", "Speech Bubble"),
];

impl Preset {
    /// The name the format knows it by.
    #[must_use]
    pub fn word(self) -> &'static str {
        NAMED.iter().find(|(preset, ..)| *preset == self).map_or("rect", |(_, word, _)| *word)
    }

    /// Reads one back out of a document.
    ///
    /// A preset this does not know becomes a rectangle: the shape is then the
    /// wrong shape but the right size in the right place, which is a better
    /// showing of the document than leaving it out.
    #[must_use]
    pub fn from_word(name: &str) -> Self {
        NAMED
            .iter()
            .find(|(_, word, _)| *word == name)
            .map_or(Self::Rectangle, |(preset, ..)| *preset)
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        NAMED
            .iter()
            .find(|(preset, ..)| *preset == self)
            .map_or("Rectangle", |(_, _, label)| *label)
    }

    /// Whether the shape encloses an area that text could sit in.
    ///
    /// A line does not, which is why a line never carries text and never has a
    /// fill.
    #[must_use]
    pub fn is_closed(self) -> bool {
        self != Self::Line
    }

    /// Every preset that can be picked, in the order the gallery shows them.
    ///
    /// The same order as [`NAMED`], which is the format's own grouping: the
    /// rectangles, then the shapes of straight lines, then the ones made of
    /// arcs, and the odd ones at the end.
    #[must_use]
    pub fn all() -> Vec<Self> {
        NAMED.iter().map(|(preset, ..)| *preset).collect()
    }
}

/// The outline of a shape, inside the box given.
///
/// `x` and `y` are the top left corner and the box grows right and down, which
/// is how a canvas is measured.
#[must_use]
pub fn path_in(preset: Preset, x: f32, y: f32, width: f32, height: f32) -> Path {
    let mut path = Path::new();
    if width <= 0.0 || height <= 0.0 {
        return path;
    }
    let (left, top, right, bottom) = (x, y, x + width, y + height);
    let point = |px: f32, py: f32| Point::new(px, py);

    // The block arrows are their own file: there are twenty-eight of them and
    // they are nearly all one shape. See [`crate::arrows`].
    if let Some(arrow) = crate::arrows::path_in(preset, left, top, right, bottom) {
        return arrow;
    }
    // And so are the flowchart shapes, for the same reason and in the same
    // way. See [`crate::flowchart`].
    if let Some(shape) = crate::flowchart::path_in(preset, left, top, right, bottom) {
        return shape;
    }
    // And the stars and banners. See [`crate::banners`].
    if let Some(shape) = crate::banners::path_in(preset, left, top, right, bottom) {
        return shape;
    }

    match preset {
        Preset::Rectangle | Preset::Callout => {
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
            if preset == Preset::Callout {
                // The tail, hanging from the lower left of the bubble. Word
                // puts it wherever the shape's adjustment says; this puts it
                // where a bubble usually has one.
                let tip_x = left + width * 0.20;
                path.move_to(point(left + width * 0.18, bottom));
                path.line_to(point(left + width * 0.34, bottom));
                path.line_to(point(tip_x, bottom + height * 0.22));
                path.close();
            }
        }
        Preset::RoundedRectangle => {
            // A sixth of the shorter side, which is the corner Word rounds by
            // default.
            let radius = (width.min(height) / 6.0).min(width / 2.0).min(height / 2.0);
            rounded_rectangle(&mut path, left, top, right, bottom, radius);
        }
        Preset::Ellipse => {
            ellipse(
                &mut path,
                (left + right) / 2.0,
                (top + bottom) / 2.0,
                width / 2.0,
                height / 2.0,
            );
        }
        Preset::Triangle => {
            path.move_to(point((left + right) / 2.0, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::RightTriangle => {
            path.move_to(point(left, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::Diamond => {
            path.move_to(point((left + right) / 2.0, top));
            path.line_to(point(right, (top + bottom) / 2.0));
            path.line_to(point((left + right) / 2.0, bottom));
            path.line_to(point(left, (top + bottom) / 2.0));
            path.close();
        }
        Preset::Pentagon => regular(&mut path, left, top, width, height, 5),
        Preset::Hexagon => regular(&mut path, left, top, width, height, 6),
        Preset::Arrow => {
            // The shaft is the middle half of the height and the head takes the
            // last third of the width, which is the arrow Word draws.
            let shaft_top = top + height * 0.25;
            let shaft_bottom = bottom - height * 0.25;
            let neck = left + width * 0.66;
            path.move_to(point(left, shaft_top));
            path.line_to(point(neck, shaft_top));
            path.line_to(point(neck, top));
            path.line_to(point(right, (top + bottom) / 2.0));
            path.line_to(point(neck, bottom));
            path.line_to(point(neck, shaft_bottom));
            path.line_to(point(left, shaft_bottom));
            path.close();
        }
        // The rectangle gallery: the same rectangle with its corners treated
        // differently. Clockwise from the top left.
        Preset::SnipOneCorner => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Square, Corner::Snipped, Corner::Square, Corner::Square],
        ),
        Preset::SnipTwoSame => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Snipped, Corner::Snipped, Corner::Square, Corner::Square],
        ),
        Preset::SnipTwoDiagonal => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Snipped, Corner::Square, Corner::Snipped, Corner::Square],
        ),
        Preset::SnipAndRound => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Snipped, Corner::Square, Corner::Square, Corner::Rounded],
        ),
        Preset::RoundOneCorner => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Square, Corner::Rounded, Corner::Square, Corner::Square],
        ),
        Preset::RoundTwoSame => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Rounded, Corner::Rounded, Corner::Square, Corner::Square],
        ),
        Preset::RoundTwoDiagonal => cornered(
            &mut path,
            left,
            top,
            right,
            bottom,
            [Corner::Rounded, Corner::Square, Corner::Rounded, Corner::Square],
        ),

        // The rest of the regular polygons.
        Preset::Heptagon => regular(&mut path, left, top, width, height, 7),
        Preset::Octagon => regular(&mut path, left, top, width, height, 8),
        Preset::Decagon => regular(&mut path, left, top, width, height, 10),
        Preset::Dodecagon => regular(&mut path, left, top, width, height, 12),

        // And the shapes that are a handful of straight lines.
        Preset::Trapezoid => {
            let inset = width * 0.25;
            path.move_to(point(left + inset, top));
            path.line_to(point(right - inset, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::Parallelogram => {
            let lean = width * 0.25;
            path.move_to(point(left + lean, top));
            path.line_to(point(right, top));
            path.line_to(point(right - lean, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::Cross => {
            // The arms are a third of the way in from each side, which is the
            // cross the format draws when nothing says otherwise.
            let (arm_x, arm_y) = (width / 3.0, height / 3.0);
            path.move_to(point(left + arm_x, top));
            path.line_to(point(right - arm_x, top));
            path.line_to(point(right - arm_x, top + arm_y));
            path.line_to(point(right, top + arm_y));
            path.line_to(point(right, bottom - arm_y));
            path.line_to(point(right - arm_x, bottom - arm_y));
            path.line_to(point(right - arm_x, bottom));
            path.line_to(point(left + arm_x, bottom));
            path.line_to(point(left + arm_x, bottom - arm_y));
            path.line_to(point(left, bottom - arm_y));
            path.line_to(point(left, top + arm_y));
            path.line_to(point(left + arm_x, top + arm_y));
            path.close();
        }
        Preset::LShape => {
            let (arm_x, arm_y) = (width / 3.0, height / 3.0);
            path.move_to(point(left, top));
            path.line_to(point(left + arm_x, top));
            path.line_to(point(left + arm_x, bottom - arm_y));
            path.line_to(point(right, bottom - arm_y));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::HalfFrame => {
            // Two sides of a frame, mitred where they meet.
            let (arm_x, arm_y) = (width / 3.0, height / 3.0);
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right - arm_x, top + arm_y));
            path.line_to(point(left + arm_x, top + arm_y));
            path.line_to(point(left + arm_x, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::Frame => {
            // All four sides, which is a rectangle with a rectangle cut out.
            let (arm_x, arm_y) = (width / 8.0, height / 8.0);
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
            // The other way round, so the two leave a hole.
            path.move_to(point(left + arm_x, top + arm_y));
            path.line_to(point(left + arm_x, bottom - arm_y));
            path.line_to(point(right - arm_x, bottom - arm_y));
            path.line_to(point(right - arm_x, top + arm_y));
            path.close();
        }

        // The ones made of arcs. Angles are measured the way a canvas measures
        // them: clockwise, from the three o'clock position.
        Preset::Pie => wedge(
            &mut path,
            (left + right) / 2.0,
            (top + bottom) / 2.0,
            width / 2.0,
            height / 2.0,
            0.0,
            core::f32::consts::PI * 1.5,
            true,
        ),
        Preset::Chord => wedge(
            &mut path,
            (left + right) / 2.0,
            (top + bottom) / 2.0,
            width / 2.0,
            height / 2.0,
            0.0,
            core::f32::consts::PI * 1.5,
            false,
        ),
        Preset::Arc => {
            // A quarter of the way round, and open: an arc has no inside, so it
            // is drawn as a band rather than filled.
            band(
                &mut path,
                (left + right) / 2.0,
                (top + bottom) / 2.0,
                width / 2.0,
                height / 2.0,
                -core::f32::consts::FRAC_PI_2,
                0.0,
                width.min(height) / 16.0,
            );
        }
        Preset::Donut => ring(
            &mut path,
            (left + right) / 2.0,
            (top + bottom) / 2.0,
            width / 2.0,
            height / 2.0,
            width.min(height) / 6.0,
        ),
        Preset::NoSymbol => {
            ring(
                &mut path,
                (left + right) / 2.0,
                (top + bottom) / 2.0,
                width / 2.0,
                height / 2.0,
                width.min(height) / 6.0,
            );
            // The bar across it, from one side of the ring to the other.
            let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
            let (rx, ry) =
                (width / 2.0 - width.min(height) / 6.0, height / 2.0 - width.min(height) / 6.0);
            let thickness = width.min(height) / 12.0;
            let (sin, cos) = core::f32::consts::FRAC_PI_4.sin_cos();
            path.move_to(point(cx - rx * cos - thickness * sin, cy - ry * sin + thickness * cos));
            path.line_to(point(cx + rx * cos - thickness * sin, cy + ry * sin + thickness * cos));
            path.line_to(point(cx + rx * cos + thickness * sin, cy + ry * sin - thickness * cos));
            path.line_to(point(cx - rx * cos + thickness * sin, cy - ry * sin - thickness * cos));
            path.close();
        }
        Preset::BlockArc => band(
            &mut path,
            (left + right) / 2.0,
            (top + bottom) / 2.0,
            width / 2.0,
            height / 2.0,
            core::f32::consts::PI,
            core::f32::consts::TAU,
            width.min(height) / 4.0,
        ),
        Preset::Can => {
            // A cylinder seen from the side: the tube, and the ellipse that is
            // its top seen at an angle.
            let lid = height / 8.0;
            let (cx, rx) = ((left + right) / 2.0, width / 2.0);
            path.move_to(point(left, top + lid));
            arc_into(
                &mut path,
                cx,
                top + lid,
                rx,
                lid,
                core::f32::consts::PI,
                core::f32::consts::TAU,
            );
            path.line_to(point(right, bottom - lid));
            arc_into(&mut path, cx, bottom - lid, rx, lid, 0.0, core::f32::consts::PI);
            path.close();
            // The lid, which is what makes it a can rather than a tube.
            ellipse(&mut path, cx, top + lid, rx, lid);
        }
        Preset::Teardrop => {
            // A circle with one corner pulled out to a point.
            let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
            let (rx, ry) = (width / 2.0, height / 2.0);
            path.move_to(point(cx + rx, cy));
            arc_into(&mut path, cx, cy, rx, ry, 0.0, core::f32::consts::PI * 1.5);
            // And out to the corner, which is where the point goes.
            path.line_to(point(right, top));
            path.close();
        }
        Preset::Plaque => {
            // A rectangle with its corners taken *out* rather than in.
            let reach = width.min(height) / 6.0;
            path.move_to(point(left + reach, top));
            path.line_to(point(right - reach, top));
            path.cubic_to(
                point(right - reach, top + reach * QUARTER),
                point(right - reach * QUARTER, top + reach),
                point(right, top + reach),
            );
            path.line_to(point(right, bottom - reach));
            path.cubic_to(
                point(right - reach * QUARTER, bottom - reach),
                point(right - reach, bottom - reach * QUARTER),
                point(right - reach, bottom),
            );
            path.line_to(point(left + reach, bottom));
            path.cubic_to(
                point(left + reach, bottom - reach * QUARTER),
                point(left + reach * QUARTER, bottom - reach),
                point(left, bottom - reach),
            );
            path.line_to(point(left, top + reach));
            path.cubic_to(
                point(left + reach * QUARTER, top + reach),
                point(left + reach, top + reach * QUARTER),
                point(left + reach, top),
            );
            path.close();
        }
        Preset::Moon => {
            // A crescent, both of whose edges are inside the box: the left half
            // of the ellipse going out, and a shallower curve coming back. The
            // horns point right, which is the way the format draws it.
            let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
            let (rx, ry) = (width / 2.0, height / 2.0);
            path.move_to(point(cx, bottom));
            arc_into(
                &mut path,
                cx,
                cy,
                rx,
                ry,
                core::f32::consts::FRAC_PI_2,
                core::f32::consts::PI * 1.5,
            );
            // And back along a curve that bulges the same way the outer one
            // does, but less far — which is what takes a bite out of the disc
            // and leaves a crescent. Bulging the other way would close it up
            // into a shape fatter than the half it came round.
            let bulge = rx * 0.55;
            path.cubic_to(
                point(cx - bulge, top + height * 0.1),
                point(cx - bulge, bottom - height * 0.1),
                point(cx, bottom),
            );
            path.close();
        }
        Preset::Heart => {
            // Two lobes and a point. The control points sit on the edges of the
            // box rather than outside it, which makes the lobes a little less
            // round than the format's own and keeps the shape where it was put.
            let (cx, cy) = ((left + right) / 2.0, top + height * 0.25);
            path.move_to(point(cx, bottom));
            path.cubic_to(point(left, top + height * 0.60), point(left, top), point(cx, cy));
            path.cubic_to(point(right, top), point(right, top + height * 0.60), point(cx, bottom));
            path.close();
        }
        Preset::Line => {
            path.move_to(point(left, top));
            path.line_to(point(right, bottom));
        }
        // The block arrows answered above, before this match was reached: they
        // are in a file of their own. Naming them here would be naming them
        // twice, and the one thing worse than a shape in no list is a shape in
        // two. See [`crate::arrows`].
        _ => {
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
    }
    path
}

/// The band that draws a shape's outline.
///
/// The shape, and the shape a little smaller wound the other way — filled by
/// the nonzero rule the two leave a ring. A line has no inside, so its outline
/// is drawn as a long thin rectangle along it instead.
#[must_use]
pub fn outline_in(preset: Preset, x: f32, y: f32, width: f32, height: f32, weight: f32) -> Path {
    let weight = weight.max(0.5);
    if preset == Preset::Line {
        return thick_line(x, y, x + width, y + height, weight);
    }

    let mut path = path_in(preset, x, y, width, height);
    // The inside of the band: the same shape, inset by the weight on every
    // side, and reversed so that the nonzero rule leaves a hole rather than
    // filling it in twice.
    let inset_width = (width - weight * 2.0).max(0.0);
    let inset_height = (height - weight * 2.0).max(0.0);
    if inset_width <= 0.0 || inset_height <= 0.0 {
        return path;
    }
    let inner = path_in(preset, x + weight, y + weight, inset_width, inset_height);
    path.extend_reversed(&inner);
    // And the lines some shapes have inside them, which are drawn with the
    // same line as the outline and are part of neither the area of the shape
    // nor the band round it. See [`crate::flowchart::rules_into`].
    crate::flowchart::rules_into(&mut path, preset, x, y, width, height, weight);
    crate::banners::rules_into(&mut path, preset, x, y, width, height, weight);
    path
}

/// What a corner of a rectangle is done to.
///
/// The format's rectangle gallery is one shape with its four corners treated
/// four ways, and the nine entries in it are nine arrangements of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Corner {
    /// Left as it is.
    Square,
    /// Cut straight across.
    Snipped,
    /// Taken round.
    Rounded,
}

/// How far in a snip or a round reaches, as a fraction of the shorter side.
///
/// A sixth, which is what the format's own definitions use for every one of
/// these shapes when nothing says otherwise. It is a fraction of the *shorter*
/// side and not of each: the corner has to be square, or a snip on a wide
/// rectangle comes out as a long shallow wedge instead of a corner cut off.
const CORNER: f32 = 1.0 / 6.0;

/// A rectangle with each of its four corners done to as asked.
fn cornered(path: &mut Path, left: f32, top: f32, right: f32, bottom: f32, corners: [Corner; 4]) {
    let width = right - left;
    let height = bottom - top;
    let reach = (width.min(height) * CORNER).min(width / 2.0).min(height / 2.0);

    // Clockwise from the top left, which is the order the corners are given in.
    let places = [(left, top), (right, top), (right, bottom), (left, bottom)];
    // Which way along each edge the corner's two ends lie.
    let steps: [(f32, f32, f32, f32); 4] = [
        (0.0, reach, reach, 0.0),
        (-reach, 0.0, 0.0, reach),
        (0.0, -reach, -reach, 0.0),
        (reach, 0.0, 0.0, -reach),
    ];

    let mut started = false;
    for (index, corner) in corners.iter().enumerate() {
        let (x, y) = places[index];
        let (before_x, before_y, after_x, after_y) = steps[index];
        let (from, to) =
            (Point::new(x + before_x, y + before_y), Point::new(x + after_x, y + after_y));

        match corner {
            Corner::Square => {
                let point = Point::new(x, y);
                if started {
                    path.line_to(point);
                } else {
                    path.move_to(point);
                    started = true;
                }
            }
            Corner::Snipped | Corner::Rounded => {
                if started {
                    path.line_to(from);
                } else {
                    path.move_to(from);
                    started = true;
                }
                if *corner == Corner::Snipped {
                    path.line_to(to);
                } else {
                    // The corner itself pulls the curve, which is what makes a
                    // quarter circle out of two straight approaches.
                    path.cubic_to(
                        Point::new(
                            from.x + (x - from.x) * QUARTER,
                            from.y + (y - from.y) * QUARTER,
                        ),
                        Point::new(to.x + (x - to.x) * QUARTER, to.y + (y - to.y) * QUARTER),
                        to,
                    );
                }
            }
        }
    }
    path.close();
}

/// How far along a corner a control point sits for a cubic to pass for a
/// quarter circle. Out by less than a thousandth of the radius, which is far
/// less than a pixel at any size a shape is drawn.
const QUARTER: f32 = 0.552_284_8;

/// Adds a run of an ellipse to a path that is already somewhere.
///
/// Straight from where the path is to the first point of the run, and then
/// along it. Angles are in radians and go the way a canvas measures them:
/// clockwise, from the three o'clock position.
pub(crate) fn arc_into(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32) {
    let steps = ARC_STEPS * 2;
    for step in 0..=steps {
        let angle = from + (to - from) * step as f32 / steps as f32;
        let at = Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());
        // An arc that begins the shape says where it begins, because a path
        // whose first step is a line starts at the origin and drags an edge
        // across everything between there and the shape.
        if step == 0 && path.is_empty() {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
}

/// A wedge of an ellipse, from one angle to another.
///
/// `through_middle` says whether the two ends are joined through the middle —
/// which is the difference between a slice of pie and the same slice with its
/// crust cut straight across.
#[allow(clippy::too_many_arguments, reason = "an arc has a centre, two radii and two angles")]
fn wedge(
    path: &mut Path,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    from: f32,
    to: f32,
    through_middle: bool,
) {
    let steps = ARC_STEPS * 2;
    let at = |angle: f32| Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());

    if through_middle {
        path.move_to(Point::new(cx, cy));
        path.line_to(at(from));
    } else {
        path.move_to(at(from));
    }
    for step in 1..=steps {
        let angle = from + (to - from) * step as f32 / steps as f32;
        path.line_to(at(angle));
    }
    path.close();
}

/// A ring: an ellipse with a smaller one inside it, wound the other way so the
/// two leave a hole.
fn ring(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32, thickness: f32) {
    ellipse(path, cx, cy, rx, ry);
    let (inner_x, inner_y) = ((rx - thickness).max(0.0), (ry - thickness).max(0.0));
    if inner_x <= 0.0 || inner_y <= 0.0 {
        return;
    }
    // Wound the other way round, which is what leaves the hole.
    let steps = ARC_STEPS * 4;
    path.move_to(Point::new(cx + inner_x, cy));
    for step in 1..=steps {
        let angle = -core::f32::consts::TAU * step as f32 / steps as f32;
        path.line_to(Point::new(cx + inner_x * angle.cos(), cy + inner_y * angle.sin()));
    }
    path.close();
}

/// A band of an ellipse: the part between two angles and two radii.
#[allow(clippy::too_many_arguments, reason = "an arc has a centre, two radii and two angles")]
fn band(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32, thickness: f32) {
    let steps = ARC_STEPS * 2;
    let (inner_x, inner_y) = ((rx - thickness).max(0.0), (ry - thickness).max(0.0));
    let outer = |angle: f32| Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());
    let inner = |angle: f32| Point::new(cx + inner_x * angle.cos(), cy + inner_y * angle.sin());

    path.move_to(outer(from));
    for step in 1..=steps {
        path.line_to(outer(from + (to - from) * step as f32 / steps as f32));
    }
    path.line_to(inner(to));
    for step in 1..=steps {
        path.line_to(inner(to + (from - to) * step as f32 / steps as f32));
    }
    path.close();
}

/// A rectangle drawn along a line, which is how a line is given a width.
fn thick_line(x1: f32, y1: f32, x2: f32, y2: f32, weight: f32) -> Path {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let length = dx.hypot(dy);
    let mut path = Path::new();
    if length <= 0.0 {
        return path;
    }
    // The line's own direction turned a quarter, scaled to half the weight:
    // that is the offset from the middle of the line to either edge of it.
    let (nx, ny) = (-dy / length * weight / 2.0, dx / length * weight / 2.0);
    path.move_to(Point::new(x1 + nx, y1 + ny));
    path.line_to(Point::new(x2 + nx, y2 + ny));
    path.line_to(Point::new(x2 - nx, y2 - ny));
    path.line_to(Point::new(x1 - nx, y1 - ny));
    path.close();
    path
}

/// A rectangle with its corners taken off in quarter circles.
pub(crate) fn rounded_rectangle(
    path: &mut Path,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) {
    let point = Point::new;
    path.move_to(point(left + radius, top));
    path.line_to(point(right - radius, top));
    path.quad_to(point(right, top), point(right, top + radius));
    path.line_to(point(right, bottom - radius));
    path.quad_to(point(right, bottom), point(right - radius, bottom));
    path.line_to(point(left + radius, bottom));
    path.quad_to(point(left, bottom), point(left, bottom - radius));
    path.line_to(point(left, top + radius));
    path.quad_to(point(left, top), point(left + radius, top));
    path.close();
}

/// An ellipse about a centre, drawn as a run of quadratic curves.
pub(crate) fn ellipse(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32) {
    let steps = ARC_STEPS * 4;
    let angle_of = |step: usize| step as f32 / steps as f32 * core::f32::consts::TAU;
    let at = |angle: f32| Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());

    path.move_to(at(0.0));
    for step in 1..=steps {
        let previous = angle_of(step - 1);
        let angle = angle_of(step);
        // The control point of a quadratic through two points of a circle sits
        // where the two tangents meet, which is at the half-angle and further
        // out by the secant of it.
        let middle = (previous + angle) / 2.0;
        let reach = 1.0 / ((angle - previous) / 2.0).cos();
        let control = Point::new(cx + rx * reach * middle.cos(), cy + ry * reach * middle.sin());
        path.quad_to(control, at(angle));
    }
    path.close();
}

/// A regular polygon of `sides` sides, filling the box.
///
/// Point upwards, which is how a pentagon and a hexagon are both drawn.
fn regular(path: &mut Path, left: f32, top: f32, width: f32, height: f32, sides: usize) {
    let (cx, cy) = (left + width / 2.0, top + height / 2.0);
    let (rx, ry) = (width / 2.0, height / 2.0);
    let quarter = core::f32::consts::FRAC_PI_2;

    for step in 0..sides {
        let angle = -quarter + step as f32 / sides as f32 * core::f32::consts::TAU;
        let point = Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());
        if step == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
}

/// How far across a shape reaches between two heights.
///
/// # Why the text needs this
///
/// Word's Tight and Through wrapping run the text up to the shape itself
/// rather than to the box round it, so a line beside the point of a triangle
/// gets nearly the whole width and one beside its base gets none. Taking the
/// box for both is what makes text wrapped round a circle look wrapped round a
/// square.
///
/// The answer is the leftmost and rightmost the outline reaches anywhere in the
/// band, which is what a line of text in that band has to keep out of. Nothing
/// comes back when the shape does not reach into the band at all.
#[must_use]
pub fn span_between(
    preset: Preset,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    top: f32,
    bottom: f32,
) -> Option<(f32, f32)> {
    if bottom <= top || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let path = path_in(preset, x, y, width, height);

    let mut left = f32::MAX;
    let mut right = f32::MIN;
    let mut note = |point: Point| {
        left = left.min(point.x);
        right = right.max(point.x);
    };

    // The outline is walked as straight steps: a curve is flattened finely
    // enough that the answer is right to within a fraction of a pixel, which is
    // finer than a line of text can be placed anyway.
    let mut start = Point::new(x, y);
    let mut here = start;
    for command in &path.commands {
        match *command {
            Command::MoveTo(point) => {
                start = point;
                here = point;
            }
            Command::LineTo(point) => {
                clip_segment(here, point, top, bottom, &mut note);
                here = point;
            }
            Command::QuadTo(control, point) => {
                let mut previous = here;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let next = quadratic(here, control, point, t);
                    clip_segment(previous, next, top, bottom, &mut note);
                    previous = next;
                }
                here = point;
            }
            Command::CubicTo(first, second, point) => {
                let mut previous = here;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let next = cubic(here, first, second, point, t);
                    clip_segment(previous, next, top, bottom, &mut note);
                    previous = next;
                }
                here = point;
            }
            Command::Close => {
                clip_segment(here, start, top, bottom, &mut note);
                here = start;
            }
        }
    }

    (right > left).then_some((left, right))
}

/// How many straight steps a curve is walked in.
const CURVE_STEPS: usize = 16;

/// Notes where a straight step lies inside a band of heights.
fn clip_segment(from: Point, to: Point, top: f32, bottom: f32, note: &mut impl FnMut(Point)) {
    // `high` is the end nearer the top of the page, which is the smaller y.
    let (high, low) = if from.y <= to.y { (from, to) } else { (to, from) };
    // Wholly above the band, or wholly below it: nothing of it is beside the
    // line.
    if low.y < top || high.y > bottom {
        return;
    }

    // Both ends inside the band count as they are; an end outside is replaced
    // by where the step crosses the edge.
    for point in [high, low] {
        if point.y >= top && point.y <= bottom {
            note(point);
        }
    }
    for edge in [top, bottom] {
        if (high.y..=low.y).contains(&edge) && (low.y - high.y).abs() > f32::EPSILON {
            let share = (edge - high.y) / (low.y - high.y);
            note(Point::new(high.x + (low.x - high.x) * share, edge));
        }
    }
}

/// A point along a quadratic curve.
fn quadratic(from: Point, control: Point, to: Point, t: f32) -> Point {
    let inverse = 1.0 - t;
    Point::new(
        inverse * inverse * from.x + 2.0 * inverse * t * control.x + t * t * to.x,
        inverse * inverse * from.y + 2.0 * inverse * t * control.y + t * t * to.y,
    )
}

/// And along a cubic one.
fn cubic(from: Point, first: Point, second: Point, to: Point, t: f32) -> Point {
    let inverse = 1.0 - t;
    let (a, b, c, d) = (
        inverse * inverse * inverse,
        3.0 * inverse * inverse * t,
        3.0 * inverse * t * t,
        t * t * t,
    );
    Point::new(
        a * from.x + b * first.x + c * second.x + d * to.x,
        a * from.y + b * first.y + c * second.y + d * to.y,
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_reaches_right_across_at_every_height() {
        let span = span_between(Preset::Rectangle, 10.0, 20.0, 100.0, 50.0, 30.0, 40.0);
        let (left, right) = span.expect("a rectangle covers the band");
        assert!((left - 10.0).abs() < 0.5, "{left}");
        assert!((right - 110.0).abs() < 0.5, "{right}");
    }

    #[test]
    fn a_band_above_the_shape_is_not_beside_it() {
        assert_eq!(span_between(Preset::Rectangle, 10.0, 20.0, 100.0, 50.0, 0.0, 10.0), None);
    }

    #[test]
    fn a_band_below_the_shape_is_not_beside_it_either() {
        assert_eq!(span_between(Preset::Rectangle, 10.0, 20.0, 100.0, 50.0, 100.0, 120.0), None);
    }

    #[test]
    fn a_triangle_is_narrow_at_its_point_and_wide_at_its_base() {
        let point =
            span_between(Preset::Triangle, 0.0, 0.0, 100.0, 100.0, 0.0, 10.0).expect("the point");
        let base =
            span_between(Preset::Triangle, 0.0, 0.0, 100.0, 100.0, 90.0, 100.0).expect("the base");
        let width = |(left, right): (f32, f32)| right - left;
        assert!(width(point) < width(base) / 2.0, "{point:?} against {base:?}");
    }

    #[test]
    fn a_circle_is_widest_across_its_middle() {
        let middle =
            span_between(Preset::Ellipse, 0.0, 0.0, 100.0, 100.0, 45.0, 55.0).expect("the middle");
        let top =
            span_between(Preset::Ellipse, 0.0, 0.0, 100.0, 100.0, 0.0, 10.0).expect("the top");
        let width = |(left, right): (f32, f32)| right - left;
        assert!(width(middle) > width(top), "{middle:?} against {top:?}");
        // And as wide as the box it was given, near enough.
        assert!((width(middle) - 100.0).abs() < 2.0, "{middle:?}");
    }

    #[test]
    fn a_shape_never_reaches_outside_the_box_it_was_given() {
        for preset in Preset::all() {
            if preset == Preset::Callout {
                continue;
            }
            for band in 0..10 {
                let top = band as f32 * 10.0;
                let Some((left, right)) =
                    span_between(preset, 0.0, 0.0, 100.0, 100.0, top, top + 10.0)
                else {
                    continue;
                };
                assert!(left >= -0.5, "{} reaches to {left}", preset.word());
                assert!(right <= 100.5, "{} reaches to {right}", preset.word());
            }
        }
    }

    #[test]
    fn a_band_of_no_height_is_no_band() {
        assert_eq!(span_between(Preset::Rectangle, 0.0, 0.0, 100.0, 100.0, 50.0, 50.0), None);
    }

    /// The box a path covers, as left, top, right, bottom.
    fn bounds(path: &Path) -> (f32, f32, f32, f32) {
        let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for point in path.points() {
            bounds.0 = bounds.0.min(point.x);
            bounds.1 = bounds.1.min(point.y);
            bounds.2 = bounds.2.max(point.x);
            bounds.3 = bounds.3.max(point.y);
        }
        bounds
    }

    #[test]
    fn every_preset_survives_being_written_and_read_back() {
        for preset in Preset::all() {
            assert_eq!(Preset::from_word(preset.word()), preset);
        }
    }

    #[test]
    fn a_preset_nobody_knows_becomes_a_rectangle() {
        assert_eq!(Preset::from_word("cloudCallout"), Preset::Rectangle);
    }

    #[test]
    fn every_shape_stays_inside_the_box_it_was_given() {
        for preset in Preset::all() {
            // The bubble's tail hangs below the box on purpose, which is what a
            // tail is.
            if preset == Preset::Callout {
                continue;
            }
            let path = path_in(preset, 10.0, 20.0, 100.0, 60.0);
            let (left, top, right, bottom) = bounds(&path);
            assert!(left >= 9.9, "{} starts at {left}", preset.label());
            assert!(top >= 19.9, "{} starts at {top}", preset.label());
            assert!(right <= 110.1, "{} reaches {right}", preset.label());
            assert!(bottom <= 80.1, "{} reaches {bottom}", preset.label());
        }
    }

    #[test]
    fn every_shape_fills_the_box_it_was_given() {
        for preset in Preset::all() {
            // The arc is a quarter of an ellipse and covers a quarter of the
            // box, and the moon is a crescent down one side of it. Both are
            // as wide as what they are, which is less than the box.
            if matches!(preset, Preset::Arc | Preset::Moon) {
                continue;
            }
            let path = path_in(preset, 0.0, 0.0, 100.0, 60.0);
            let (left, top, right, bottom) = bounds(&path);
            assert!(right - left > 50.0, "{} is only {} wide", preset.label(), right - left);
            assert!(bottom - top > 30.0, "{} is only {} tall", preset.label(), bottom - top);
        }
    }

    #[test]
    fn a_box_of_no_size_draws_nothing() {
        for preset in Preset::all() {
            assert!(path_in(preset, 0.0, 0.0, 0.0, 50.0).points().next().is_none());
            assert!(path_in(preset, 0.0, 0.0, 50.0, -1.0).points().next().is_none());
        }
    }

    #[test]
    fn an_outline_is_a_ring_and_so_has_more_in_it_than_the_shape() {
        for preset in Preset::all() {
            if preset == Preset::Line {
                continue;
            }
            let shape = path_in(preset, 0.0, 0.0, 100.0, 60.0);
            let outline = outline_in(preset, 0.0, 0.0, 100.0, 60.0, 2.0);
            assert!(
                outline.points().count() > shape.points().count(),
                "{} has no inner edge",
                preset.label()
            );
        }
    }

    #[test]
    fn an_outline_thicker_than_the_shape_is_solid_rather_than_empty() {
        let outline = outline_in(Preset::Rectangle, 0.0, 0.0, 4.0, 4.0, 10.0);
        assert!(outline.points().next().is_some(), "a very thick outline should still draw");
    }

    #[test]
    fn a_line_is_drawn_as_a_band_along_itself() {
        let outline = outline_in(Preset::Line, 0.0, 0.0, 100.0, 0.0, 4.0);
        let (_, top, _, bottom) = bounds(&outline);
        assert!((bottom - top - 4.0).abs() < 0.01, "the band is {} thick", bottom - top);
    }

    #[test]
    fn only_a_line_is_open() {
        for preset in Preset::all() {
            assert_eq!(preset.is_closed(), preset != Preset::Line, "{}", preset.label());
        }
    }

    #[test]
    fn an_outline_really_does_leave_a_hole_when_it_is_filled() {
        use wp_raster::{Canvas, Color};

        let mut canvas = Canvas::filled(60, 60, Color::WHITE);
        let outline = outline_in(Preset::Rectangle, 10.0, 10.0, 40.0, 40.0, 4.0);
        canvas.fill_path(&outline, Color::BLACK);

        // On the band, and inside it where the hole is.
        assert_eq!(canvas.pixel(11, 30), Color::BLACK, "the band was not drawn");
        assert_eq!(canvas.pixel(30, 30), Color::WHITE, "the ring was filled in solid");
    }

    #[test]
    fn an_ellipse_reaches_the_middle_of_every_side() {
        let path = path_in(Preset::Ellipse, 0.0, 0.0, 100.0, 60.0);
        let (left, top, right, bottom) = bounds(&path);
        assert!((left - 0.0).abs() < 0.5 && (right - 100.0).abs() < 0.5, "{left} to {right}");
        assert!((top - 0.0).abs() < 0.5 && (bottom - 60.0).abs() < 0.5, "{top} to {bottom}");
    }
}

#[cfg(test)]
mod gallery_tests {
    use super::*;
    use wp_raster::{Canvas, Color};

    /// Draws a shape filled black on white paper, in a box of a given size.
    fn drawn(preset: Preset, size: usize) -> Canvas {
        let mut canvas = Canvas::filled(size, size, Color::WHITE);
        canvas.fill_path(&path_in(preset, 0.0, 0.0, size as f32, size as f32), Color::BLACK);
        canvas
    }

    fn is_ink(canvas: &Canvas, x: usize, y: usize) -> bool {
        canvas.pixel(x, y).red < 128
    }

    #[test]
    fn every_shape_says_where_it_starts() {
        // A path whose first step is a line starts at the origin, wherever the
        // shape itself is, and drags an edge across everything in between. It
        // is invisible to the tests that measure a shape, because they look at
        // the points the path names and the origin is not one of them.
        for preset in Preset::all() {
            let path = path_in(preset, 10.0, 20.0, 100.0, 60.0);
            assert!(
                matches!(path.commands.first(), Some(wp_raster::Command::MoveTo(_))),
                "{preset:?} starts with {:?}",
                path.commands.first()
            );
        }
    }

    #[test]
    fn every_preset_has_a_name_and_a_name_and_draws_something() {
        // The three things a shape has to have to be a shape anybody can use:
        // a name in the file, a name on the screen, and an outline.
        for preset in Preset::all() {
            assert!(!preset.word().is_empty(), "{preset:?} has no name in the format");
            assert!(!preset.label().is_empty(), "{preset:?} has no name to show");
            let path = path_in(preset, 0.0, 0.0, 40.0, 40.0);
            assert!(!path.is_empty(), "{preset:?} draws nothing");
        }
    }

    #[test]
    fn no_two_presets_share_a_name() {
        // Two shapes under one name is one shape nobody can open a document
        // into: whichever is found first wins and the other never draws.
        let all = Preset::all();
        for (index, preset) in all.iter().enumerate() {
            assert!(
                !all[..index].iter().any(|other| other.word() == preset.word()),
                "{preset:?} shares a name with something before it"
            );
            assert!(
                !all[..index].iter().any(|other| other.label() == preset.label()),
                "{preset:?} shares a label with something before it"
            );
        }
    }

    #[test]
    fn every_name_in_a_document_comes_back_as_the_shape_it_names() {
        for preset in Preset::all() {
            assert_eq!(Preset::from_word(preset.word()), preset, "{preset:?}");
        }
        // And a name this program does not know draws as a rectangle, which is
        // wrong but is visibly a shape of the right size in the right place.
        assert_eq!(Preset::from_word("cloudCallout"), Preset::Rectangle);
    }

    #[test]
    fn every_closed_shape_fills_its_own_middle() {
        // Every one of them but the ring-shaped few, which are hollow there on
        // purpose, and the line, which has no inside at all.
        // The ones that are hollow there on purpose: the rings; the two shapes
        // that are an edge of a frame rather than a frame, since an L has
        // nothing in the middle of it and that is what makes it an L; and the
        // moon, whose middle is the bite taken out of it.
        let hollow = [
            Preset::Donut,
            Preset::BlockArc,
            Preset::Frame,
            Preset::Arc,
            Preset::LShape,
            Preset::HalfFrame,
            Preset::Moon,
            // And the U-turn, which is a horseshoe: the gap between its two
            // legs is what makes it a U rather than a bar. The three elbows are
            // hollow in the corner they turn, for the same reason the L shape
            // is.
            Preset::UturnArrow,
            Preset::BentArrow,
            Preset::BentUpArrow,
            Preset::LeftUpArrow,
            // The curved arrows and the circular one are bands bent round a
            // middle they do not cover — an arc with a head on it.
            Preset::CurvedRightArrow,
            Preset::CurvedLeftArrow,
            Preset::CurvedUpArrow,
            Preset::CurvedDownArrow,
            Preset::CircularArrow,
            // The two curved ribbons bow away from the middle of the box: the
            // band is along the top of it, the tails hang at the sides, and
            // what is in the middle is the gap between them.
            Preset::CurvedUpRibbon,
            Preset::CurvedDownRibbon,
            // And the swoosh, which is a stroke rather than a solid: it rises
            // from one corner to another and the middle of the box is above
            // it.
            Preset::SwooshArrow,
        ];
        for preset in Preset::all() {
            if preset == Preset::Line || hollow.contains(&preset) {
                continue;
            }
            let canvas = drawn(preset, 40);
            assert!(is_ink(&canvas, 20, 20), "{preset:?} has nothing in the middle of it");
        }
    }

    #[test]
    fn the_stars_and_banners_are_all_twenty_of_them() {
        // Word's own section: two explosions, ten stars, four ribbons, two
        // scrolls and two waves.
        let count = Preset::all()
            .iter()
            .skip_while(|preset| **preset != Preset::Explosion1)
            .take_while(|preset| **preset != Preset::Line)
            .count();
        assert_eq!(count, 20, "Word draws twenty stars and banners");
    }

    #[test]
    fn a_star_has_two_corners_for_every_point_it_is_named_after() {
        // A point and the dip beside it. Without the dips a ten-pointed star
        // would be a decagon, which is a shape this gallery already has.
        for (preset, points) in [(Preset::Star4, 4), (Preset::Star, 5), (Preset::Star32, 32)] {
            let path = path_in(preset, 0.0, 0.0, 100.0, 60.0);
            assert_eq!(path.points().count(), points * 2, "{}", preset.label());
        }
    }

    #[test]
    fn which_way_up_a_ribbon_goes_is_the_whole_difference() {
        // The panel of an up ribbon is at the top of the box and its tails
        // hang lower, so the space under the panel of the one is above the
        // panel of the other. The down ribbon is drawn as this turned over.
        let up = drawn(Preset::UpRibbon, 40);
        let down = drawn(Preset::DownRibbon, 40);
        assert!(is_ink(&up, 20, 2), "the up ribbon should have its panel at the top");
        assert!(!is_ink(&up, 20, 38), "and nothing below it");
        assert!(is_ink(&down, 20, 38), "the down ribbon should have its panel at the bottom");
        assert!(!is_ink(&down, 20, 2), "and nothing above it");
    }

    #[test]
    fn a_wave_is_the_same_depth_all_the_way_along() {
        // The top of a wave and its bottom undulate together. Mirroring the
        // one to get the other would give a shape that bulges in the middle
        // and pinches at the ends, which is not a wave.
        let canvas = drawn(Preset::Wave, 40);
        let depth = |x: usize| (0..40).filter(|y| is_ink(&canvas, x, *y)).count();
        let middle = depth(20);
        for across in [6, 14, 26, 33] {
            assert!(
                depth(across).abs_diff(middle) <= 1,
                "at {across} the wave is {} deep and in the middle {middle}",
                depth(across)
            );
        }
    }

    #[test]
    fn the_flowchart_gallery_has_all_twenty_eight_of_them() {
        let count = Preset::all().iter().filter(|p| p.word().starts_with("flowChart")).count();
        assert_eq!(count, 28, "Word draws twenty-eight flowchart shapes");
    }

    #[test]
    fn the_rules_inside_a_shape_are_drawn_with_its_outline() {
        // A predefined process is a process with a rule down each end. The two
        // cover the same area, so the rules are no part of it: what tells them
        // apart is drawn with the line round the shape, and a shape drawn with
        // no line has none of them.
        let process = path_in(Preset::FlowProcess, 0.0, 0.0, 100.0, 60.0);
        let predefined = path_in(Preset::FlowPredefinedProcess, 0.0, 0.0, 100.0, 60.0);
        assert_eq!(process, predefined, "the two shapes are the same rectangle");

        let plain = outline_in(Preset::FlowProcess, 0.0, 0.0, 100.0, 60.0, 2.0);
        let ruled = outline_in(Preset::FlowPredefinedProcess, 0.0, 0.0, 100.0, 60.0, 2.0);
        assert!(ruled.points().count() > plain.points().count(), "the rules were not drawn");
    }

    #[test]
    fn a_rule_does_not_cut_a_gap_in_the_outline_it_runs_into() {
        // The rules are filled by the same nonzero rule as the band round the
        // shape, so one wound the other way would cancel the band where the
        // two overlap and draw a gap across it instead of a line.
        let mut canvas = Canvas::filled(40, 40, Color::WHITE);
        canvas.fill_path(&outline_in(Preset::FlowOr, 0.0, 0.0, 40.0, 40.0, 3.0), Color::BLACK);
        assert!(is_ink(&canvas, 20, 20), "the rules were not drawn");
        assert!(is_ink(&canvas, 20, 1), "the outline has a gap where a rule runs into it");
    }

    #[test]
    fn the_ring_shaped_ones_are_hollow_in_the_middle() {
        for preset in [Preset::Donut, Preset::Frame] {
            let canvas = drawn(preset, 40);
            assert!(!is_ink(&canvas, 20, 20), "{preset:?} should have a hole in it");
            // And ink on the ring itself, or it is not a ring but nothing.
            assert!(is_ink(&canvas, 20, 2), "{preset:?} should have a rim");
        }
    }

    #[test]
    fn the_no_symbol_is_a_ring_with_a_bar_across_it() {
        // The bar is the whole point of it, and it goes through the middle —
        // so unlike the other rings this one is not hollow there.
        let canvas = drawn(Preset::NoSymbol, 40);
        assert!(is_ink(&canvas, 20, 2), "the ring");
        assert!(is_ink(&canvas, 20, 20), "and the bar across the middle");
        // Either side of the bar, inside the ring, there is nothing.
        assert!(!is_ink(&canvas, 28, 14), "and the ring is hollow beside it");
    }

    #[test]
    fn a_snipped_corner_is_gone_and_the_others_are_not() {
        let canvas = drawn(Preset::SnipOneCorner, 40);
        assert!(!is_ink(&canvas, 38, 1), "the top right should be snipped off");
        assert!(is_ink(&canvas, 1, 1), "and the top left should not be");
        assert!(is_ink(&canvas, 1, 38), "nor the bottom left");
        assert!(is_ink(&canvas, 38, 38), "nor the bottom right");
    }

    #[test]
    fn two_corners_on_the_same_side_are_not_two_on_the_diagonal() {
        let same = drawn(Preset::SnipTwoSame, 40);
        assert!(!is_ink(&same, 1, 1), "the top left should be snipped");
        assert!(!is_ink(&same, 38, 1), "and the top right");
        assert!(is_ink(&same, 1, 38), "and the bottom left should not be");

        let diagonal = drawn(Preset::SnipTwoDiagonal, 40);
        assert!(!is_ink(&diagonal, 1, 1), "the top left should be snipped");
        assert!(is_ink(&diagonal, 38, 1), "and the top right should not be");
        assert!(!is_ink(&diagonal, 38, 38), "and the bottom right should be");
    }

    #[test]
    fn a_rounded_corner_takes_less_away_than_a_snipped_one() {
        // A quarter circle covers more of its corner than the straight cut
        // across it does, so the very corner goes and the rest stays.
        let rounded = drawn(Preset::RoundOneCorner, 40);
        assert!(!is_ink(&rounded, 39, 0), "the very corner should be gone");
        assert!(is_ink(&rounded, 34, 3), "but not the whole of it");
    }

    #[test]
    fn a_shape_keeps_its_corners_out_of_its_own_box() {
        // Every shape is drawn inside the box it is given and nowhere else.
        // The speech bubble is the one exception, and says so: its tail hangs
        // below the bubble, which is what makes it a bubble.
        for preset in Preset::all() {
            if preset == Preset::Callout {
                continue;
            }
            let path = path_in(preset, 10.0, 10.0, 20.0, 20.0);
            for point in path.points() {
                assert!(
                    point.x >= 9.5 && point.x <= 30.5 && point.y >= 9.5 && point.y <= 30.5,
                    "{preset:?} draws outside its box, at ({}, {})",
                    point.x,
                    point.y
                );
            }
        }
    }

    #[test]
    fn a_box_of_nothing_draws_nothing() {
        for preset in Preset::all() {
            assert!(path_in(preset, 0.0, 0.0, 0.0, 10.0).is_empty(), "{preset:?}");
            assert!(path_in(preset, 0.0, 0.0, 10.0, 0.0).is_empty(), "{preset:?}");
        }
    }

    #[test]
    fn a_pie_is_a_wedge_and_a_chord_is_not() {
        // Both are three quarters of an ellipse; the pie is closed through the
        // middle and the chord straight across, so the middle of the quarter
        // that is missing is outside one and inside the other.
        let pie = drawn(Preset::Pie, 40);
        let chord = drawn(Preset::Chord, 40);
        // The wedge runs from three o'clock round through six and nine to
        // twelve, so what is missing is the quarter at the top right. The two
        // differ over the triangle between the two radii and the straight line
        // joining their ends: the pie is bounded by the radii and the chord by
        // the line.
        assert!(!is_ink(&pie, 28, 14), "the pie should be open there");
        assert!(is_ink(&chord, 28, 14), "and the chord should be closed across it");
    }

    #[test]
    fn a_can_has_a_lid() {
        // The line across the top is what makes it a can rather than a tube,
        // and it is drawn with the fill rather than with the pen.
        let canvas = drawn(Preset::Can, 40);
        assert!(is_ink(&canvas, 20, 3), "the lid");
        assert!(is_ink(&canvas, 20, 36), "and the bottom of the tube");
    }

    #[test]
    fn a_heart_is_two_lobes_and_a_point() {
        let canvas = drawn(Preset::Heart, 40);
        assert!(is_ink(&canvas, 10, 12), "the left lobe");
        assert!(is_ink(&canvas, 30, 12), "the right lobe");
        assert!(is_ink(&canvas, 20, 34), "and the point at the bottom");
        assert!(!is_ink(&canvas, 20, 1), "with the dip between them at the top");
    }

    #[test]
    fn a_moon_is_a_crescent_and_not_a_disc() {
        // The bite out of it is the whole point: a moon with ink in the middle
        // of its box is a disc that has been drawn the wrong way round.
        let canvas = drawn(Preset::Moon, 40);
        assert!(is_ink(&canvas, 3, 20), "the outer edge");
        assert!(!is_ink(&canvas, 20, 20), "and the bite out of the middle");
        assert!(is_ink(&canvas, 12, 4), "with the horns reaching the top");
    }

    #[test]
    fn a_cross_reaches_all_four_sides() {
        let canvas = drawn(Preset::Cross, 40);
        assert!(is_ink(&canvas, 20, 1), "the top arm");
        assert!(is_ink(&canvas, 20, 38), "the bottom arm");
        assert!(is_ink(&canvas, 1, 20), "the left arm");
        assert!(is_ink(&canvas, 38, 20), "the right arm");
        assert!(!is_ink(&canvas, 2, 2), "and the corners are not part of it");
    }

    #[test]
    fn an_l_is_a_bar_down_and_a_bar_along() {
        let canvas = drawn(Preset::LShape, 40);
        assert!(is_ink(&canvas, 5, 5), "the upright");
        assert!(is_ink(&canvas, 35, 35), "the foot");
        assert!(!is_ink(&canvas, 35, 5), "and nothing in the corner between them");
    }

    #[test]
    fn a_trapezoid_leans_in_and_a_parallelogram_leans_over() {
        let trapezoid = drawn(Preset::Trapezoid, 40);
        assert!(!is_ink(&trapezoid, 2, 2), "the top corners are cut off both sides");
        assert!(!is_ink(&trapezoid, 37, 2));
        assert!(is_ink(&trapezoid, 2, 37), "and the bottom ones are not");

        let parallelogram = drawn(Preset::Parallelogram, 40);
        assert!(!is_ink(&parallelogram, 2, 2), "the top left is cut off");
        assert!(is_ink(&parallelogram, 37, 2), "and the top right is not");
        assert!(is_ink(&parallelogram, 2, 37), "and the bottom left is not");
    }
}
