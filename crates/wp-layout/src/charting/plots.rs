//! The plots: how each kind of chart puts its numbers on the page.
//!
//! Every plot is given the frame left for it once the title, the key and
//! the table have taken their room, draws its axes inside that, and says
//! where its category slots ended up so that the table of numbers can line
//! its columns up with them.

use wp_docx::chart::{Grouping, Kind, LabelPosition, Series};
use wp_raster::{Color, Path, Point};

use super::{
    circle, over, polygon, thick_line, translucent, Canvas, Frame, Scale, Slots, AXIS_SHARE,
    BAR_SHARE, LABEL_SHARE, LINE,
};

/// The scale of the value axis for the series drawn as columns, bars, lines
/// or areas: what the tallest stack or the largest number reaches, or the
/// whole for a chart of shares.
fn value_scale(canvas: &Canvas<'_>, members: &[(usize, &Series)]) -> Scale {
    let chart = canvas.chart;
    if chart.grouping == Grouping::PercentStacked {
        return Scale::shares();
    }
    let (mut smallest, mut largest) = (0.0f64, 0.0f64);
    match chart.grouping {
        Grouping::Stacked => {
            // What the positive numbers of each category add up to, and the
            // negative ones.
            for index in 0..chart.points() {
                let (mut up, mut down) = (0.0, 0.0);
                for (_, series) in members {
                    let value = series.values.get(index).copied().unwrap_or(0.0);
                    if value >= 0.0 {
                        up += value;
                    } else {
                        down += value;
                    }
                }
                largest = largest.max(up);
                smallest = smallest.min(down);
            }
        }
        _ => {
            for (_, series) in members {
                for value in &series.values {
                    largest = largest.max(*value);
                    smallest = smallest.min(*value);
                }
            }
        }
    }
    Scale::spanning(&chart.value_axis, smallest, largest)
}

/// The scale of the second value axis, for the series that stand against
/// it.
fn secondary_scale(canvas: &Canvas<'_>) -> Option<Scale> {
    let chart = canvas.chart;
    let secondary: Vec<(usize, &Series)> =
        chart.series.iter().enumerate().filter(|(_, series)| series.secondary).collect();
    if secondary.is_empty() {
        return None;
    }
    let (mut smallest, mut largest) = (0.0f64, 0.0f64);
    for (_, series) in &secondary {
        for value in &series.values {
            largest = largest.max(*value);
            smallest = smallest.min(*value);
        }
    }
    Some(Scale::spanning(&Default::default(), smallest, largest))
}

/// The room the numbers up a value axis need: the widest of them, and a
/// little.
fn axis_words_room(canvas: &mut Canvas<'_>, scale: Scale) -> f32 {
    let code = canvas.chart.value_axis.number_format.clone();
    let size = canvas.size;
    scale
        .marks()
        .iter()
        .map(|mark| canvas.measure(&super::formatted(code.as_deref(), *mark), size))
        .fold(0.0, f32::max)
        + 6.0
}

/// The axes and the gridlines of a plot whose numbers go up: the value
/// axis up the left, the category axis along the bottom, and the second
/// value axis up the right when there is one.
fn upright_axes(canvas: &mut Canvas<'_>, plot: Frame, scale: Scale, secondary: Option<Scale>) {
    let line = canvas.palette.line;
    let faint = canvas.faint();
    let size = canvas.size;
    let code = canvas.chart.value_axis.number_format.clone();
    let deleted = canvas.chart.value_axis.deleted;

    canvas.rule(plot.left, plot.bottom, plot.width(), LINE, line);
    if !deleted {
        canvas.rule(plot.left, plot.top, LINE, plot.height(), line);
    }
    for mark in scale.marks() {
        let at = plot.bottom - plot.height() * scale.along(mark);
        // The nought line is the axis itself, drawn in full.
        if mark.abs() > scale.step * 0.001 {
            canvas.rule(plot.left, at, plot.width(), 1.0, faint);
        } else {
            canvas.rule(plot.left, at, plot.width(), LINE, line);
        }
        if !deleted {
            let text = super::formatted(code.as_deref(), mark);
            canvas.right_aligned(&text, plot.left - 4.0, at + size / 3.0, size);
        }
    }
    if let Some(second) = secondary {
        canvas.rule(plot.right, plot.top, LINE, plot.height(), line);
        for mark in second.marks() {
            let at = plot.bottom - plot.height() * second.along(mark);
            canvas.words(&super::number(mark), plot.right + 4.0, at + size / 3.0, size);
        }
    }
}

/// The plot area of an upright chart: what is left of the frame once the
/// words up the side and along the bottom have their room.
fn upright_plot(canvas: &mut Canvas<'_>, frame: Frame, scale: Scale, table_labels: f32) -> Frame {
    let words = if canvas.chart.value_axis.deleted { 0.0 } else { axis_words_room(canvas, scale) };
    // Room for the numbers up the side, or for the names down the table's
    // first column when that is wider — but never most of the chart.
    let left = frame.left + words.max(table_labels).min(frame.width() * AXIS_SHARE * 2.5);
    let right = if canvas.chart.has_secondary_axis() {
        frame.right - frame.width() * AXIS_SHARE
    } else {
        frame.right
    };
    // The names along the bottom have a band of their own — unless the
    // table of numbers is drawn, whose first row names the categories in
    // their place.
    let bottom = if canvas.chart.category_axis.deleted || canvas.chart.data_table.is_some() {
        frame.bottom
    } else {
        frame.bottom - frame.height() * LABEL_SHARE
    };
    Frame { left, top: frame.top, right, bottom }
}

/// The names of the categories under their slots.
fn category_names(canvas: &mut Canvas<'_>, plot: Frame, slot: f32, at_edges: bool) {
    if canvas.chart.category_axis.deleted || canvas.chart.data_table.is_some() {
        return;
    }
    let size = canvas.size;
    let names = canvas.chart.categories.clone();
    for (index, name) in names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let middle = if at_edges {
            plot.left + slot * index as f32
        } else {
            plot.left + slot * (index as f32 + 0.5)
        };
        canvas.centred(name, middle, plot.bottom + size + 4.0, size);
    }
}

/// Where a value stands in a stack: the bottom and the top of its piece,
/// given what was stacked before it.
fn stacked(value: f64, up: &mut f64, down: &mut f64) -> (f64, f64) {
    if value >= 0.0 {
        let from = *up;
        *up += value;
        (from, *up)
    } else {
        let from = *down;
        *down += value;
        (*down, from)
    }
}

/// What each category's numbers add up to in size, for a chart of shares
/// and for a label that says the share.
fn totals(members: &[(usize, &Series)], points: usize) -> Vec<f64> {
    (0..points)
        .map(|index| {
            members
                .iter()
                .map(|(_, series)| series.values.get(index).copied().unwrap_or(0.0).abs())
                .sum()
        })
        .collect()
}

/// Upright columns, with any series drawn as a line laid over them.
pub(super) fn columns(canvas: &mut Canvas<'_>, frame: Frame, table_labels: f32) -> Option<Slots> {
    let chart = canvas.chart;
    let members: Vec<(usize, &Series)> = chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, series)| chart.kind_of(series) == Kind::Column && !series.secondary)
        .collect();
    let scale = value_scale(canvas, &members);
    let second = secondary_scale(canvas);
    let plot = upright_plot(canvas, frame, scale, table_labels);
    upright_axes(canvas, plot, scale, second);

    let points = chart.points().max(1);
    let slot = plot.width() / points as f32;
    let y_of = |value: f64| plot.bottom - plot.height() * scale.along(value);
    let totals = totals(&members, points);

    // One slot per category, shared out between the series: two series put
    // two columns side by side in the slot, which is what Word calls
    // clustered and is how a chart of several series is read. Stacked, they
    // stand on one another in one column.
    let run = if chart.grouping == Grouping::Clustered { members.len().max(1) } else { 1 } as f32;
    let bar = slot * BAR_SHARE / run;
    for index in 0..points {
        let slot_left = plot.left + slot * index as f32;
        let (mut up, mut down) = (0.0, 0.0);
        for (place, (which, series)) in members.iter().enumerate() {
            let Some(value) = series.values.get(index).copied() else { continue };
            let total = totals.get(index).copied().unwrap_or(0.0);
            let share = if total > 0.0 { value.abs() / total } else { 0.0 };
            let (from, to) = match chart.grouping {
                Grouping::Clustered => (0.0f64.min(value), 0.0f64.max(value)),
                Grouping::Stacked => stacked(value, &mut up, &mut down),
                Grouping::PercentStacked => {
                    stacked(if total > 0.0 { value / total } else { 0.0 }, &mut up, &mut down)
                }
            };
            let x = if chart.grouping == Grouping::Clustered {
                slot_left + (slot - bar * run) / 2.0 + bar * place as f32
            } else {
                slot_left + (slot - bar) / 2.0
            };
            let (top, bottom) = (y_of(to), y_of(from));
            let colour = canvas.point_colour(*which, series, index);
            canvas.rule(x, top, bar, bottom - top, colour);

            // The number on the column: past its end, or inside it where the
            // chart asks, or in its middle when it is stacked.
            let stacked_up = chart.grouping != Grouping::Clustered;
            let position = canvas.chart.labels_of(series).position.unwrap_or(if stacked_up {
                LabelPosition::Center
            } else {
                LabelPosition::OutsideEnd
            });
            let size = canvas.size;
            let baseline = match position {
                LabelPosition::Center => (top + bottom) / 2.0 + size * 0.3,
                LabelPosition::InsideEnd if value >= 0.0 => top + size,
                LabelPosition::InsideEnd => bottom - size * 0.3,
                LabelPosition::InsideBase if value >= 0.0 => bottom - size * 0.3,
                LabelPosition::InsideBase => top + size,
                _ if value >= 0.0 => top - size * 0.4,
                _ => bottom + size,
            };
            canvas.label(series, index, value, share, (x + bar / 2.0, baseline));
        }
    }
    category_names(canvas, plot, slot, false);

    // The line laid over the columns, for a combination chart.
    lines_over(canvas, plot, slot, scale, second, false);

    Some(Slots {
        left: plot.left,
        edges: (0..=points).map(|index| plot.left + slot * index as f32).collect(),
    })
}

/// The series drawn as lines over another kind's plot, and the series that
/// stand against the second axis.
fn lines_over(
    canvas: &mut Canvas<'_>,
    plot: Frame,
    slot: f32,
    scale: Scale,
    second: Option<Scale>,
    all: bool,
) {
    let chart = canvas.chart;
    let overlaid: Vec<(usize, &Series)> = chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, series)| {
            let kind = chart.kind_of(series);
            all || (kind != chart.kind && matches!(kind, Kind::Line | Kind::Scatter))
                || series.secondary
        })
        .collect();
    for (which, series) in overlaid {
        let scale = if series.secondary { second.unwrap_or(scale) } else { scale };
        let colour = canvas.series_colour(which, series);
        let mut previous: Option<(f32, f32)> = None;
        for (index, value) in series.values.iter().enumerate() {
            let x = plot.left + slot * (index as f32 + 0.5);
            let y = plot.bottom - plot.height() * scale.along(*value);
            if let Some((last_x, last_y)) = previous {
                canvas.path(thick_line(last_x, last_y, x, y, LINE), colour);
            }
            canvas.rule(x - LINE, y - LINE, LINE * 2.0, LINE * 2.0, colour);
            previous = Some((x, y));
            let size = canvas.size;
            canvas.label(series, index, *value, 0.0, (x, y - size * 0.6));
        }
    }
}

/// Bars lying on their side.
pub(super) fn bars(canvas: &mut Canvas<'_>, frame: Frame) -> Option<Slots> {
    let chart = canvas.chart;
    let members: Vec<(usize, &Series)> = chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, series)| chart.kind_of(series) == Kind::Bar && !series.secondary)
        .collect();
    let scale = value_scale(canvas, &members);
    let size = canvas.size;
    let line = canvas.palette.line;
    let faint = canvas.faint();

    // The names of the categories stand to the left, so the plot starts
    // past the widest of them.
    let names = if chart.category_axis.deleted {
        0.0
    } else {
        chart.categories.iter().map(|name| canvas.measure(name, size)).fold(0.0, f32::max) + 8.0
    };
    let plot = Frame {
        left: frame.left + names.min(frame.width() * AXIS_SHARE * 2.0),
        top: frame.top,
        right: frame.right,
        bottom: if chart.value_axis.deleted {
            frame.bottom
        } else {
            frame.bottom - frame.height() * LABEL_SHARE
        },
    };
    let x_of = |value: f64| plot.left + plot.width() * scale.along(value);

    // The axes: the numbers along the bottom this time.
    canvas.rule(plot.left, plot.top, LINE, plot.height(), line);
    let code = chart.value_axis.number_format.clone();
    for mark in scale.marks() {
        let at = x_of(mark);
        if mark.abs() > scale.step * 0.001 {
            canvas.rule(at, plot.top, 1.0, plot.height(), faint);
        } else {
            canvas.rule(at, plot.top, LINE, plot.height(), line);
        }
        if !chart.value_axis.deleted {
            let text = super::formatted(code.as_deref(), mark);
            canvas.centred(&text, at, plot.bottom + size + 2.0, size);
        }
    }

    let points = chart.points().max(1);
    let slot = plot.height() / points as f32;
    let run = if chart.grouping == Grouping::Clustered { members.len().max(1) } else { 1 } as f32;
    let thick = slot * BAR_SHARE / run;
    let totals = totals(&members, points);
    for index in 0..points {
        let slot_top = plot.top + slot * index as f32;
        let (mut up, mut down) = (0.0, 0.0);
        for (place, (which, series)) in members.iter().enumerate() {
            let Some(value) = series.values.get(index).copied() else { continue };
            let total = totals.get(index).copied().unwrap_or(0.0);
            let share = if total > 0.0 { value.abs() / total } else { 0.0 };
            let (from, to) = match chart.grouping {
                Grouping::Clustered => (0.0f64.min(value), 0.0f64.max(value)),
                Grouping::Stacked => stacked(value, &mut up, &mut down),
                Grouping::PercentStacked => {
                    stacked(if total > 0.0 { value / total } else { 0.0 }, &mut up, &mut down)
                }
            };
            let y = if chart.grouping == Grouping::Clustered {
                slot_top + (slot - thick * run) / 2.0 + thick * place as f32
            } else {
                slot_top + (slot - thick) / 2.0
            };
            let (left, right) = (x_of(from), x_of(to));
            let colour = canvas.point_colour(*which, series, index);
            canvas.rule(left, y, right - left, thick, colour);

            let position = canvas.chart.labels_of(series).position.unwrap_or(
                if chart.grouping == Grouping::Clustered {
                    LabelPosition::OutsideEnd
                } else {
                    LabelPosition::Center
                },
            );
            let text = canvas.label_text(series, index, value, share);
            if !text.is_empty() && canvas.chart.labels_of(series).shows_anything() {
                let width = canvas.measure(&text, size * 0.85);
                let x = match position {
                    LabelPosition::Center => (left + right) / 2.0 - width / 2.0,
                    LabelPosition::InsideEnd if value >= 0.0 => right - width - 3.0,
                    LabelPosition::InsideEnd => left + 3.0,
                    LabelPosition::InsideBase if value >= 0.0 => left + 3.0,
                    LabelPosition::InsideBase => right - width - 3.0,
                    _ if value >= 0.0 => right + 4.0,
                    _ => left - width - 4.0,
                };
                canvas.words(&text, x, y + thick / 2.0 + size / 3.0, size * 0.85);
            }
        }

        if let Some(name) = chart.categories.get(index).filter(|name| !name.is_empty()) {
            if !chart.category_axis.deleted {
                canvas.right_aligned(
                    name,
                    plot.left - 4.0,
                    slot_top + slot / 2.0 + size / 3.0,
                    size,
                );
            }
        }
    }
    None
}

/// A line through the points of each series, or the room under it filled
/// in.
pub(super) fn lines(canvas: &mut Canvas<'_>, frame: Frame, table_labels: f32) -> Option<Slots> {
    let chart = canvas.chart;
    let area = chart.kind == Kind::Area;
    let members: Vec<(usize, &Series)> = chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, series)| chart.kind_of(series) == chart.kind && !series.secondary)
        .collect();
    let scale = value_scale(canvas, &members);
    let second = secondary_scale(canvas);
    let plot = upright_plot(canvas, frame, scale, table_labels);
    upright_axes(canvas, plot, scale, second);

    let points = chart.points().max(1);
    // An area's points stand on the marks and a line's between them, which
    // is where each kind puts them by default.
    let slot =
        if area { plot.width() / (points.max(2) - 1) as f32 } else { plot.width() / points as f32 };
    let x_of = |index: usize| {
        if area {
            plot.left + slot * index as f32
        } else {
            plot.left + slot * (index as f32 + 0.5)
        }
    };
    let y_of = |value: f64| plot.bottom - plot.height() * scale.along(value);
    let totals = totals(&members, points);

    // Where each category's stack has got to, for the series stacked on
    // one another.
    let mut stacked_up = vec![0.0f64; points];
    let mut stacked_down = vec![0.0f64; points];
    for (which, series) in &members {
        let colour = canvas.series_colour(*which, series);
        let mut tops: Vec<(f32, f32, f64, f64)> = Vec::new();
        for index in 0..points {
            let Some(value) = series.values.get(index).copied() else { continue };
            let total = totals.get(index).copied().unwrap_or(0.0);
            let share = if total > 0.0 { value.abs() / total } else { 0.0 };
            let (from, to) = match chart.grouping {
                Grouping::Clustered => (0.0, value),
                Grouping::Stacked => {
                    stacked(value, &mut stacked_up[index], &mut stacked_down[index])
                }
                Grouping::PercentStacked => stacked(
                    if total > 0.0 { value / total } else { 0.0 },
                    &mut stacked_up[index],
                    &mut stacked_down[index],
                ),
            };
            let shown = if chart.grouping == Grouping::Clustered { value } else { to };
            let base = if chart.grouping == Grouping::Clustered { 0.0 } else { from };
            tops.push((x_of(index), y_of(shown), share, base));
        }

        if area && tops.len() >= 2 {
            // The shape under the line, down to what is under it: the nought
            // line, or the series stacked below.
            let mut corners: Vec<(f32, f32)> = tops.iter().map(|(x, y, _, _)| (*x, *y)).collect();
            for (x, _, _, base) in tops.iter().rev() {
                corners.push((*x, y_of(*base)));
            }
            canvas.path(polygon(&corners), colour);
        } else {
            let mut previous: Option<(f32, f32)> = None;
            for (x, y, _, _) in &tops {
                if let Some((last_x, last_y)) = previous {
                    canvas.path(thick_line(last_x, last_y, *x, *y, LINE), colour);
                }
                canvas.rule(x - LINE, y - LINE, LINE * 2.0, LINE * 2.0, colour);
                previous = Some((*x, *y));
            }
        }
        for (index, (x, y, share, _)) in tops.iter().enumerate() {
            let value = series.values.get(index).copied().unwrap_or(0.0);
            let size = canvas.size;
            canvas.label(series, index, value, *share, (*x, y - size * 0.6));
        }
    }
    category_names(canvas, plot, slot, area);
    lines_over(
        canvas,
        plot,
        if area { plot.width() / points as f32 } else { slot },
        scale,
        second,
        false,
    );

    let edges = if area {
        // The table's columns are the slots between the marks, half a slot
        // either side of each point.
        (0..=points)
            .map(|index| plot.left + (plot.width() / points as f32) * index as f32)
            .collect()
    } else {
        (0..=points).map(|index| plot.left + slot * index as f32).collect()
    };
    Some(Slots { left: plot.left, edges })
}

/// Points at an x and a y, and bubbles as large as their third number.
pub(super) fn points(canvas: &mut Canvas<'_>, frame: Frame) -> Option<Slots> {
    let chart = canvas.chart;
    let size = canvas.size;
    let faint = canvas.faint();

    let (mut x_low, mut x_high, mut y_low, mut y_high) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut largest_size = 0.0f64;
    for series in &chart.series {
        for (index, y) in series.values.iter().enumerate() {
            let x = series.xs.get(index).copied().unwrap_or(index as f64 + 1.0);
            x_low = x_low.min(x);
            x_high = x_high.max(x);
            y_low = y_low.min(*y);
            y_high = y_high.max(*y);
            largest_size = largest_size.max(series.sizes.get(index).copied().unwrap_or(1.0).abs());
        }
    }
    let x_scale = Scale::spanning(&chart.category_axis, x_low, x_high);
    let y_scale = Scale::spanning(&chart.value_axis, y_low, y_high);
    let words = axis_words_room(canvas, y_scale);
    // A bubble's size is its area, so its radius goes as the root of the
    // number; the largest fills a tenth of the plot across, and the plot
    // keeps that much clear at its edges so a bubble on the axis is whole.
    let widest =
        if chart.kind == Kind::Bubble { frame.width().min(frame.height()) * 0.1 } else { 0.0 };
    let plot = Frame {
        left: frame.left + words.min(frame.width() * AXIS_SHARE) + widest,
        top: frame.top + widest,
        right: frame.right - size - widest,
        bottom: frame.bottom - frame.height() * LABEL_SHARE - widest,
    };
    upright_axes(canvas, plot, y_scale, None);
    let x_code = chart.category_axis.number_format.clone();
    for mark in x_scale.marks() {
        let at = plot.left + plot.width() * x_scale.along(mark);
        if mark.abs() > x_scale.step * 0.001 {
            canvas.rule(at, plot.top, 1.0, plot.height(), faint);
        }
        if !chart.category_axis.deleted {
            let text = super::formatted(x_code.as_deref(), mark);
            canvas.centred(&text, at, plot.bottom + size + 2.0, size);
        }
    }

    for (which, series) in chart.series.iter().enumerate() {
        for (index, y) in series.values.iter().enumerate() {
            let x = series.xs.get(index).copied().unwrap_or(index as f64 + 1.0);
            let px = plot.left + plot.width() * x_scale.along(x);
            let py = plot.bottom - plot.height() * y_scale.along(*y);
            let colour = canvas.point_colour(which, series, index);
            if chart.kind == Kind::Bubble {
                let bubble = series.sizes.get(index).copied().unwrap_or(1.0).abs();
                let radius = if largest_size > 0.0 {
                    widest * (bubble / largest_size).sqrt() as f32
                } else {
                    widest * 0.5
                };
                canvas.path(circle(px, py, radius.max(1.0)), translucent(colour, 200));
                canvas.label(series, index, *y, 0.0, (px, py + size * 0.3));
            } else {
                let dot = LINE * 1.25;
                canvas.rule(px - dot, py - dot, dot * 2.0, dot * 2.0, colour);
                canvas.label(series, index, *y, 0.0, (px, py - size * 0.6));
            }
        }
    }
    None
}

/// One axis per category, spread round a circle, and each series as a
/// ring through its numbers.
pub(super) fn radar(canvas: &mut Canvas<'_>, frame: Frame) -> Option<Slots> {
    let chart = canvas.chart;
    let size = canvas.size;
    let faint = canvas.faint();
    let points = chart.points();
    if points == 0 {
        return None;
    }
    let scale = Scale::spanning(&chart.value_axis, chart.smallest(), chart.largest());

    // Room round the outside for the names of the categories.
    let names = chart.categories.iter().map(|name| canvas.measure(name, size)).fold(0.0, f32::max);
    let radius = ((frame.width() - 2.0 * names - size) / 2.0)
        .min((frame.height() - 3.0 * size) / 2.0)
        .max(size);
    let centre_x = (frame.left + frame.right) / 2.0;
    let centre_y = (frame.top + frame.bottom) / 2.0;
    let angle_of = |index: usize| {
        -core::f32::consts::FRAC_PI_2 + core::f32::consts::TAU * index as f32 / points as f32
    };
    let at = |index: usize, share: f32| {
        let angle = angle_of(index);
        (centre_x + radius * share * angle.cos(), centre_y + radius * share * angle.sin())
    };

    // The rings of the scale and the spokes of the categories.
    for mark in scale.marks() {
        let share = scale.along(mark);
        if share <= 0.0 {
            continue;
        }
        let corners: Vec<(f32, f32)> = (0..points).map(|index| at(index, share)).collect();
        let mut ring = Path::new();
        for (index, (x, y)) in corners.iter().enumerate() {
            if index == 0 {
                ring.move_to(Point::new(*x, *y));
            } else {
                ring.line_to(Point::new(*x, *y));
            }
        }
        ring.close();
        for pair in
            corners.windows(2).chain(std::iter::once(&[corners[corners.len() - 1], corners[0]][..]))
        {
            canvas.path(thick_line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, 1.0), faint);
        }
        // The numbers of the scale stand beside the spoke that points up,
        // to its left, clear of the name at its end.
        if !chart.value_axis.deleted {
            let code = chart.value_axis.number_format.clone();
            let text = super::formatted(code.as_deref(), mark);
            canvas.right_aligned(
                &text,
                centre_x - 3.0,
                centre_y - radius * share + size * 0.3,
                size * 0.8,
            );
        }
    }
    for index in 0..points {
        let (x, y) = at(index, 1.0);
        canvas.path(thick_line(centre_x, centre_y, x, y, 1.0), faint);
        if let Some(name) = chart.categories.get(index).filter(|name| !name.is_empty()) {
            let (x, y) = at(index, 1.0);
            let angle = angle_of(index);
            let width = canvas.measure(name, size);
            // Outside the ring, on the side the spoke points to.
            let label_x = x + angle.cos() * size * 0.6 - width * (1.0 - angle.cos()) / 2.0;
            let label_y = y + angle.sin() * size * 0.6 + size * 0.35;
            canvas.words(name, label_x, label_y, size);
        }
    }

    // Each series as a ring through its numbers.
    for (which, series) in chart.series.iter().enumerate() {
        let colour = canvas.series_colour(which, series);
        let corners: Vec<(usize, (f32, f32), f64)> = series
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| (index, at(index, scale.along(*value)), *value))
            .collect();
        for pair in corners.windows(2) {
            canvas.path(
                thick_line(pair[0].1 .0, pair[0].1 .1, pair[1].1 .0, pair[1].1 .1, LINE),
                colour,
            );
        }
        if corners.len() == points && points > 2 {
            let (first, last) = (corners[0].1, corners[corners.len() - 1].1);
            canvas.path(thick_line(last.0, last.1, first.0, first.1, LINE), colour);
        }
        for (index, (x, y), value) in corners {
            canvas.rule(x - LINE, y - LINE, LINE * 2.0, LINE * 2.0, colour);
            canvas.label(series, index, value, 0.0, (x, y - size * 0.6));
        }
    }
    None
}

/// The bands of height a surface is drawn in: what each is called and its
/// colour, lowest first.
pub(super) fn bands(canvas: &Canvas<'_>) -> Vec<(String, Color)> {
    let chart = canvas.chart;
    let scale = Scale::spanning(&chart.value_axis, chart.smallest(), chart.largest());
    let marks = scale.marks();
    let code = chart.value_axis.number_format.clone();
    marks
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let name = format!(
                "{}–{}",
                super::formatted(code.as_deref(), pair[0]),
                super::formatted(code.as_deref(), pair[1])
            );
            (name, super::accent(canvas.palette, index))
        })
        .collect()
}

/// The numbers as heights over a grid of categories and series, seen from
/// above: each cell in the colour of the band its number falls in.
pub(super) fn surface(canvas: &mut Canvas<'_>, frame: Frame, table_labels: f32) -> Option<Slots> {
    let chart = canvas.chart;
    let size = canvas.size;
    let line = canvas.palette.line;
    let points = chart.points().max(1);
    let rows = chart.series.len().max(1);
    let scale = Scale::spanning(&chart.value_axis, chart.smallest(), chart.largest());
    let marks = scale.marks();

    // The series' names stand to the left, the categories' along the bottom.
    let names =
        chart.series.iter().map(|series| canvas.measure(&series.name, size)).fold(0.0, f32::max)
            + 8.0;
    let plot = Frame {
        left: frame.left + names.max(table_labels).min(frame.width() * AXIS_SHARE * 2.0),
        top: frame.top,
        right: frame.right,
        bottom: frame.bottom - frame.height() * LABEL_SHARE,
    };
    let slot = plot.width() / points as f32;
    let row = plot.height() / rows as f32;
    for (which, series) in chart.series.iter().enumerate() {
        // The first series along the bottom, as the front of a surface is.
        let top = plot.bottom - row * (which + 1) as f32;
        for index in 0..points {
            let Some(value) = series.values.get(index) else { continue };
            let band = marks
                .windows(2)
                .position(|pair| *value >= pair[0] && *value < pair[1])
                .unwrap_or(marks.len().saturating_sub(2));
            let colour = super::accent(canvas.palette, band);
            canvas.rule(plot.left + slot * index as f32, top, slot, row, colour);
            let baseline = top + row / 2.0 + size * 0.3;
            let text_colour = over(colour, canvas.palette.text);
            if canvas.chart.labels_of(series).shows_anything() {
                let text = canvas.label_text(series, index, *value, 0.0);
                let (glyphs, width) =
                    canvas.shaper.shape_label(&text, 0.0, baseline, size * 0.85, text_colour);
                let shift = plot.left + slot * (index as f32 + 0.5) - width / 2.0;
                canvas.out.glyphs.extend(
                    glyphs.into_iter().map(|glyph| crate::layout::PositionedGlyph {
                        x: glyph.x + shift,
                        ..glyph
                    }),
                );
            }
        }
        canvas.right_aligned(&series.name, plot.left - 4.0, top + row / 2.0 + size / 3.0, size);
    }
    canvas.rule(plot.left, plot.bottom, plot.width(), LINE, line);
    canvas.rule(plot.left, plot.top, LINE, plot.height(), line);
    category_names(canvas, plot, slot, false);
    Some(Slots {
        left: plot.left,
        edges: (0..=points).map(|index| plot.left + slot * index as f32).collect(),
    })
}

/// A pie, divided by how much of the total each value is — or a doughnut,
/// with a ring per series round a hole.
pub(super) fn round(canvas: &mut Canvas<'_>, frame: Frame) -> Option<Slots> {
    let chart = canvas.chart;
    let size = canvas.size;
    let doughnut = chart.kind == Kind::Doughnut;
    let rings: Vec<(usize, &Series)> = if doughnut {
        chart.series.iter().enumerate().collect()
    } else {
        chart.series.iter().enumerate().take(1).collect()
    };
    if rings.iter().all(|(_, series)| series.values.iter().sum::<f64>() <= 0.0) {
        return None;
    }

    // Room outside the circle for the words moved off their slices: a
    // little, because most words fit inside and a pie drawn small to make
    // room for the few that do not is a pie nobody can read.
    let outside = chart.series.iter().any(|series| {
        let labels = chart.labels_of(series);
        labels.shows_anything()
            && !matches!(labels.position, Some(LabelPosition::Center | LabelPosition::InsideEnd))
    });
    let room = if outside { size * 1.2 } else { 0.0 };
    let radius = ((frame.width().min(frame.height()) - 2.0 * room) / 2.0).max(size);
    let centre_x = (frame.left + frame.right) / 2.0;
    let centre_y = (frame.top + frame.bottom) / 2.0;
    let hole = if doughnut { radius * f32::from(chart.hole.clamp(10, 90)) / 100.0 } else { 0.0 };
    let thickness = (radius - hole) / rings.len().max(1) as f32;

    for (ring, (which, series)) in rings.iter().enumerate() {
        let total: f64 = series.values.iter().sum();
        if total <= 0.0 {
            continue;
        }
        // The first series is the innermost ring, which is how Word stacks
        // them.
        let inner = hole + thickness * ring as f32;
        let outer = inner + thickness;
        // From the top, clockwise, which is where a pie starts.
        let mut angle = -core::f32::consts::FRAC_PI_2;
        for (index, value) in series.values.iter().enumerate() {
            let share = (*value / total).max(0.0);
            let sweep = share as f32 * core::f32::consts::TAU;
            let colour = canvas.point_colour(*which, series, index);
            canvas.path(slice(centre_x, centre_y, inner, outer, angle, sweep), colour);
            let middle = angle + sweep / 2.0;
            angle += sweep;

            let labels = chart.labels_of(series);
            if !labels.shows_anything() || sweep <= 0.0 {
                continue;
            }
            let text = canvas.label_text(series, index, *value, share);
            if text.is_empty() {
                continue;
            }
            let width = canvas.measure(&text, size * 0.85);
            // Inside the slice, halfway out along the middle of it, when it
            // fits across the slice there — or where the chart asks — and
            // otherwise moved out past the edge with a line back to it.
            let mid_radius = (inner + outer) / 2.0;
            let across = 2.0 * mid_radius * (sweep / 2.0).sin().abs();
            let fits = doughnut || sweep > core::f32::consts::PI || width <= across;
            let inside = match labels.position {
                Some(
                    LabelPosition::Center | LabelPosition::InsideEnd | LabelPosition::InsideBase,
                ) => true,
                Some(LabelPosition::OutsideEnd) => false,
                _ => fits,
            };
            if inside {
                let x = centre_x + middle.cos() * mid_radius;
                let y = centre_y + middle.sin() * mid_radius;
                let text_colour = over(colour, canvas.palette.text);
                let (glyphs, width) =
                    canvas.shaper.shape_label(&text, 0.0, y + size * 0.3, size * 0.85, text_colour);
                let shift = x - width / 2.0;
                canvas.out.glyphs.extend(
                    glyphs.into_iter().map(|glyph| crate::layout::PositionedGlyph {
                        x: glyph.x + shift,
                        ..glyph
                    }),
                );
            } else {
                let edge_x = centre_x + middle.cos() * outer;
                let edge_y = centre_y + middle.sin() * outer;
                let out_x = centre_x + middle.cos() * (outer + size * 0.9);
                let out_y = centre_y + middle.sin() * (outer + size * 0.9);
                if labels.leader_lines {
                    canvas.path(thick_line(edge_x, edge_y, out_x, out_y, 1.0), canvas.palette.line);
                }
                // To the right of the line on the right-hand side of the pie
                // and to the left of it on the left.
                let x = if middle.cos() >= 0.0 { out_x + 2.0 } else { out_x - width - 2.0 };
                canvas.words(&text, x, out_y + size * 0.3, size * 0.85);
            }
        }
    }
    None
}

/// One slice of a pie or a ring of a doughnut, as a shape: from one radius
/// out to another, through a sweep of angle.
fn slice(centre_x: f32, centre_y: f32, inner: f32, outer: f32, from: f32, sweep: f32) -> Path {
    let mut path = Path::new();
    // Enough straight edges that the curve reads as a curve: one every few
    // degrees, which is finer than a page can show.
    let steps = ((sweep.abs() / 0.08).ceil() as usize).clamp(2, 240);
    let point = |radius: f32, step: usize| {
        let angle = from + sweep * step as f32 / steps as f32;
        Point::new(centre_x + radius * angle.cos(), centre_y + radius * angle.sin())
    };
    if inner <= 0.0 {
        path.move_to(Point::new(centre_x, centre_y));
        for step in 0..=steps {
            path.line_to(point(outer, step));
        }
    } else {
        path.move_to(point(inner, 0));
        for step in 0..=steps {
            path.line_to(point(outer, step));
        }
        for step in (0..=steps).rev() {
            path.line_to(point(inner, step));
        }
    }
    path.close();
    path
}
