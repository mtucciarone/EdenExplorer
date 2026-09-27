//! Hand-rolled Mermaid diagram parsing + layout for the Markdown preview's
//! fenced ```mermaid``` code blocks. Real Mermaid.js needs a browser DOM to
//! lay itself out - there is no practical way to run it inside this app,
//! which deliberately has no embedded browser/JS runtime (see `CLAUDE.md`).
//! Instead this recognizes the two most common diagram kinds - flowcharts
//! and sequence diagrams - with a permissive, best-effort parser (skip what
//! isn't recognized rather than fail) and a simple layered layout, producing
//! plain geometry that `itemviewer_preview.rs` paints with `egui::Painter`
//! primitives - no SVG, no external renderer.
//!
//! Deliberately out of scope: class/state/gantt/ER diagrams, `loop`/`alt`
//! grouping boxes and `Note` blocks in sequence diagrams (their lines are
//! silently skipped - the messages inside a skipped `loop`/`alt` block are
//! still parsed and shown, just without the surrounding box), and edge
//! styles beyond solid/dashed (thick `==>` renders the same as `-->`).

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Vec2, pos2, vec2};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

pub enum MermaidDiagram {
    Flowchart(FlowchartLayout),
    Sequence(SequenceLayout),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    Rect,
    Rounded,
    Diamond,
    Circle,
}

pub struct FlowNode {
    pub rect: Rect,
    pub shape: NodeShape,
    pub label: String,
}

pub struct FlowEdge {
    pub from: Pos2,
    pub to: Pos2,
    pub label: Option<(Pos2, String)>,
    pub dashed: bool,
}

pub struct FlowchartLayout {
    pub size: Vec2,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
}

pub struct SeqParticipant {
    pub x: f32,
    pub label: String,
}

pub struct SeqMessage {
    pub y: f32,
    pub from_x: f32,
    pub to_x: f32,
    pub label: String,
    pub dashed: bool,
    /// `false` draws an X at the end instead of an arrowhead (Mermaid's
    /// `-x`/`--x`) - rare, but cheap to support alongside the arrow case.
    pub arrowhead: bool,
}

pub struct SequenceLayout {
    pub size: Vec2,
    pub participants: Vec<SeqParticipant>,
    pub lifeline_bottom: f32,
    pub messages: Vec<SeqMessage>,
}

/// Parses `source` (the fenced code block's raw content, without the
/// ` ```mermaid`/``` ` fence lines) and lays it out using `measure` to size
/// boxes/columns to their actual text - `egui::Painter::layout_no_wrap(..).
/// size()` is the intended caller. Returns `None` if the first meaningful
/// line doesn't look like a flowchart or sequence diagram header, or if a
/// recognized diagram ends up with no nodes/messages at all (nothing usable
/// to draw) - either way the caller falls back to showing the block as
/// plain/highlighted code instead.
pub fn parse_and_layout(
    source: &str,
    font_id: &FontId,
    measure: &dyn Fn(&str, &FontId) -> Vec2,
) -> Option<MermaidDiagram> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("%%"));

    let header = lines.next()?;
    if is_sequence_header(header) {
        parse_sequence(lines, font_id, measure).map(MermaidDiagram::Sequence)
    } else if is_flowchart_header(header) {
        parse_flowchart(lines, font_id, measure).map(MermaidDiagram::Flowchart)
    } else {
        None
    }
}

/// The diagram's total bounding size, so a caller can allocate exactly the
/// right amount of space (and know whether it needs to scroll) before
/// painting.
pub fn size(diagram: &MermaidDiagram) -> Vec2 {
    match diagram {
        MermaidDiagram::Flowchart(f) => f.size,
        MermaidDiagram::Sequence(s) => s.size,
    }
}

fn is_sequence_header(line: &str) -> bool {
    line.eq_ignore_ascii_case("sequenceDiagram") || line.starts_with("sequenceDiagram")
}

fn is_flowchart_header(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("graph ")
        || lower == "graph"
        || lower.starts_with("flowchart ")
        || lower == "flowchart"
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowDirection {
    TopDown,
    LeftRight,
    BottomUp,
    RightLeft,
}

fn parse_direction(header: &str) -> FlowDirection {
    let upper = header.to_ascii_uppercase();
    if upper.contains("LR") {
        FlowDirection::LeftRight
    } else if upper.contains("RL") {
        FlowDirection::RightLeft
    } else if upper.contains("BT") {
        FlowDirection::BottomUp
    } else {
        FlowDirection::TopDown
    }
}

// --- Flowchart parsing -----------------------------------------------------

struct NodeDef {
    shape: NodeShape,
    label: String,
    order: usize,
}

static EDGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^([A-Za-z0-9_]+)\s*(\(\([^)]*\)\)|\[[^\]]*\]|\([^)]*\)|\{[^}]*\})?\s*([-=.]{2,}>?)\s*(?:\|([^|]*)\|)?\s*([A-Za-z0-9_]+)\s*(\(\([^)]*\)\)|\[[^\]]*\]|\([^)]*\)|\{[^}]*\})?",
    )
    .unwrap()
});

static NODE_ONLY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9_]+)\s*(\(\([^)]*\)\)|\[[^\]]*\]|\([^)]*\)|\{[^}]*\})\s*$").unwrap()
});

/// Splits a captured shape token (e.g. `"[Ship it]"`, `"((Done))"`) into its
/// `NodeShape` and inner label text.
fn split_shape(token: &str) -> (NodeShape, String) {
    if let Some(inner) = token.strip_prefix("((").and_then(|s| s.strip_suffix("))")) {
        (NodeShape::Circle, inner.trim().to_string())
    } else if let Some(inner) = token.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        (NodeShape::Rect, inner.trim().to_string())
    } else if let Some(inner) = token.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        (NodeShape::Rounded, inner.trim().to_string())
    } else if let Some(inner) = token.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        (NodeShape::Diamond, inner.trim().to_string())
    } else {
        (NodeShape::Rect, token.to_string())
    }
}

fn register_node(
    nodes: &mut HashMap<String, NodeDef>,
    order: &mut usize,
    id: &str,
    shape_token: Option<&str>,
) {
    let (shape, label) = match shape_token {
        Some(tok) => split_shape(tok),
        None => (NodeShape::Rect, id.to_string()),
    };
    let entry = nodes.entry(id.to_string()).or_insert_with(|| {
        let this_order = *order;
        *order += 1;
        NodeDef {
            shape: NodeShape::Rect,
            label: id.to_string(),
            order: this_order,
        }
    });
    // A bare mention (no shape token) never overwrites an already-known
    // shape/label from an earlier, more informative mention of the same id.
    if shape_token.is_some() {
        entry.shape = shape;
        entry.label = label;
    }
}

fn parse_flowchart<'a>(
    lines: impl Iterator<Item = &'a str>,
    font_id: &FontId,
    measure: &dyn Fn(&str, &FontId) -> Vec2,
) -> Option<FlowchartLayout> {
    let mut nodes: HashMap<String, NodeDef> = HashMap::new();
    let mut order = 0usize;
    // (from_id, to_id, label, dashed)
    let mut edges: Vec<(String, String, Option<String>, bool)> = Vec::new();
    let mut direction = FlowDirection::TopDown;
    let mut direction_set = false;

    for line in lines {
        if !direction_set && is_flowchart_header(line) {
            direction = parse_direction(line);
            direction_set = true;
            continue;
        }
        if line.starts_with("subgraph ") || line == "subgraph" || line == "end" || line.starts_with("style ")
            || line.starts_with("classDef ") || line.starts_with("class ")
            || line.starts_with("click ")
        {
            continue;
        }

        if let Some(caps) = EDGE_RE.captures(line) {
            let from_id = &caps[1];
            let from_shape = caps.get(2).map(|m| m.as_str());
            let arrow = &caps[3];
            let label = caps.get(4).map(|m| m.as_str().trim().to_string());
            let to_id = &caps[5];
            let to_shape = caps.get(6).map(|m| m.as_str());

            register_node(&mut nodes, &mut order, from_id, from_shape);
            register_node(&mut nodes, &mut order, to_id, to_shape);
            edges.push((from_id.to_string(), to_id.to_string(), label, arrow.contains('.')));
        } else if let Some(caps) = NODE_ONLY_RE.captures(line) {
            let id = &caps[1];
            let shape = caps.get(2).map(|m| m.as_str());
            register_node(&mut nodes, &mut order, id, shape);
        }
        // Anything else (comments already filtered, unrecognized syntax) is
        // silently skipped - best-effort, never a hard parse failure.
    }

    if nodes.is_empty() {
        return None;
    }

    layout_flowchart(nodes, edges, direction, font_id, measure)
}

const FLOW_H_GAP: f32 = 48.0;
const FLOW_V_GAP: f32 = 56.0;
const FLOW_PAD_X: f32 = 20.0;
const FLOW_PAD_Y: f32 = 14.0;

fn layout_flowchart(
    nodes: HashMap<String, NodeDef>,
    edges: Vec<(String, String, Option<String>, bool)>,
    direction: FlowDirection,
    font_id: &FontId,
    measure: &dyn Fn(&str, &FontId) -> Vec2,
) -> Option<FlowchartLayout> {
    // Stable id order (first-seen), used for deterministic layout and as a
    // fallback ordering within a rank.
    let mut ids: Vec<String> = nodes.keys().cloned().collect();
    ids.sort_by_key(|id| nodes[id].order);
    let index_of: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    let mut in_degree: Vec<usize> = vec![0; ids.len()];
    for (from, to, _, _) in &edges {
        let (Some(&fi), Some(&ti)) = (index_of.get(from.as_str()), index_of.get(to.as_str())) else {
            continue;
        };
        adjacency[fi].push(ti);
        in_degree[ti] += 1;
    }

    // Kahn's algorithm for a topological rank (longest path from a source),
    // falling back to appending anything left over (cycle members) in
    // first-seen order rather than looping forever.
    let mut rank = vec![0usize; ids.len()];
    let mut queue: Vec<usize> = (0..ids.len()).filter(|&i| in_degree[i] == 0).collect();
    queue.sort();
    let mut remaining_in_degree = in_degree.clone();
    let mut visited = vec![false; ids.len()];
    let mut qi = 0;
    while qi < queue.len() {
        let node = queue[qi];
        qi += 1;
        visited[node] = true;
        for &next in &adjacency[node] {
            rank[next] = rank[next].max(rank[node] + 1);
            remaining_in_degree[next] = remaining_in_degree[next].saturating_sub(1);
            if remaining_in_degree[next] == 0 && !visited[next] {
                queue.push(next);
            }
        }
    }
    let max_rank_from_topo = rank.iter().copied().max().unwrap_or(0);
    for (i, id) in ids.iter().enumerate() {
        if !visited[i] {
            rank[i] = max_rank_from_topo + 1 + nodes[id].order;
        }
    }

    // Group into ranks, each rank ordered by first-seen order.
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, &r) in rank.iter().enumerate() {
        ranks[r].push(i);
    }
    for bucket in &mut ranks {
        bucket.sort_by_key(|&i| nodes[&ids[i]].order);
    }

    // Measure every node's box size from its label.
    let sizes: Vec<Vec2> = ids
        .iter()
        .map(|id| {
            let def = &nodes[id];
            let text_size = measure(&def.label, font_id);
            match def.shape {
                NodeShape::Diamond => vec2(text_size.x * 1.8 + FLOW_PAD_X, text_size.y * 1.8 + FLOW_PAD_Y * 2.0),
                NodeShape::Circle => {
                    let d = (text_size.x.max(text_size.y) * 1.6 + FLOW_PAD_X).max(text_size.y + FLOW_PAD_Y * 2.0);
                    vec2(d, d)
                }
                _ => vec2(text_size.x + FLOW_PAD_X * 2.0, text_size.y + FLOW_PAD_Y * 2.0),
            }
        })
        .collect();

    let horizontal = matches!(direction, FlowDirection::LeftRight | FlowDirection::RightLeft);

    // "cross" = size along the axis perpendicular to rank progression;
    // "along" = size along the rank-progression axis.
    let rank_cross_extent: Vec<f32> = ranks
        .iter()
        .map(|bucket| {
            bucket
                .iter()
                .map(|&i| if horizontal { sizes[i].y } else { sizes[i].x })
                .sum::<f32>()
                + FLOW_H_GAP * bucket.len().saturating_sub(1) as f32
        })
        .collect();
    let rank_along_extent: Vec<f32> = ranks
        .iter()
        .map(|bucket| {
            bucket
                .iter()
                .map(|&i| if horizontal { sizes[i].x } else { sizes[i].y })
                .fold(0.0_f32, f32::max)
        })
        .collect();

    let total_cross = rank_cross_extent.iter().copied().fold(0.0_f32, f32::max).max(1.0);
    let total_along: f32 = rank_along_extent.iter().sum::<f32>()
        + FLOW_V_GAP * max_rank as f32;

    let mut rects = vec![Rect::NOTHING; ids.len()];
    let mut along_offset = 0.0_f32;
    for (r, bucket) in ranks.iter().enumerate() {
        let cross_extent = rank_cross_extent[r];
        let mut cross_offset = (total_cross - cross_extent) / 2.0;
        for &i in bucket {
            let size = sizes[i];
            let (cross_size, _along_size) = if horizontal { (size.y, size.x) } else { (size.x, size.y) };
            let min = if horizontal {
                pos2(along_offset, cross_offset)
            } else {
                pos2(cross_offset, along_offset)
            };
            rects[i] = Rect::from_min_size(min, size);
            cross_offset += cross_size + FLOW_H_GAP;
        }
        along_offset += rank_along_extent[r] + FLOW_V_GAP;
    }

    // BT/RL just run the rank axis in reverse.
    let along_span = total_along;
    if matches!(direction, FlowDirection::BottomUp) {
        for rect in &mut rects {
            let flipped_top = along_span - (rect.min.y - 0.0) - rect.height();
            *rect = Rect::from_min_size(pos2(rect.min.x, flipped_top), rect.size());
        }
    } else if matches!(direction, FlowDirection::RightLeft) {
        for rect in &mut rects {
            let flipped_left = along_span - rect.min.x - rect.width();
            *rect = Rect::from_min_size(pos2(flipped_left, rect.min.y), rect.size());
        }
    }

    let size = if horizontal {
        vec2(along_span, total_cross)
    } else {
        vec2(total_cross, along_span)
    };

    let flow_nodes: Vec<FlowNode> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| FlowNode {
            rect: rects[i],
            shape: nodes[id].shape,
            label: nodes[id].label.clone(),
        })
        .collect();

    let flow_edges: Vec<FlowEdge> = edges
        .iter()
        .filter_map(|(from, to, label, dashed)| {
            let fi = *index_of.get(from.as_str())?;
            let ti = *index_of.get(to.as_str())?;
            let (from_pt, to_pt) = edge_endpoints(rects[fi], rects[ti], direction);
            let label = label.as_ref().filter(|l| !l.is_empty()).map(|text| {
                let mid = from_pt + (to_pt - from_pt) * 0.5;
                (mid, text.clone())
            });
            Some(FlowEdge {
                from: from_pt,
                to: to_pt,
                label,
                dashed: *dashed,
            })
        })
        .collect();

    Some(FlowchartLayout {
        size,
        nodes: flow_nodes,
        edges: flow_edges,
    })
}

/// Connects the boundary of `from` to the boundary of `to` along whichever
/// axis the diagram flows on - good enough for a layered graph where most
/// edges run rank-to-rank in the primary direction; a same-rank or back edge
/// just ends up a straight line between the two nearest boundary points,
/// which is visually acceptable rather than exact.
fn edge_endpoints(from: Rect, to: Rect, direction: FlowDirection) -> (Pos2, Pos2) {
    let from_c = from.center();
    let to_c = to.center();
    match direction {
        FlowDirection::TopDown | FlowDirection::BottomUp => {
            if to_c.y >= from_c.y {
                (pos2(from_c.x, from.max.y), pos2(to_c.x, to.min.y))
            } else {
                (pos2(from_c.x, from.min.y), pos2(to_c.x, to.max.y))
            }
        }
        FlowDirection::LeftRight | FlowDirection::RightLeft => {
            if to_c.x >= from_c.x {
                (pos2(from.max.x, from_c.y), pos2(to.min.x, to_c.y))
            } else {
                (pos2(from.min.x, from_c.y), pos2(to.max.x, to_c.y))
            }
        }
    }
}

// --- Sequence diagram parsing ----------------------------------------------

static SEQ_PARTICIPANT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:participant|actor)\s+([A-Za-z0-9_]+)(?:\s+as\s+(.+))?$").unwrap()
});

static SEQ_MESSAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9_]+)\s*(-{1,2}>{1,2}|-{1,2}x)\s*([A-Za-z0-9_]+)\s*:\s*(.*)$").unwrap()
});

fn parse_sequence<'a>(
    lines: impl Iterator<Item = &'a str>,
    font_id: &FontId,
    measure: &dyn Fn(&str, &FontId) -> Vec2,
) -> Option<SequenceLayout> {
    let mut participant_order: Vec<String> = Vec::new();
    let mut participant_label: HashMap<String, String> = HashMap::new();
    // (from, to, text, dashed, arrowhead)
    let mut messages: Vec<(String, String, String, bool, bool)> = Vec::new();

    let ensure_participant = |id: &str, participant_order: &mut Vec<String>, participant_label: &mut HashMap<String, String>| {
        if !participant_label.contains_key(id) {
            participant_order.push(id.to_string());
            participant_label.insert(id.to_string(), id.to_string());
        }
    };

    for line in lines {
        // Note the trailing space (or bare exact match) on every keyword
        // check below - `starts_with("par")` alone would also swallow every
        // `participant ...` line, since "participant" itself starts with
        // "par". Same reasoning for "and"/"alt"/"opt" against any real
        // identifier that happens to start the same way.
        if line.starts_with("loop ") || line == "loop"
            || line.starts_with("alt ") || line == "alt"
            || line.starts_with("else") // "else" and "else if ..." both valid, no space required
            || line.starts_with("opt ") || line == "opt"
            || line.starts_with("par ") || line == "par"
            || line.starts_with("and ") || line == "and"
            || line == "end" || line.starts_with("Note ") || line.starts_with("activate ")
            || line.starts_with("deactivate ") || line.starts_with("autonumber")
        {
            continue;
        }

        if let Some(caps) = SEQ_PARTICIPANT_RE.captures(line) {
            let id = caps[1].to_string();
            let label = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| id.clone());
            if !participant_order.contains(&id) {
                participant_order.push(id.clone());
            }
            participant_label.insert(id, label);
            continue;
        }

        if let Some(caps) = SEQ_MESSAGE_RE.captures(line) {
            let from = caps[1].to_string();
            let arrow = &caps[2];
            let to = caps[3].to_string();
            let text = caps[4].trim().to_string();
            ensure_participant(&from, &mut participant_order, &mut participant_label);
            ensure_participant(&to, &mut participant_order, &mut participant_label);
            let dashed = arrow.starts_with("--");
            let arrowhead = !arrow.ends_with('x');
            messages.push((from, to, text, dashed, arrowhead));
        }
    }

    if participant_order.is_empty() || messages.is_empty() {
        return None;
    }

    layout_sequence(participant_order, participant_label, messages, font_id, measure)
}

const SEQ_HEADER_HEIGHT: f32 = 34.0;
const SEQ_ROW_HEIGHT: f32 = 40.0;
const SEQ_COL_PAD: f32 = 28.0;
const SEQ_MIN_COL_WIDTH: f32 = 90.0;

fn layout_sequence(
    order: Vec<String>,
    labels: HashMap<String, String>,
    messages: Vec<(String, String, String, bool, bool)>,
    font_id: &FontId,
    measure: &dyn Fn(&str, &FontId) -> Vec2,
) -> Option<SequenceLayout> {
    let index_of: HashMap<&str, usize> = order.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

    // Column width = widest of the participant's own label and half of any
    // message text touching it (so labels don't get squeezed by cramped
    // columns) - simplified to just the label width plus a fixed pad, which
    // covers the common case without needing per-message text wrapping.
    let mut col_width = vec![SEQ_MIN_COL_WIDTH; order.len()];
    for (i, id) in order.iter().enumerate() {
        let label = &labels[id];
        let w = measure(label, font_id).x + SEQ_COL_PAD * 2.0;
        col_width[i] = col_width[i].max(w);
    }

    let mut x = 0.0_f32;
    let mut xs = Vec::with_capacity(order.len());
    for (i, _) in order.iter().enumerate() {
        xs.push(x + col_width[i] / 2.0);
        x += col_width[i] + FLOW_H_GAP;
    }
    let total_width = x - FLOW_H_GAP;

    let lifeline_bottom = SEQ_HEADER_HEIGHT + SEQ_ROW_HEIGHT * messages.len() as f32 + 20.0;

    let seq_messages: Vec<SeqMessage> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, (from, to, text, dashed, arrowhead))| {
            let from_x = *xs.get(*index_of.get(from.as_str())?)?;
            let to_x = *xs.get(*index_of.get(to.as_str())?)?;
            Some(SeqMessage {
                y: SEQ_HEADER_HEIGHT + SEQ_ROW_HEIGHT * (i as f32 + 1.0),
                from_x,
                to_x,
                label: text.clone(),
                dashed: *dashed,
                arrowhead: *arrowhead,
            })
        })
        .collect();

    let participants = order
        .iter()
        .enumerate()
        .map(|(i, id)| SeqParticipant {
            x: xs[i],
            label: labels[id].clone(),
        })
        .collect();

    Some(SequenceLayout {
        size: vec2(total_width, lifeline_bottom),
        participants,
        lifeline_bottom,
        messages: seq_messages,
    })
}

/// Paints a laid-out diagram at `origin` (top-left) into `ui`'s current
/// painter, using `palette`-derived colors passed in by the caller (kept
/// generic here rather than importing `ThemePalette` directly, so this
/// module has no dependency on the app's theme types).
pub fn paint(
    painter: &egui::Painter,
    origin: Pos2,
    diagram: &MermaidDiagram,
    font_id: &FontId,
    text_color: Color32,
    line_color: Color32,
    fill_color: Color32,
) {
    match diagram {
        MermaidDiagram::Flowchart(flow) => paint_flowchart(painter, origin, flow, font_id, text_color, line_color, fill_color),
        MermaidDiagram::Sequence(seq) => paint_sequence(painter, origin, seq, font_id, text_color, line_color, fill_color),
    }
}

fn paint_flowchart(
    painter: &egui::Painter,
    origin: Pos2,
    flow: &FlowchartLayout,
    font_id: &FontId,
    text_color: Color32,
    line_color: Color32,
    fill_color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, line_color);
    for edge in &flow.edges {
        let from = origin + edge.from.to_vec2();
        let to = origin + edge.to.to_vec2();
        if edge.dashed {
            paint_dashed_line(painter, from, to, stroke);
        } else {
            painter.line_segment([from, to], stroke);
        }
        let dir = to - from;
        if dir.length() > 0.5 {
            let tip_len = 10.0_f32.min(dir.length() / 3.0);
            painter.arrow(to - dir.normalized() * tip_len, dir.normalized() * tip_len, stroke);
        }
        if let Some((pos, label)) = &edge.label {
            let p = origin + pos.to_vec2();
            painter.rect_filled(
                Rect::from_center_size(p, painter.layout_no_wrap(label.clone(), font_id.clone(), text_color).size() + vec2(6.0, 2.0)),
                2.0,
                fill_color,
            );
            painter.text(p, egui::Align2::CENTER_CENTER, label, font_id.clone(), text_color);
        }
    }

    for node in &flow.nodes {
        let rect = Rect::from_min_size(origin + node.rect.min.to_vec2(), node.rect.size());
        match node.shape {
            NodeShape::Rect => {
                painter.rect_filled(rect, 3.0, fill_color);
                painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);
            }
            NodeShape::Rounded => {
                painter.rect_filled(rect, rect.height() / 2.0, fill_color);
                painter.rect_stroke(rect, rect.height() / 2.0, stroke, egui::StrokeKind::Outside);
            }
            NodeShape::Circle => {
                let radius = rect.width().min(rect.height()) / 2.0;
                painter.circle_filled(rect.center(), radius, fill_color);
                painter.circle_stroke(rect.center(), radius, stroke);
            }
            NodeShape::Diamond => {
                let c = rect.center();
                let points = vec![
                    pos2(c.x, rect.min.y),
                    pos2(rect.max.x, c.y),
                    pos2(c.x, rect.max.y),
                    pos2(rect.min.x, c.y),
                ];
                painter.add(egui::Shape::convex_polygon(points.clone(), fill_color, egui::Stroke::NONE));
                painter.add(egui::Shape::closed_line(points, stroke));
            }
        }
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, &node.label, font_id.clone(), text_color);
    }
}

fn paint_sequence(
    painter: &egui::Painter,
    origin: Pos2,
    seq: &SequenceLayout,
    font_id: &FontId,
    text_color: Color32,
    line_color: Color32,
    fill_color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, line_color);

    for participant in &seq.participants {
        let x = origin.x + participant.x;
        painter.line_segment(
            [pos2(x, origin.y + SEQ_HEADER_HEIGHT), pos2(x, origin.y + seq.lifeline_bottom)],
            egui::Stroke::new(1.0, line_color.gamma_multiply(0.6)),
        );
        let label_size = painter.layout_no_wrap(participant.label.clone(), font_id.clone(), text_color).size();
        let header_rect = Rect::from_center_size(
            pos2(x, origin.y + SEQ_HEADER_HEIGHT / 2.0),
            vec2(label_size.x + SEQ_COL_PAD, SEQ_HEADER_HEIGHT - 6.0),
        );
        painter.rect_filled(header_rect, 3.0, fill_color);
        painter.rect_stroke(header_rect, 3.0, stroke, egui::StrokeKind::Outside);
        painter.text(header_rect.center(), egui::Align2::CENTER_CENTER, &participant.label, font_id.clone(), text_color);
    }

    for message in &seq.messages {
        let y = origin.y + message.y;
        let from = pos2(origin.x + message.from_x, y);
        let to = pos2(origin.x + message.to_x, y);

        if (from.x - to.x).abs() < 1.0 {
            // Self-message: a small rightward bump back to the same lifeline.
            let bump = pos2(from.x + 50.0, y - 12.0);
            let bump_end = pos2(from.x, y + 12.0);
            let pts = [from, bump, pos2(bump.x, bump_end.y), bump_end];
            for pair in pts.windows(2) {
                if message.dashed {
                    paint_dashed_line(painter, pair[0], pair[1], stroke);
                } else {
                    painter.line_segment([pair[0], pair[1]], stroke);
                }
            }
            painter.arrow(bump_end, (bump_end - pos2(bump.x, bump_end.y - 8.0)).normalized() * 8.0, stroke);
        } else if message.dashed {
            paint_dashed_line(painter, from, to, stroke);
        } else {
            painter.line_segment([from, to], stroke);
        }

        if (from.x - to.x).abs() >= 1.0 {
            let dir = to - from;
            if message.arrowhead {
                let tip_len = 9.0_f32.min(dir.length() / 3.0);
                painter.arrow(to - dir.normalized() * tip_len, dir.normalized() * tip_len, stroke);
            } else {
                let half = 5.0;
                let n = dir.normalized();
                let perp = vec2(-n.y, n.x) * half;
                painter.line_segment([to - n * half - perp, to + n * half + perp], stroke);
                painter.line_segment([to - n * half + perp, to + n * half - perp], stroke);
            }
        }

        if !message.label.is_empty() {
            let mid_x = (from.x + to.x) / 2.0;
            painter.text(pos2(mid_x, y - 6.0), egui::Align2::CENTER_BOTTOM, &message.label, font_id.clone(), text_color);
        }
    }
}

fn paint_dashed_line(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: egui::Stroke) {
    let dir = to - from;
    let len = dir.length();
    if len < 0.5 {
        return;
    }
    let n = dir / len;
    let dash = 6.0_f32;
    let gap = 4.0_f32;
    let mut t = 0.0_f32;
    while t < len {
        let seg_end = (t + dash).min(len);
        painter.line_segment([from + n * t, from + n * seg_end], stroke);
        t += dash + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_measure(text: &str, _font_id: &FontId) -> Vec2 {
        vec2(text.chars().count() as f32 * 7.0, 14.0)
    }

    #[test]
    fn non_mermaid_source_returns_none() {
        let font = FontId::monospace(12.0);
        assert!(parse_and_layout("just some text\nmore text", &font, &fixed_measure).is_none());
    }

    #[test]
    fn simple_flowchart_parses_nodes_and_edges() {
        let font = FontId::monospace(12.0);
        let source = "graph TD\n    A[Start] --> B{Decision}\n    B -->|Yes| C[Ship it]\n    B -->|No| D[Debug]\n    D --> B\n";
        let diagram = parse_and_layout(source, &font, &fixed_measure).expect("should parse");
        let MermaidDiagram::Flowchart(flow) = diagram else {
            panic!("expected a flowchart");
        };
        assert_eq!(flow.nodes.len(), 4);
        assert_eq!(flow.edges.len(), 4);
        let decision = flow.nodes.iter().find(|n| n.label == "Decision").unwrap();
        assert_eq!(decision.shape as u8, NodeShape::Diamond as u8);
        let labeled_edges = flow.edges.iter().filter(|e| e.label.is_some()).count();
        assert_eq!(labeled_edges, 2);
    }

    #[test]
    fn flowchart_lays_out_nodes_in_increasing_rank_order() {
        let font = FontId::monospace(12.0);
        let source = "graph TD\n    A --> B\n    B --> C\n";
        let diagram = parse_and_layout(source, &font, &fixed_measure).expect("should parse");
        let MermaidDiagram::Flowchart(flow) = diagram else {
            panic!("expected a flowchart");
        };
        let a = flow.nodes.iter().find(|n| n.label == "A").unwrap();
        let b = flow.nodes.iter().find(|n| n.label == "B").unwrap();
        let c = flow.nodes.iter().find(|n| n.label == "C").unwrap();
        assert!(a.rect.min.y < b.rect.min.y);
        assert!(b.rect.min.y < c.rect.min.y);
    }

    #[test]
    fn flowchart_with_a_cycle_does_not_hang_and_still_lays_out_every_node() {
        let font = FontId::monospace(12.0);
        let source = "graph TD\n    A --> B\n    B --> C\n    C --> A\n";
        let diagram = parse_and_layout(source, &font, &fixed_measure).expect("should parse");
        let MermaidDiagram::Flowchart(flow) = diagram else {
            panic!("expected a flowchart");
        };
        assert_eq!(flow.nodes.len(), 3);
    }

    #[test]
    fn simple_sequence_diagram_parses_participants_and_messages() {
        let font = FontId::monospace(12.0);
        let source = "sequenceDiagram\n    Alice->>Bob: Hello\n    Bob-->>Alice: Hi there\n";
        let diagram = parse_and_layout(source, &font, &fixed_measure).expect("should parse");
        let MermaidDiagram::Sequence(seq) = diagram else {
            panic!("expected a sequence diagram");
        };
        assert_eq!(seq.participants.len(), 2);
        assert_eq!(seq.messages.len(), 2);
        assert!(!seq.messages[0].dashed);
        assert!(seq.messages[1].dashed);
    }

    #[test]
    fn sequence_diagram_respects_explicit_participant_order_and_alias() {
        let font = FontId::monospace(12.0);
        let source = "sequenceDiagram\n    participant B as Bob\n    participant A as Alice\n    A->>B: Hi\n";
        let diagram = parse_and_layout(source, &font, &fixed_measure).expect("should parse");
        let MermaidDiagram::Sequence(seq) = diagram else {
            panic!("expected a sequence diagram");
        };
        assert_eq!(seq.participants[0].label, "Bob");
        assert_eq!(seq.participants[1].label, "Alice");
    }

    #[test]
    fn sequence_message_with_x_end_has_no_arrowhead() {
        let font = FontId::monospace(12.0);
        let source = "sequenceDiagram\n    Alice-xBob: Timeout\n";
        let diagram = parse_and_layout(source, &font, &fixed_measure).expect("should parse");
        let MermaidDiagram::Sequence(seq) = diagram else {
            panic!("expected a sequence diagram");
        };
        assert!(!seq.messages[0].arrowhead);
    }
}
