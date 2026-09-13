//! What a connector is fastened to: `a:stCxn` and `a:endCxn`.
//!
//! # Why a connector is not simply a line in a box
//!
//! A line drawn between two shapes is a line: move either shape and it stays
//! where it was, pointing at nothing. A *connector* is fastened to them, and
//! the file says so — which end is fastened to which shape, and to which of
//! that shape's connection points. Where the connector is then drawn follows
//! from where those two shapes are, and the box it was saved with is only the
//! answer from the last time anybody worked it out.
//!
//! # Which id
//!
//! The one on the shape's own non-visual properties, `wps:cNvPr/@id`, and not
//! the one on the drawing that wraps it. Two different numbers live a few
//! elements apart in the same file and only one of them is the one a connector
//! names.

use wp_xml::tree::Element;

/// One end of a connector, fastened to a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Join {
    /// The `wps:cNvPr/@id` of the shape this end is fastened to.
    pub shape: u32,
    /// Which of that shape's connection points, counted the way the format
    /// counts them. See [`wp_layout`'s geometry] for where each preset puts
    /// them — for nearly every shape the first four are the top, the left, the
    /// bottom and the right.
    pub site: u32,
}

/// Both ends of a connector, either of which may be fastened to nothing.
///
/// A connector with one end loose is an ordinary thing: it is pinned to a shape
/// at one end and left where it was drawn at the other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Joins {
    pub start: Option<Join>,
    pub end: Option<Join>,
}

impl Joins {
    /// Whether either end is fastened to anything.
    #[must_use]
    pub fn is_nothing(self) -> bool {
        self.start.is_none() && self.end.is_none()
    }

    /// Whether either end is fastened to the shape with this id.
    #[must_use]
    pub fn holds(self, shape: u32) -> bool {
        [self.start, self.end].into_iter().flatten().any(|join| join.shape == shape)
    }
}

/// What the connector properties say the two ends are fastened to.
#[must_use]
pub fn read_joins(properties: &Element) -> Joins {
    Joins { start: read_join(properties, "stCxn"), end: read_join(properties, "endCxn") }
}

fn read_join(properties: &Element, local: &str) -> Option<Join> {
    let end = properties.child_elements().find(|child| child.local_name() == local)?;
    Some(Join {
        shape: end.attribute_by_name("id")?.parse().ok()?,
        site: end.attribute_by_name("idx").and_then(|idx| idx.parse().ok()).unwrap_or(0),
    })
}

/// And the same written back out, as the children of `wps:cNvCnPr`.
#[must_use]
pub fn join_elements(joins: Joins) -> Vec<Element> {
    [("a:stCxn", joins.start), ("a:endCxn", joins.end)]
        .into_iter()
        .filter_map(|(name, join)| {
            let join = join?;
            let mut element = Element::new(name, Some(crate::shapes::A));
            element.set_attribute("id", &join.shape.to_string());
            element.set_attribute("idx", &join.site.to_string());
            Some(element)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_end_fastened_to_nothing_is_written_as_nothing() {
        assert!(join_elements(Joins::default()).is_empty());
    }

    #[test]
    fn a_connector_knows_which_shapes_it_holds() {
        let joins = Joins {
            start: Some(Join { shape: 4, site: 3 }),
            end: Some(Join { shape: 7, site: 1 }),
        };
        assert!(joins.holds(4) && joins.holds(7));
        assert!(!joins.holds(5), "a shape it is not fastened to");
    }
}
