//! The Tracing view: one lane per MSBuild worker node, blocks nested
//! flame-chart style, painted straight into the window with `canvas`.
//! Nesting is recomputed over the *visible* blocks so kind filters
//! re-compact the chart. Scroll pans, ⌥-scroll zooms about the pointer,
//! hovering shows a tooltip, clicking a block reveals it in the tree.

use crate::engine::Session;
use crate::model::{Timeline, TimelineBlock, format_duration};
use crate::theme::Theme;
use gpui::{
    BorderStyle, Bounds, ClickEvent, ContentMask, Context, CursorStyle, EventEmitter, Font, Hsla,
    MouseMoveEvent, Pixels, Point, ScrollWheelEvent, TextRun, Window, anchored, canvas, deferred,
    div, point, prelude::*, px, quad, rgb, size,
};
use std::sync::Arc;

pub enum TimelineEvent {
    Reveal(String),
}

const LANE_HEADER: f32 = 22.;
const BLOCK_ROW: f32 = 18.;
const LANE_GAP: f32 = 10.;
const LEFT_PAD: f32 = 8.;
const RULER: f32 = 22.;
const MIN_VISIBLE: f32 = 0.5;

#[derive(Clone, Copy, PartialEq)]
pub struct Filter {
    pub evaluations: bool,
    pub projects: bool,
    pub targets: bool,
    pub tasks: bool,
    pub other: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { evaluations: true, projects: true, targets: true, tasks: true, other: true }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Category {
    Evaluation,
    Project,
    Target,
    Task,
    Other,
}

fn category(kind: &str) -> Category {
    match kind {
        "ProjectEvaluation" => Category::Evaluation,
        "Project" => Category::Project,
        "Target" => Category::Target,
        "Task" => Category::Task,
        _ => Category::Other,
    }
}

impl Filter {
    fn allows(&self, kind: &str) -> bool {
        match category(kind) {
            Category::Evaluation => self.evaluations,
            Category::Project => self.projects,
            Category::Target => self.targets,
            Category::Task => self.tasks,
            Category::Other => self.other,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Counts {
    evaluations: usize,
    projects: usize,
    targets: usize,
    tasks: usize,
    other: usize,
}

struct Placed {
    block: TimelineBlock,
    indent: usize,
}

struct LaneLayout {
    lane_id: i64,
    y: f32,
    height: f32,
    rows: Vec<Placed>,
}

struct Layout {
    lanes: Vec<LaneLayout>,
    total_height: f32,
    duration_ms: f64,
}

fn rank(kind: &str) -> u8 {
    match kind {
        "ProjectEvaluation" | "Project" => 0,
        "Target" => 1,
        "Task" => 2,
        _ => 3,
    }
}

/// Sorts (start; containers first; longer first) and assigns each block an
/// indent equal to the number of still-open blocks.
fn place(blocks: &[TimelineBlock]) -> (Vec<Placed>, usize) {
    let mut sorted: Vec<&TimelineBlock> = blocks.iter().collect();
    sorted.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap()
            .then_with(|| rank(&a.kind).cmp(&rank(&b.kind)))
            .then_with(|| b.end.partial_cmp(&a.end).unwrap())
    });
    let mut rows = Vec::with_capacity(sorted.len());
    let mut open_ends: Vec<f64> = Vec::new();
    let mut max_indent = 0;
    for block in sorted {
        while open_ends.last().map_or(false, |&top| top <= block.start + 0.0005) {
            open_ends.pop();
        }
        let indent = open_ends.len();
        max_indent = max_indent.max(indent);
        rows.push(Placed { block: block.clone(), indent });
        open_ends.push(block.end);
    }
    (rows, max_indent)
}

fn layout(timeline: &Timeline, filter: Filter) -> Layout {
    let mut lanes = Vec::new();
    let mut y = RULER;
    for lane in &timeline.lanes {
        let visible: Vec<TimelineBlock> = lane.blocks.iter().filter(|b| filter.allows(&b.kind)).cloned().collect();
        if visible.is_empty() {
            continue;
        }
        let (rows, max_indent) = place(&visible);
        let height = LANE_HEADER + (max_indent as f32 + 1.) * BLOCK_ROW;
        lanes.push(LaneLayout { lane_id: lane.node_id, y, height, rows });
        y += height + LANE_GAP;
    }
    Layout { lanes, total_height: y + LANE_GAP, duration_ms: timeline.duration_ms.max(1.) }
}

fn block_color(kind: &str, has_error: bool, theme: &Theme) -> Hsla {
    if has_error {
        return theme.error;
    }
    match kind {
        "Project" => rgb(0x34c759).into(),
        "ProjectEvaluation" => rgb(0x30b0c7).into(),
        "Target" => rgb(0xaf52de).into(),
        "Task" => rgb(0x0a84ff).into(),
        _ => rgb(0x8e8e93).into(),
    }
}

pub struct TimelineView {
    session: Arc<Session>,
    timeline: Option<Arc<Timeline>>,
    layout: Option<Arc<Layout>>,
    counts: Counts,
    error: Option<String>,
    filter: Filter,
    /// 0 = fit to width on the next paint.
    px_per_ms: f32,
    scroll: Point<f32>,
    bounds: Bounds<Pixels>,
    hover: Option<(TimelineBlock, Point<Pixels>)>,
}

impl EventEmitter<TimelineEvent> for TimelineView {}

impl TimelineView {
    pub fn new(session: Arc<Session>, cx: &mut Context<Self>) -> Self {
        let mut this = TimelineView {
            session,
            timeline: None,
            layout: None,
            counts: Counts::default(),
            error: None,
            filter: Filter::default(),
            px_per_ms: 0.,
            scroll: point(0., 0.),
            bounds: Bounds::default(),
            hover: None,
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.timeline().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(timeline) => {
                        let mut counts = Counts::default();
                        for lane in &timeline.lanes {
                            for block in &lane.blocks {
                                match category(&block.kind) {
                                    Category::Evaluation => counts.evaluations += 1,
                                    Category::Project => counts.projects += 1,
                                    Category::Target => counts.targets += 1,
                                    Category::Task => counts.tasks += 1,
                                    Category::Other => counts.other += 1,
                                }
                            }
                        }
                        this.counts = counts;
                        this.layout = Some(Arc::new(layout(&timeline, this.filter)));
                        this.timeline = Some(Arc::new(timeline));
                    }
                    Err(err) => this.error = Some(format!("{err:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn relayout(&mut self, cx: &mut Context<Self>) {
        if let Some(timeline) = &self.timeline {
            self.layout = Some(Arc::new(layout(timeline, self.filter)));
        }
        cx.notify();
    }

    fn content_width(&self) -> f32 {
        self.layout.as_ref().map_or(0., |l| l.duration_ms as f32 * self.px_per_ms + LEFT_PAD * 2.)
    }

    fn clamp_scroll(&mut self) {
        let view_w: f32 = self.bounds.size.width.into();
        let view_h: f32 = self.bounds.size.height.into();
        let max_x = (self.content_width() - view_w).max(0.);
        let max_y = self.layout.as_ref().map_or(0., |l| (l.total_height - view_h).max(0.));
        self.scroll.x = self.scroll.x.clamp(0., max_x);
        self.scroll.y = self.scroll.y.clamp(0., max_y);
    }

    fn set_zoom(&mut self, new_px_per_ms: f32, anchor_x: f32, cx: &mut Context<Self>) {
        let clamped = new_px_per_ms.clamp(0.0001, 500.);
        if self.px_per_ms <= 0. {
            self.px_per_ms = clamped;
            cx.notify();
            return;
        }
        // Keep the time under `anchor_x` (view-local) stable.
        let anchor_ms = (self.scroll.x + anchor_x - LEFT_PAD) / self.px_per_ms;
        self.px_per_ms = clamped;
        self.scroll.x = anchor_ms * self.px_per_ms + LEFT_PAD - anchor_x;
        self.clamp_scroll();
        cx.notify();
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center: f32 = f32::from(self.bounds.size.width) / 2.;
        let current = self.px_per_ms;
        self.set_zoom(current * factor, center, cx);
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        self.px_per_ms = 0.;
        self.scroll = point(0., 0.);
        cx.notify();
    }

    fn block_at(&self, window_point: Point<Pixels>) -> Option<TimelineBlock> {
        let layout = self.layout.as_ref()?;
        if !self.bounds.contains(&window_point) || self.px_per_ms <= 0. {
            return None;
        }
        let local_x: f32 = (window_point.x - self.bounds.origin.x).into();
        let local_y: f32 = (window_point.y - self.bounds.origin.y).into();
        let x = local_x + self.scroll.x;
        let y = local_y + self.scroll.y;
        for lane in &layout.lanes {
            if y < lane.y + LANE_HEADER || y >= lane.y + lane.height {
                continue;
            }
            let indent = ((y - lane.y - LANE_HEADER) / BLOCK_ROW) as usize;
            let ms = ((x - LEFT_PAD) / self.px_per_ms) as f64;
            let mut hit = None;
            for placed in &lane.rows {
                if placed.block.start > ms {
                    break;
                }
                if placed.indent == indent && placed.block.end as f32 * self.px_per_ms + LEFT_PAD + MIN_VISIBLE >= x {
                    hit = Some(placed.block.clone());
                }
            }
            return hit;
        }
        None
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height());
        if event.modifiers.alt {
            let dy: f32 = delta.y.into();
            let factor = if dy > 0. { 1.1 } else { 1. / 1.1 };
            let anchor: f32 = (event.position.x - self.bounds.origin.x).into();
            let current = self.px_per_ms;
            self.set_zoom(current * factor, anchor, cx);
            return;
        }
        let dx: f32 = delta.x.into();
        let dy: f32 = delta.y.into();
        self.scroll.x -= dx;
        self.scroll.y -= dy;
        self.clamp_scroll();
        self.hover = None;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let hit = self.block_at(event.position);
        let changed = match (&self.hover, &hit) {
            (Some((a, _)), Some(b)) => a.id != b.id,
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.hover = hit.map(|b| (b, event.position));
            cx.notify();
        } else if let Some((_, pos)) = &mut self.hover {
            *pos = event.position;
        }
    }

    fn on_click(&mut self, event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(block) = self.block_at(event.position()) {
            cx.emit(TimelineEvent::Reveal(block.id));
        }
    }

    fn render_controls(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let counts = self.counts;
        let toggle = |id: &'static str, label: String, on: bool, f: fn(&mut Filter) -> &mut bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(6.))
                .h(px(20.))
                .rounded(px(4.))
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(theme.hover))
                .text_color(if on { theme.text } else { theme.text_tertiary })
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    let flag = f(&mut this.filter);
                    *flag = !*flag;
                    this.relayout(cx);
                }))
                .child(div().w(px(12.)).text_color(theme.accent).child(if on { "✓" } else { "" }))
                .child(label)
        };
        let legend = |color: Hsla, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().w(px(8.)).h(px(8.)).rounded(px(2.)).bg(color))
                .child(div().text_color(theme.text_secondary).child(label))
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px(px(8.))
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .bg(theme.selection_inactive)
                .text_color(theme.text)
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.85))
                .child(label)
        };

        let lane_count = self.timeline.as_ref().map_or(0, |t| t.lanes.len());
        let duration = self.timeline.as_ref().map_or(0., |t| t.duration_ms);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .w_full()
            .flex_none()
            .px(px(10.))
            .bg(theme.titlebar_background)
            .border_t_1()
            .border_color(theme.border)
            .text_size(px(11.))
            .child(div().text_color(theme.text_secondary).child(format!("{lane_count} node{}", if lane_count == 1 { "" } else { "s" })))
            .child(div().font_family(crate::theme::MONO).text_color(theme.text_secondary).child(format_duration(duration)))
            .child(div().w(px(1.)).h(px(14.)).bg(theme.border))
            .child(toggle("f-eval", format!("Evaluations ({})", counts.evaluations), self.filter.evaluations, |f| &mut f.evaluations))
            .child(toggle("f-proj", format!("Projects ({})", counts.projects), self.filter.projects, |f| &mut f.projects))
            .child(toggle("f-target", format!("Targets ({})", counts.targets), self.filter.targets, |f| &mut f.targets))
            .child(toggle("f-task", format!("Tasks ({})", counts.tasks), self.filter.tasks, |f| &mut f.tasks))
            .when(counts.other > 0, |d| d.child(toggle("f-other", format!("Other ({})", counts.other), self.filter.other, |f| &mut f.other)))
            .child(div().flex_1())
            .child(legend(block_color("ProjectEvaluation", false, theme), "Evaluation"))
            .child(legend(block_color("Project", false, theme), "Project"))
            .child(legend(block_color("Target", false, theme), "Target"))
            .child(legend(block_color("Task", false, theme), "Task"))
            .child(legend(theme.error, "Failed"))
            .child(div().w(px(1.)).h(px(14.)).bg(theme.border))
            .child(button("zoom-out", "−").on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.zoom_by(1. / 1.5, cx))))
            .child(button("zoom-fit", "Fit").on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.fit(cx))))
            .child(button("zoom-in", "+").on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.zoom_by(1.5, cx))))
            .into_any_element()
    }

    fn render_tooltip(&self, theme: &Theme) -> Option<gpui::AnyElement> {
        let (block, position) = self.hover.as_ref()?;
        let mut lines = vec![
            format!("{} {}", block.kind, block.text.clone().unwrap_or_default()),
            format!("Duration: {}", format_duration(block.duration())),
            format!("Start: +{}", format_duration(block.start)),
        ];
        if block.has_error {
            lines.push("Contains errors".into());
        }
        let mut panel = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_w(px(420.))
            .px(px(10.))
            .py(px(7.))
            .rounded(px(6.))
            .bg(theme.window_background)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .text_size(px(11.))
            .text_color(theme.text);
        for (i, line) in lines.into_iter().enumerate() {
            panel = panel.child(div().when(i > 0, |d| d.text_color(theme.text_secondary)).child(line));
        }
        Some(
            deferred(
                anchored()
                    .position(point(position.x + px(14.), position.y + px(16.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(panel),
            )
            .into_any_element(),
        )
    }
}

struct Frame {
    layout: Arc<Layout>,
    px_per_ms: f32,
    scroll: Point<f32>,
    theme: Theme,
    font: Font,
}

impl Render for TimelineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();

        let Some(layout) = self.layout.clone() else {
            let text = self.error.clone().unwrap_or_else(|| "Building timeline…".into());
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.content_background)
                .text_size(px(12.))
                .text_color(if self.error.is_some() { theme.error } else { theme.text_secondary })
                .child(text)
                .into_any_element();
        };
        if layout.lanes.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.content_background)
                .text_color(theme.text_secondary)
                .child("No timing data")
                .into_any_element();
        }

        let entity = cx.entity();
        let px_per_ms = self.px_per_ms;
        let scroll = self.scroll;
        let paint_layout = layout.clone();

        let chart = canvas(
            move |bounds, window, cx| {
                // Record geometry and settle fit-to-width before painting.
                let frame_layout = paint_layout.clone();
                let (zoom, scroll) = entity.update(cx, |this, _| {
                    this.bounds = bounds;
                    if this.px_per_ms <= 0. {
                        let visible: f32 = f32::from(bounds.size.width) - LEFT_PAD * 2.;
                        this.px_per_ms = (visible.max(50.) / frame_layout.duration_ms as f32).max(0.0001);
                    }
                    this.clamp_scroll();
                    (this.px_per_ms, this.scroll)
                });
                let style = window.text_style();
                Frame { layout: frame_layout, px_per_ms: zoom, scroll, theme, font: style.font() }
            },
            move |bounds, frame: Frame, window, cx| paint_chart(bounds, frame, window, cx),
        )
        .size_full();

        let _ = (px_per_ms, scroll);
        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.content_background)
            .child(
                div()
                    .id("timeline-chart")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .cursor(if self.hover.is_some() { CursorStyle::PointingHand } else { CursorStyle::Arrow })
                    .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_click(cx.listener(Self::on_click))
                    .child(chart),
            )
            .child(self.render_controls(&theme, cx));
        if let Some(tooltip) = self.render_tooltip(&theme) {
            root = root.child(tooltip);
        }
        root.into_any_element()
    }
}

fn paint_chart(bounds: Bounds<Pixels>, frame: Frame, window: &mut Window, cx: &mut gpui::App) {
    let theme = frame.theme;
    let origin = bounds.origin;
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    let sx = frame.scroll.x;
    let sy = frame.scroll.y;
    let zoom = frame.px_per_ms;
    let x_for = |ms: f64| origin.x + px(ms as f32 * zoom + LEFT_PAD - sx);
    let y_for = |y: f32| origin.y + px(y - sy);
    let ms_for_local = |local_x: f32| ((local_x + sx - LEFT_PAD) / zoom) as f64;

    let label_font = frame.font.clone();
    let mut label_font_bold = frame.font.clone();
    label_font_bold.weight = gpui::FontWeight::SEMIBOLD;
    let mut mono = frame.font.clone();
    mono.family = crate::theme::MONO.into();

    let shape = |text: String, font: &Font, size: f32, color: Hsla, window: &Window| {
        let run = TextRun { len: text.len(), font: font.clone(), color, background_color: None, underline: None, strikethrough: None };
        window.text_system().shape_line(text.into(), px(size), &[run], None)
    };

    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        // Lane separators, headers and blocks.
        let visible_start = ms_for_local(0.) - 1.;
        let visible_end = ms_for_local(width) + 1.;
        for lane in &frame.layout.lanes {
            let top: f32 = lane.y - sy;
            if top + lane.height + LANE_GAP < 0. || top > height {
                continue;
            }
            let rows_top = lane.y + LANE_HEADER;
            for placed in &lane.rows {
                let block = &placed.block;
                if block.start > visible_end {
                    break;
                }
                if block.end < visible_start {
                    continue;
                }
                let x0 = x_for(block.start);
                let x1 = x_for(block.end);
                let w = (x1 - x0).max(px(MIN_VISIBLE));
                let y = y_for(rows_top + placed.indent as f32 * BLOCK_ROW + 1.);
                let rect = Bounds::new(point(x0, y), size(w, px(BLOCK_ROW - 2.)));
                let color = block_color(&block.kind, block.has_error, &theme);
                let border = if f32::from(w) > 3. { px(0.5) } else { px(0.) };
                window.paint_quad(quad(
                    rect,
                    px(2.),
                    color.opacity(0.55),
                    gpui::Edges::all(border),
                    color,
                    BorderStyle::Solid,
                ));
                if f32::from(w) > 40. {
                    if let Some(text) = block.text.as_ref().filter(|t| !t.is_empty()) {
                        let avail = f32::from(w) - 8.;
                        let max_chars = ((avail / 6.) as usize).max(1);
                        let label: String = if text.chars().count() > max_chars {
                            let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect();
                            s.push('…');
                            s
                        } else {
                            text.clone()
                        };
                        let line = shape(label, &label_font, 10., theme.text, window);
                        if f32::from(line.width) <= avail + 2. {
                            window.with_content_mask(Some(ContentMask { bounds: rect }), |window| {
                                line.paint(point(x0 + px(4.), y + px(1.)), px(BLOCK_ROW - 2.), gpui::TextAlign::Left, None, window, cx).ok();
                            });
                        }
                    }
                }
            }

            // Header pinned to the visible left edge.
            let title = if lane.lane_id == 0 { "Evaluation".to_string() } else { format!("Node {}", lane.lane_id) };
            let header = shape(title, &label_font_bold, 10., theme.text_secondary, window);
            header.paint(point(origin.x + px(LEFT_PAD), y_for(lane.y + 4.)), px(14.), gpui::TextAlign::Left, None, window, cx).ok();

            let sep_y = y_for(lane.y + lane.height + LANE_GAP / 2.);
            window.paint_quad(gpui::fill(Bounds::new(point(origin.x, sep_y), size(bounds.size.width, px(1.))), theme.border));
        }

        // Ruler: 1/2/5×10^n ms ticks at ≥80px spacing, drawn over the lanes.
        window.paint_quad(gpui::fill(Bounds::new(origin, size(bounds.size.width, px(RULER))), theme.content_background));
        let mut step: f64 = 1.;
        while step as f32 * zoom < 80. {
            let magnitude = 10f64.powf(step.log10().floor());
            let mantissa = step / magnitude;
            step = if mantissa < 2. { 2. * magnitude } else if mantissa < 5. { 5. * magnitude } else { 10. * magnitude };
        }
        let mut tick = (ms_for_local(0.).max(0.) / step).floor() * step;
        while tick <= frame.layout.duration_ms {
            let x = x_for(tick);
            if x > origin.x + bounds.size.width {
                break;
            }
            window.paint_quad(gpui::fill(Bounds::new(point(x, origin.y + px(RULER - 4.)), size(px(1.), px(4.))), theme.border));
            window.paint_quad(gpui::fill(Bounds::new(point(x, origin.y + px(RULER)), size(px(1.), bounds.size.height)), theme.guide));
            let label = if tick >= 1000. {
                let secs = tick / 1000.;
                if (secs - secs.round()).abs() < 1e-9 { format!("{} s", secs.round() as i64) } else { format!("{secs} s") }
            } else if (tick - tick.round()).abs() < 1e-9 {
                format!("{} ms", tick.round() as i64)
            } else {
                format!("{tick} ms")
            };
            let line = shape(label, &mono, 9., theme.text_tertiary, window);
            line.paint(point(x + px(3.), origin.y + px(4.)), px(12.), gpui::TextAlign::Left, None, window, cx).ok();
            tick += step;
        }
    });
}
