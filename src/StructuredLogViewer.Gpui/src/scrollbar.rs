//! Overlay scrollbars.
//!
//! gpui scrolls a `div` or a `uniform_list` perfectly well from a wheel or a
//! trackpad, but it draws nothing while doing it — there is no scrollbar
//! element in the framework, and a mouse without a wheel (or anyone who
//! wants to know how far down a file they are) has nothing to go on.
//!
//! This is one `canvas` overlaid on the scrolling content. It reads the
//! geometry straight off the [`ScrollHandle`] each frame, paints a thumb per
//! overflowing axis, and registers window-level mouse handlers so a drag
//! keeps working after the pointer leaves the thumb. Drag state lives in an
//! `Rc<RefCell<_>>` the view owns, because paint closures cannot borrow the
//! view.

use crate::theme::Theme;
use gpui::{
    Bounds, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    ScrollHandle, Window, canvas, div, point, prelude::*, px, quad, size,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Track width. Thin enough to overlay content without reserving a gutter.
const THICKNESS: f32 = 10.;
const THUMB: f32 = 7.;
const MIN_THUMB: f32 = 24.;
/// Keeps the two thumbs from meeting in the corner.
const CORNER: f32 = THICKNESS;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy)]
struct Drag {
    axis: Axis,
    /// Where the pointer went down, and what the offset was then.
    from: Pixels,
    offset: Pixels,
}

/// The bit of scrollbar state that has to survive between frames. One per
/// scrollable region; the view owns it and hands out clones.
#[derive(Clone, Default)]
pub struct Scrollbars(Rc<RefCell<Option<Drag>>>);

impl Scrollbars {
    pub fn new() -> Scrollbars {
        Scrollbars::default()
    }

    /// An element to overlay on the scrolling content, sized to it.
    pub fn render(&self, handle: &ScrollHandle, theme: &Theme) -> gpui::AnyElement {
        let drag = self.0.clone();
        let handle = handle.clone();
        let theme = *theme;
        div()
            .absolute()
            .inset_0()
            // No id and no interactivity: the bars are painted over the
            // content, they must not eat its clicks or wheel events. The
            // drag is handled with window-level listeners instead.
            .child(
                canvas(|_, _, _| (), move |bounds, _, window, _| paint(bounds, &handle, &drag, &theme, window))
                    .size_full(),
            )
            .into_any_element()
    }
}

/// Thumb extent along `axis`, as (start, length) inside a track of
/// `track_length`, or None when the content fits.
fn thumb(handle: &ScrollHandle, axis: Axis, track_length: f32) -> Option<(f32, f32)> {
    let (offset, max) = match axis {
        Axis::Vertical => (handle.offset().y, handle.max_offset().y),
        Axis::Horizontal => (handle.offset().x, handle.max_offset().x),
    };
    let max: f32 = max.into();
    if max <= 1. || track_length <= MIN_THUMB {
        return None;
    }
    // gpui scrolls by translating content, so the offset runs 0 → -max.
    let scrolled = (-f32::from(offset)).clamp(0., max);
    let content = track_length + max;
    let length = (track_length * track_length / content).max(MIN_THUMB).min(track_length);
    let start = (scrolled / max) * (track_length - length);
    Some((start, length))
}

fn paint(bounds: Bounds<Pixels>, handle: &ScrollHandle, drag: &Rc<RefCell<Option<Drag>>>, theme: &Theme, window: &mut Window) {
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    let hovered = bounds.contains(&window.mouse_position());
    let dragging = drag.borrow().is_some();

    // Both tracks stop short of the corner so the thumbs never overlap.
    let v_track = height - if thumb(handle, Axis::Horizontal, width).is_some() { CORNER } else { 0. };
    let h_track = width - if thumb(handle, Axis::Vertical, height).is_some() { CORNER } else { 0. };

    let vertical = thumb(handle, Axis::Vertical, v_track);
    let horizontal = thumb(handle, Axis::Horizontal, h_track);
    if vertical.is_none() && horizontal.is_none() {
        return;
    }

    // Quiet until the pointer is over the region, like an overlay scroller.
    let alpha = if dragging {
        0.9
    } else if hovered {
        0.65
    } else {
        0.38
    };
    let color = theme.text_secondary.opacity(alpha);
    let inset = (THICKNESS - THUMB) / 2.;

    let v_bounds = vertical.map(|(start, length)| {
        Bounds::new(
            point(bounds.origin.x + px(width - THICKNESS + inset), bounds.origin.y + px(start)),
            size(px(THUMB), px(length)),
        )
    });
    let h_bounds = horizontal.map(|(start, length)| {
        Bounds::new(
            point(bounds.origin.x + px(start), bounds.origin.y + px(height - THICKNESS + inset)),
            size(px(length), px(THUMB)),
        )
    });

    for rect in [v_bounds, h_bounds].into_iter().flatten() {
        window.paint_quad(quad(
            rect,
            px(THUMB / 2.),
            color,
            gpui::Edges::all(px(0.)),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
    }

    // ----- dragging -----
    //
    // Registered on the window rather than the element so the drag survives
    // the pointer leaving the thumb, which is most of a scrollbar's job.
    {
        let drag = drag.clone();
        let handle = handle.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _window, _cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let inside = |rect: Option<Bounds<Pixels>>| rect.is_some_and(|r| r.contains(&event.position));
            let started = if inside(v_bounds) {
                Some(Drag { axis: Axis::Vertical, from: event.position.y, offset: handle.offset().y })
            } else if inside(h_bounds) {
                Some(Drag { axis: Axis::Horizontal, from: event.position.x, offset: handle.offset().x })
            } else {
                None
            };
            if started.is_some() {
                *drag.borrow_mut() = started;
            }
        });
    }
    {
        let drag = drag.clone();
        let handle = handle.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let Some(state) = *drag.borrow() else { return };
            let (track, extent, max, position) = match state.axis {
                Axis::Vertical => (v_track, vertical.map_or(0., |(_, l)| l), handle.max_offset().y, event.position.y),
                Axis::Horizontal => (h_track, horizontal.map_or(0., |(_, l)| l), handle.max_offset().x, event.position.x),
            };
            let travel = track - extent;
            if travel <= 0. {
                return;
            }
            // Thumb travel maps onto scroll range.
            let delta: f32 = (position - state.from).into();
            let scrolled = (state.offset + px(-delta / travel * f32::from(max))).clamp(-max, px(0.));
            let mut offset = handle.offset();
            match state.axis {
                Axis::Vertical => offset.y = scrolled,
                Axis::Horizontal => offset.x = scrolled,
            }
            handle.set_offset(offset);
            window.refresh();
        });
    }
    {
        let drag = drag.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _cx| {
            if phase == DispatchPhase::Bubble && drag.borrow_mut().take().is_some() {
                window.refresh();
            }
        });
    }
}
