//! The art borders: which names the format has, and which of them are patterns.
//!
//! # What an art border is
//!
//! One of about a hundred and sixty extra values `w:val` may take on a page
//! border — `apples`, `hearts`, `rope`, `zigZag` — each of which Word draws by
//! repeating a picture along the edge instead of drawing a line. They live in
//! the same attribute and the same enumeration as `single` and `double`, and
//! they differ from those in one way that matters to anything reading a file:
//!
//! **`w:sz` is in points for an art border**, one to thirty-one, where for a
//! line it is in eighths of a point. A program that read them the same way
//! would draw a twenty-point border of apples as a two-and-a-half-point one.
//! That is why the whole list is here rather than only the part that is drawn:
//! knowing a name is art is knowing what its width means.
//!
//! # Why only some of them are offered
//!
//! Because the pictures are Word's. They are artwork Microsoft ships, they are
//! not licensed for reuse, and this program draws nothing it did not make —
//! the same reason `chrome/icons.rs` draws its own buttons.
//!
//! But a good many of the names do not describe a picture at all. They describe
//! a pattern: `basicBlackSquares` is a row of black squares, `triangles` is a
//! row of triangles, `checkered` is a checkerboard. A row of black squares
//! drawn from rectangles is a row of black squares, and copies nothing. Those
//! are in [`DRAWN`], and [`crate::Document`] offers them; the rest are kept
//! exactly as they came and drawn as a plain line of their width.

/// Every art value `w:val` may take, in the order the format lists them.
///
/// The whole enumeration, not only the part that is drawn: this is what says
/// whether a border's width is in points or in eighths of one.
pub const ALL: &[&str] = &[
    "apples",
    "archedScallops",
    "babyPacifier",
    "babyRattle",
    "balloons3Colors",
    "balloonsHotAir",
    "basicBlackDashes",
    "basicBlackDots",
    "basicBlackSquares",
    "basicThinLines",
    "basicWhiteDashes",
    "basicWhiteDots",
    "basicWhiteSquares",
    "basicWideInline",
    "basicWideMidline",
    "basicWideOutline",
    "bats",
    "birds",
    "birdsFlight",
    "cabins",
    "cakeSlice",
    "candyCorn",
    "celticKnotwork",
    "certificateBanner",
    "chainLink",
    "champagneBottle",
    "checkedBarBlack",
    "checkedBarColor",
    "checkered",
    "christmasTree",
    "circlesLines",
    "circlesRectangles",
    "classicalWave",
    "clocks",
    "compass",
    "confetti",
    "confettiGrays",
    "confettiOutline",
    "confettiStreamers",
    "confettiWhite",
    "cornerTriangles",
    "couponCutoutDashes",
    "couponCutoutDots",
    "crazyMaze",
    "creaturesButterfly",
    "creaturesFish",
    "creaturesInsects",
    "creaturesLadyBug",
    "crossStitch",
    "cup",
    "decoArch",
    "decoArchColor",
    "decoBlocks",
    "diamondsGray",
    "doubleD",
    "doubleDiamonds",
    "earth1",
    "earth2",
    "eclipsingSquares1",
    "eclipsingSquares2",
    "eggsBlack",
    "fans",
    "film",
    "firecracker",
    "flowersBlockPrint",
    "flowersDaisies",
    "flowersModern1",
    "flowersModern2",
    "flowersPansy",
    "flowersRedRose",
    "flowersRoses",
    "flowersTeacup",
    "flowersTiny",
    "gems",
    "gingerbreadMan",
    "gradient",
    "handmade1",
    "handmade2",
    "heartBalloon",
    "heartGray",
    "hearts",
    "heebieJeebies",
    "holly",
    "houseFunky",
    "hypnotic",
    "iceCreamCones",
    "lightBulb",
    "lightning1",
    "lightning2",
    "mapPins",
    "mapleLeaf",
    "mapleMuffins",
    "marquee",
    "marqueeToothed",
    "moons",
    "mosaic",
    "musicNotes",
    "northwest",
    "ovals",
    "packages",
    "palmsBlack",
    "palmsColor",
    "paperClips",
    "papyrus",
    "partyFavor",
    "partyGlass",
    "pencils",
    "people",
    "peopleHats",
    "peopleWaving",
    "poinsettias",
    "postageStamp",
    "pumpkin1",
    "pushPinNote1",
    "pushPinNote2",
    "pyramids",
    "pyramidsAbove",
    "quadrants",
    "rings",
    "safari",
    "sawtooth",
    "sawtoothGray",
    "scaredCat",
    "seattle",
    "shadowedSquares",
    "sharksTeeth",
    "shorebirdTracks",
    "skyrocket",
    "snowflakeFancy",
    "snowflakes",
    "sombrero",
    "southwest",
    "stars",
    "stars3d",
    "starsBlack",
    "starsShadowed",
    "starsTop",
    "sun",
    "swirligig",
    "tornPaper",
    "tornPaperBlack",
    "trees",
    "triangleParty",
    "triangles",
    "tribal1",
    "tribal2",
    "tribal3",
    "tribal4",
    "tribal5",
    "tribal6",
    "twistedLines1",
    "twistedLines2",
    "vine",
    "waveline",
    "weavingAngles",
    "weavingBraid",
    "weavingRibbon",
    "weavingStrips",
    "whiteFlowers",
    "woodwork",
    "xIllusions",
    "zanyTriangles",
    "zigZag",
    "zigZagStitch",
];

/// The ones that are a pattern rather than a picture, with what to call them.
///
/// Each is drawn from rectangles by `wp-layout`, as its name describes. The
/// label is what a person picking one reads; the name is what goes in the file.
pub const DRAWN: &[(&str, &str)] = &[
    ("Black dashes", "basicBlackDashes"),
    ("Black dots", "basicBlackDots"),
    ("Black squares", "basicBlackSquares"),
    ("White dashes", "basicWhiteDashes"),
    ("White dots", "basicWhiteDots"),
    ("White squares", "basicWhiteSquares"),
    ("Thin lines", "basicThinLines"),
    ("Wide, line inside", "basicWideInline"),
    ("Wide, line down the middle", "basicWideMidline"),
    ("Wide outline", "basicWideOutline"),
    ("Checkerboard", "checkered"),
    ("Checked bar", "checkedBarBlack"),
    ("Classical wave", "classicalWave"),
    ("Cross stitch", "crossStitch"),
    ("Grey diamonds", "diamondsGray"),
    ("Eclipsing squares", "eclipsingSquares1"),
    ("Squares within squares", "eclipsingSquares2"),
    ("Gradient", "gradient"),
    ("Hatching, north-west", "northwest"),
    ("Hatching, south-west", "southwest"),
    ("Quadrants", "quadrants"),
    ("Sawtooth", "sawtooth"),
    ("Grey sawtooth", "sawtoothGray"),
    ("Shadowed squares", "shadowedSquares"),
    ("Shark's teeth", "sharksTeeth"),
    ("Triangles", "triangles"),
    ("Wave", "waveline"),
    ("Zigzag", "zigZag"),
    ("Zigzag stitch", "zigZagStitch"),
];

/// The widest an art border may be, in points, which is as far as `w:sz` goes
/// for one.
pub const WIDEST: u32 = 31;

/// What Word gives a border of art when it is first asked for, in points.
pub const USUAL_WIDTH: u32 = 20;

/// Whether a `w:val` names an art border rather than a line.
#[must_use]
pub fn is_art(style: &str) -> bool {
    ALL.binary_search(&style).is_ok()
}

/// Whether this program draws that art as itself.
#[must_use]
pub fn is_drawn(style: &str) -> bool {
    DRAWN.iter().any(|(_, name)| *name == style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_in_order_so_it_can_be_searched() {
        // `is_art` looks a name up by halving the list, which is only an answer
        // if the list is sorted. A name added out of order would make some
        // other name unfindable, and a border twenty points wide would come out
        // two and a half.
        assert!(ALL.windows(2).all(|pair| pair[0] < pair[1]), "the art names are out of order");
    }

    #[test]
    fn an_art_name_is_art_and_a_line_style_is_not() {
        for name in ["apples", "zigZag", "basicBlackDots", "hearts", "tribal6"] {
            assert!(is_art(name), "{name} should be art");
        }
        for name in ["single", "double", "dotted", "dashDotStroked", "threeDEmboss", "none", ""] {
            assert!(!is_art(name), "{name} should be a line");
        }
    }

    #[test]
    fn everything_offered_is_a_name_the_format_has() {
        for (label, name) in DRAWN {
            assert!(is_art(name), "{label} is written as {name:?}, which is not an art border");
        }
    }

    #[test]
    fn nothing_is_offered_twice() {
        for (at, (_, name)) in DRAWN.iter().enumerate() {
            assert!(
                !DRAWN[..at].iter().any(|(_, earlier)| earlier == name),
                "{name} is offered twice"
            );
        }
    }

    #[test]
    fn the_ones_drawn_are_a_part_of_the_whole_gallery_and_not_all_of_it() {
        // If this ever stopped being true, the note in the roadmap about the
        // rest being kept rather than drawn would have stopped being true too.
        assert!(DRAWN.len() < ALL.len() / 2);
        assert!(DRAWN.len() > 20, "hardly any of the patterns are offered");
    }
}
