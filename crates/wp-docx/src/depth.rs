//! What makes a shape solid: `a:sp3d` and `a:scene3d`.
//!
//! A bevel round its edge, a depth behind it, what the sides of that depth are
//! made of, and where the scene is looked at from and lit from.
//!
//! # Why these are two elements and not one
//!
//! Because one of them belongs to the shape and the other to the room it is in.
//! The bevel and the depth are the shape's own — how thick it is, what its edge
//! is rolled to, what it is made of. The camera and the lighting are the
//! scene's, and every shape in a document that shares a scene is seen from the
//! same place. The format keeps them apart and so does this.
//!
//! # The units
//!
//! The format's own: English metric units for a width or a depth, and sixtieths
//! of a degree for every angle. Nothing is turned into pixels here, for the same
//! reason nothing else is: how big a pixel is depends on the zoom.

use wp_xml::tree::Element;

/// How solid the shape itself is: `a:sp3d`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Depth {
    /// The edge rolled over at the front and at the back.
    pub bevel_top: Option<Bevel>,
    pub bevel_bottom: Option<Bevel>,
    /// How far back the shape goes, and what colour its sides are. A depth with
    /// no colour takes the shape's own fill, which is what Word does.
    pub extrusion_emu: i64,
    pub extrusion_colour: Option<String>,
    /// The line drawn round the whole solid, which is not the shape's outline:
    /// it follows the silhouette of the solid and not the edge of the face.
    pub contour_emu: i64,
    pub contour_colour: Option<String>,
    /// What it is made of, by the format's own name: `matte`, `plastic`,
    /// `metal` and the rest. What it changes is how sharply the bevel catches
    /// the light.
    pub material: String,
}

impl Depth {
    /// Whether the shape is solid at all, which most are not.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.bevel_top.is_none()
            && self.bevel_bottom.is_none()
            && self.extrusion_emu == 0
            && self.contour_emu == 0
    }
}

/// An edge rolled over.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bevel {
    pub width_emu: i64,
    pub height_emu: i64,
    /// The shape of the roll, by the format's name for it: `circle`, `relaxedInset`,
    /// `slope` and the rest. Kept as it was written, because a bevel drawn as
    /// the wrong roll is still nearer the truth than a flat edge.
    pub kind: String,
}

/// Where the scene is seen from and lit from: `a:scene3d`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    /// The camera, by the format's name: `orthographicFront` and the rest.
    pub camera: String,
    /// How far round the scene is turned, in sixtieths of a degree.
    pub latitude: i32,
    pub longitude: i32,
    pub revolution: i32,
    /// The lighting, by its name, and which way it comes from.
    pub light: String,
    pub light_from: String,
}

impl Scene {
    /// Whether anything says how the scene is seen.
    #[must_use]
    pub fn is_nothing(&self) -> bool {
        self.camera.is_empty() && self.light.is_empty()
    }
}

/// What the shape's properties say about how solid it is.
#[must_use]
pub fn read_depth(properties: &Element) -> Depth {
    let Some(solid) = child(properties, "sp3d") else {
        return Depth::default();
    };
    Depth {
        bevel_top: child(solid, "bevelT").map(read_bevel),
        bevel_bottom: child(solid, "bevelB").map(read_bevel),
        extrusion_emu: number(solid, "extrusionH"),
        extrusion_colour: child(solid, "extrusionClr").and_then(colour_of),
        contour_emu: number(solid, "contourW"),
        contour_colour: child(solid, "contourClr").and_then(colour_of),
        material: solid.attribute_by_name("prstMaterial").unwrap_or_default().to_owned(),
    }
}

/// And what they say about the scene it stands in.
#[must_use]
pub fn read_scene(properties: &Element) -> Scene {
    let Some(scene) = child(properties, "scene3d") else {
        return Scene::default();
    };
    let camera = child(scene, "camera");
    let turn = camera.and_then(|camera| child(camera, "rot"));
    let light = child(scene, "lightRig");
    Scene {
        camera: camera
            .and_then(|camera| camera.attribute_by_name("prst"))
            .unwrap_or_default()
            .to_owned(),
        latitude: turn.map_or(0, |turn| whole(turn, "lat")),
        longitude: turn.map_or(0, |turn| whole(turn, "lon")),
        revolution: turn.map_or(0, |turn| whole(turn, "rev")),
        light: light
            .and_then(|light| light.attribute_by_name("rig"))
            .unwrap_or_default()
            .to_owned(),
        light_from: light
            .and_then(|light| light.attribute_by_name("dir"))
            .unwrap_or_default()
            .to_owned(),
    }
}

/// The shape's own solidity, written back out.
#[must_use]
pub fn depth_element(depth: &Depth) -> Option<Element> {
    if depth.is_flat() && depth.material.is_empty() {
        return None;
    }
    let mut solid = Element::new("a:sp3d", Some(crate::shapes::A));
    if depth.extrusion_emu > 0 {
        solid.set_attribute("extrusionH", &depth.extrusion_emu.to_string());
    }
    if depth.contour_emu > 0 {
        solid.set_attribute("contourW", &depth.contour_emu.to_string());
    }
    if !depth.material.is_empty() {
        solid.set_attribute("prstMaterial", &depth.material);
    }
    // The order is the schema's: the two bevels, then the two colours.
    for (name, bevel) in [("a:bevelT", &depth.bevel_top), ("a:bevelB", &depth.bevel_bottom)] {
        if let Some(bevel) = bevel {
            let mut element = Element::new(name, Some(crate::shapes::A));
            element.set_attribute("w", &bevel.width_emu.to_string());
            element.set_attribute("h", &bevel.height_emu.to_string());
            if !bevel.kind.is_empty() {
                element.set_attribute("prst", &bevel.kind);
            }
            solid.push_element(element);
        }
    }
    for (name, colour) in
        [("a:extrusionClr", &depth.extrusion_colour), ("a:contourClr", &depth.contour_colour)]
    {
        if let Some(colour) = colour {
            let mut element = Element::new(name, Some(crate::shapes::A));
            let mut value = Element::new("a:srgbClr", Some(crate::shapes::A));
            value.set_attribute("val", colour);
            element.push_element(value);
            solid.push_element(element);
        }
    }
    Some(solid)
}

/// And the scene it stands in.
#[must_use]
pub fn scene_element(scene: &Scene) -> Option<Element> {
    if scene.is_nothing() {
        return None;
    }
    let mut element = Element::new("a:scene3d", Some(crate::shapes::A));
    let mut camera = Element::new("a:camera", Some(crate::shapes::A));
    camera.set_attribute(
        "prst",
        if scene.camera.is_empty() { "orthographicFront" } else { &scene.camera },
    );
    if scene.latitude != 0 || scene.longitude != 0 || scene.revolution != 0 {
        let mut turn = Element::new("a:rot", Some(crate::shapes::A));
        turn.set_attribute("lat", &scene.latitude.to_string());
        turn.set_attribute("lon", &scene.longitude.to_string());
        turn.set_attribute("rev", &scene.revolution.to_string());
        camera.push_element(turn);
    }
    element.push_element(camera);

    let mut light = Element::new("a:lightRig", Some(crate::shapes::A));
    light.set_attribute("rig", if scene.light.is_empty() { "threePt" } else { &scene.light });
    light.set_attribute("dir", if scene.light_from.is_empty() { "t" } else { &scene.light_from });
    element.push_element(light);
    Some(element)
}

fn read_bevel(element: &Element) -> Bevel {
    Bevel {
        // What Word writes when it says nothing: a bevel of six points by
        // three, which is the one behind the first entry of its own gallery.
        width_emu: if element.attribute_by_name("w").is_some() {
            number(element, "w")
        } else {
            76_200
        },
        height_emu: if element.attribute_by_name("h").is_some() {
            number(element, "h")
        } else {
            38_100
        },
        kind: element.attribute_by_name("prst").unwrap_or("circle").to_owned(),
    }
}

fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

fn colour_of(parent: &Element) -> Option<String> {
    child(parent, "srgbClr")?.attribute_by_name("val").map(str::to_uppercase)
}

fn number(element: &Element, name: &str) -> i64 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

fn whole(element: &Element, name: &str) -> i32 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_shape_writes_nothing() {
        assert!(depth_element(&Depth::default()).is_none());
        assert!(scene_element(&Scene::default()).is_none());
    }

    #[test]
    fn a_bevel_that_says_nothing_about_its_size_is_the_one_word_draws() {
        // Six points by three, which is the first entry of Word's own gallery:
        // a bevel written as an empty element is a bevel, not nothing.
        let bevel = read_bevel(&Element::new("a:bevelT", Some(crate::shapes::A)));
        assert_eq!((bevel.width_emu, bevel.height_emu), (76_200, 38_100));
        assert_eq!(bevel.kind, "circle");
    }
    /// A shape's properties with whatever is given inside them.
    fn properties(inside: Vec<Element>) -> Element {
        let mut element = Element::new("wps:spPr", Some(crate::shapes::WPS));
        for child in inside {
            element.push_element(child);
        }
        element
    }

    #[test]
    fn what_makes_a_shape_solid_survives_being_written_and_read_back() {
        let depth = Depth {
            bevel_top: Some(Bevel {
                width_emu: 114_300,
                height_emu: 76_200,
                kind: "relaxedInset".to_owned(),
            }),
            bevel_bottom: None,
            extrusion_emu: 457_200,
            extrusion_colour: Some("2F528F".to_owned()),
            contour_emu: 12_700,
            contour_colour: Some("000000".to_owned()),
            material: "metal".to_owned(),
        };
        let written = depth_element(&depth).expect("a solid shape writes something");
        assert_eq!(read_depth(&properties(vec![written])), depth);
    }

    #[test]
    fn the_scene_a_shape_stands_in_survives_it_too() {
        let scene = Scene {
            camera: "orthographicFront".to_owned(),
            latitude: 900_000,
            longitude: 1_800_000,
            revolution: 0,
            light: "threePt".to_owned(),
            light_from: "t".to_owned(),
        };
        let written = scene_element(&scene).expect("a scene writes something");
        assert_eq!(read_scene(&properties(vec![written])), scene);
    }

    #[test]
    fn a_shape_that_says_nothing_is_flat() {
        let plain = properties(Vec::new());
        assert!(read_depth(&plain).is_flat());
        assert!(read_scene(&plain).is_nothing());
    }
}
