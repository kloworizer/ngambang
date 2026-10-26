//! Slips added in 0.1.9: quick notes, battery & network, quick launch, the
//! clipboard history and downloads. Each draws into a column (`b.x`, `b.w`)
//! from `y` down to `b.limit`, so slips and strips share them.

use super::*;

const ROW_TEXT: TextStyle = TextStyle { size: 12.5, weight: Weight::Regular, mono: false };
const ROW_META: TextStyle = TextStyle { size: 10.0, weight: Weight::Regular, mono: true };

/// "just now", "5m", "3h", "2d".
fn age(m: &Model, at: i64) -> String {
    match m.unix_now().map(|now| (now - at).max(0) / 60) {
        None => String::new(),
        Some(0) => "now".into(),
        Some(min) if min < 60 => format!("{min}m"),
        Some(min) if min < 48 * 60 => format!("{}h", min / 60),
        Some(min) => format!("{}d", min / 1440),
    }
}

/// A one-line text field with a caret, for the slip's input.
pub(super) fn field(b: &mut B, r: Rect, text: &str, hint: &str, accent: Color) {
    b.prims.extend(soft::well(r, r.h.min(24.0) / 2.0));
    let (tx, tw) = (r.x + 12.0, r.w - 24.0);
    if text.is_empty() {
        b.text(Rect::new(tx, r.y, tw, r.h), hint, ROW_TEXT, LABEL);
        b.rect(Rect::new(tx, r.y + r.h / 2.0 - 8.0, 2.0, 16.0), 1.0, accent);
    } else {
        let mut shown = text.to_string();
        while b.tm.width(&shown, ROW_TEXT) > tw - 6.0 && !shown.is_empty() {
            shown.remove(0);
        }
        let w = b.tm.width(&shown, ROW_TEXT).min(tw - 4.0);
        b.text(Rect::new(tx, r.y, tw, r.h), shown, ROW_TEXT, TEXT);
        b.rect(Rect::new(tx + w + 2.0, r.y + r.h / 2.0 - 8.0, 2.0, 16.0), 1.0, accent);
    }
}

/// A small icon button without a face (a row's tool).
fn tool(b: &mut B, r: Rect, icon: Icon, color: Color, action: Action, label: String) {
    let s = 13.0;
    b.icon(icon, Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s), color);
    b.hits.push(Hit { rect: r, kind: HitKind::Click(action), label });
}

// ---- notes ----------------------------------------------------------------

/// Quick notes: a field while writing, then the notes newest first.
pub(super) fn notes(m: &Model, b: &mut B, mut y: f32, bottom_buttons: bool) -> f32 {
    let row_h = 38.0;
    if let Some(text) = m.typing(InputFor::Note) {
        field(b, Rect::new(b.x, y, b.w, 34.0), text, "Write a note…", OK);
        y += 42.0;
        let bw = (b.w - 8.0) / 2.0;
        b.ibutton(Rect::new(b.x, y, bw, 32.0), Some(Icon::Check), "Save", Btn::Primary(OK), Action::TodoSubmit);
        b.ibutton(Rect::new(b.x + bw + 8.0, y, bw, 32.0), Some(Icon::Cross), "Cancel", Btn::Secondary, Action::TodoCancel);
        y += 40.0;
    } else if m.notes.is_empty() {
        b.text(Rect::new(b.x, y, b.w, 20.0), "No notes yet.", ROW_TEXT, TEXT_2);
        b.text(Rect::new(b.x, y + 20.0, b.w, 16.0), "ngambang note \"…\" adds one from a terminal", ROW_META, LABEL);
        y += 44.0;
    }
    let reserve = if bottom_buttons && m.typing(InputFor::Note).is_none() { 44.0 } else { 0.0 };
    let fit = ((b.limit - reserve - y) / row_h).floor().max(0.0) as usize;
    let shown = m.notes.len().min(fit);
    for n in m.notes.iter().take(shown) {
        let r = Rect::new(b.x, y, b.w, row_h - 4.0);
        b.rect(Rect::new(r.x, r.y + 5.0, 3.0, r.h - 10.0), 1.5, OK);
        b.text(Rect::new(r.x + 12.0, r.y, r.w - 12.0 - 60.0, 20.0), n.text.clone(), ROW_TEXT, TEXT);
        b.text(Rect::new(r.x + 12.0, r.y + 18.0, r.w - 72.0, 14.0), age(m, n.at), ROW_META, LABEL);
        b.hits.push(Hit {
            rect: Rect::new(r.x, r.y, r.w - 60.0, r.h),
            kind: HitKind::Click(Action::NoteCopy(n.id)),
            label: format!("Copy note: {}", n.text),
        });
        tool(b, Rect::new(r.right() - 58.0, r.y + 3.0, 28.0, 28.0), Icon::Copy, TEXT_2, Action::NoteCopy(n.id), "Copy note".into());
        tool(b, Rect::new(r.right() - 28.0, r.y + 3.0, 28.0, 28.0), Icon::Trash, TEXT_2, Action::NoteRemove(n.id), "Delete note".into());
        y += row_h;
    }
    if m.notes.len() > shown && shown > 0 {
        b.text(Rect::new(b.x + 12.0, y - 2.0, b.w, 14.0), format!("+{} more", m.notes.len() - shown), ROW_META, LABEL);
        y += 14.0;
    }
    if bottom_buttons && m.typing(InputFor::Note).is_none() {
        let by = b.pin(y + 6.0, 36.0);
        b.ibutton(Rect::new(b.x, by, b.w, 36.0), Some(Icon::Plus), "New note", Btn::Primary(OK), Action::NoteAdd);
        y = by + 36.0;
    }
    y
}

// ---- battery, network and power plans --------------------------------------

/// The battery, large; the connection and VPN under it.
pub(super) fn power_now(m: &Model, b: &mut B, y: f32) -> f32 {
    let (x, w) = (b.x, b.w);
    match m.power.and_then(|p| p.percent.map(|pct| (p, pct))) {
        Some((p, pct)) => {
            let color = if !p.on_ac && pct <= m.config.power.low_battery as u8 { PRIVACY_TEXT } else { OK };
            b.icon(Icon::Battery, Rect::new(x, y + 8.0, 34.0, 34.0), color);
            b.text(
                Rect::new(x + 44.0, y - 2.0, 120.0, 48.0),
                format!("{pct}%"),
                TextStyle { size: 38.0, weight: Weight::Semibold, mono: false },
                TEXT,
            );
            let state = if p.on_ac {
                if pct >= 100 {
                    "Charged".to_string()
                } else {
                    "Charging".to_string()
                }
            } else {
                match p.secs_left {
                    Some(s) => format!("On battery · {} left", format::hours_minutes(s as u64 / 60)),
                    None => "On battery".to_string(),
                }
            };
            let state = if p.saver { format!("{state} · saver on") } else { state };
            b.text(Rect::new(x + 44.0, y + 44.0, w - 44.0, 16.0), state, TextStyle::SMALL, TEXT_2);
        }
        None => {
            b.icon(Icon::Plug, Rect::new(x, y + 6.0, 28.0, 28.0), OK);
            b.text(
                Rect::new(x + 40.0, y, w - 40.0, 40.0),
                "Plugged in",
                TextStyle { size: 22.0, weight: Weight::Semibold, mono: false },
                TEXT,
            );
            b.text(Rect::new(x + 40.0, y + 36.0, w - 40.0, 16.0), "No battery", TextStyle::SMALL, TEXT_2);
        }
    }
    let mut y = y + 70.0;
    let (online, wifi, vpn) = match &m.net {
        Some(n) => (Some(n.online), n.wifi.clone(), n.vpn.clone()),
        None => (m.online, None, None),
    };
    let (icon, color, text) = match online {
        Some(true) => (Icon::Wifi, OK, format!("Online{}", wifi.map(|w| format!(" · {w}")).unwrap_or_default())),
        Some(false) => (Icon::Wifi, PRIVACY_TEXT, "Offline".to_string()),
        None => (Icon::Wifi, TEXT_2, "Checking the connection…".to_string()),
    };
    b.icon(icon, Rect::new(x, y + 1.0, 14.0, 14.0), color);
    b.text(Rect::new(x + 22.0, y, w - 22.0, 16.0), text, TextStyle::SMALL, TEXT);
    y += 22.0;
    if let Some(v) = vpn {
        b.icon(Icon::Pin, Rect::new(x, y + 1.0, 14.0, 14.0), OK);
        b.text(Rect::new(x + 22.0, y, w - 22.0, 16.0), format!("VPN · {v}"), TextStyle::SMALL, TEXT);
        y += 22.0;
    }
    y
}

/// The power plans as chips; the active one lit.
pub(super) fn power_plans(m: &Model, b: &mut B, y: f32, chip_h: f32) -> f32 {
    if m.power_plans.is_empty() {
        return y;
    }
    b.text(Rect::new(b.x, y, b.w, 14.0), "POWER PLAN", TextStyle::LABEL, LABEL);
    let (mut cx, mut cy) = (b.x, y + 20.0);
    for (i, (guid, name)) in m.power_plans.iter().enumerate() {
        let active = m.active_plan.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(guid));
        let style = TextStyle { size: 12.0, weight: Weight::Medium, mono: false };
        let cw = b.tm.width(name, style).ceil() + 24.0;
        if cx + cw > b.x + b.w && cx > b.x {
            cx = b.x;
            cy += chip_h + 6.0;
        }
        let r = Rect::new(cx, cy, cw.min(b.w), chip_h);
        if active {
            b.prims.extend(soft::raised_grad(r, chip_h / 2.0, accent_light(OK), accent_deep(OK), 2.0));
        } else {
            b.prims.extend(soft::raised(r, chip_h / 2.0, SURFACE, 1.5));
        }
        b.prims.push(Prim::Text {
            rect: r,
            text: name.clone(),
            style,
            color: if active { SURFACE } else { TEXT },
            align: Align::Center,
            wrap: false,
        });
        b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::SetPowerPlan(i)), label: format!("Use the {name} power plan") });
        cx += cw + 6.0;
    }
    cy + chip_h
}

// ---- quick launch -------------------------------------------------------------

/// Pinned apps, folders and files as tiles; while editing, a tap removes one.
pub(super) fn launch(m: &Model, b: &mut B, y: f32, tile_h: f32, per_row: usize) -> f32 {
    let items = &m.config.launch.items;
    if items.is_empty() {
        b.text(Rect::new(b.x, y, b.w, 20.0), "Nothing pinned yet.", ROW_TEXT, TEXT_2);
        b.text(Rect::new(b.x, y + 20.0, b.w, 16.0), "Drop apps, folders or files on the island", ROW_META, LABEL);
        return y + 40.0;
    }
    let gap = 8.0;
    let tw = (b.w - gap * (per_row as f32 - 1.0)) / per_row as f32;
    let rows = (((b.limit - y) + gap) / (tile_h + gap)).floor().max(1.0) as usize;
    let mut bottom = y;
    for (i, item) in items.iter().enumerate().take(rows * per_row) {
        let r = Rect::new(b.x + (i % per_row) as f32 * (tw + gap), y + (i / per_row) as f32 * (tile_h + gap), tw, tile_h);
        bottom = bottom.max(r.bottom());
        let is = (tile_h - 22.0).min(32.0);
        let ir = Rect::new(r.x + (r.w - is) / 2.0, r.y + 4.0, is, is);
        match m.launch_icons.get(&item.path) {
            Some(art) => b.prims.push(Prim::Image { rect: ir, art: art.clone(), radius: 6.0 }),
            None => {
                let folder = !item.path.contains('.') || item.path.ends_with('\\') || item.path.ends_with('/');
                let icon = if folder { Icon::Folder } else { Icon::Open };
                b.icon_tile(ir, icon, PORTS);
            }
        }
        b.prims.push(Prim::Text {
            rect: Rect::new(r.x, r.bottom() - 16.0, r.w, 14.0),
            text: item.name.clone(),
            style: TextStyle { size: 10.5, weight: Weight::Regular, mono: false },
            color: TEXT,
            align: Align::Center,
            wrap: false,
        });
        if m.launch_edit {
            b.prims.push(Prim::Circle { cx: ir.right(), cy: ir.y + 2.0, r: 7.0, fill: Some(PRIVACY), stroke: None });
            b.icon(Icon::Cross, Rect::new(ir.right() - 4.0, ir.y - 2.0, 8.0, 8.0), SURFACE);
        }
        let label = if m.launch_edit { format!("Unpin {}", item.name) } else { format!("Open {}", item.name) };
        b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::Launch(i)), label });
    }
    bottom
}

// ---- clipboard history -----------------------------------------------------------

/// The search field (or a button that opens it) and the history rows.
pub(super) fn clip_history(m: &Model, b: &mut B, mut y: f32) -> f32 {
    let query = m.typing(InputFor::ClipSearch);
    match query {
        Some(q) => field(b, Rect::new(b.x, y, b.w, 30.0), q, "Search the history…", CODER),
        None => {
            b.text(Rect::new(b.x, y, b.w - 140.0, 30.0), format!("HISTORY · {}", m.clip_history.len()), TextStyle::LABEL, LABEL);
            let sr = Rect::new(b.x + b.w - 64.0, y, 64.0, 30.0);
            b.ibutton(sr, Some(Icon::Grid), "Find", Btn::Ghost, Action::ClipSearch);
            if m.clip_history.iter().any(|e| !e.pinned) {
                b.ibutton(Rect::new(sr.x - 72.0, y, 66.0, 30.0), None, "Clear", Btn::Ghost, Action::ClipClear);
            }
        }
    }
    y += 36.0;
    let list = m.clip_matches(query.unwrap_or(""));
    if list.is_empty() {
        let msg = if m.clip_history.is_empty() { "Copied text shows up here" } else { "Nothing matches" };
        b.text(Rect::new(b.x, y, b.w, 20.0), msg, ROW_TEXT, TEXT_2);
        return y + 22.0;
    }
    let row_h = 32.0;
    let fit = ((b.limit - y) / row_h).floor().max(1.0) as usize;
    for e in list.into_iter().take(fit) {
        let r = Rect::new(b.x, y, b.w, row_h - 2.0);
        let flat: String = e.text.split_whitespace().collect::<Vec<_>>().join(" ");
        b.text(Rect::new(r.x, r.y, r.w - 92.0, r.h), flat, ROW_TEXT, TEXT);
        b.text_end(Rect::new(r.right() - 90.0, r.y, 30.0, r.h), age(m, e.at), ROW_META, LABEL);
        b.hits.push(Hit {
            rect: Rect::new(r.x, r.y, r.w - 60.0, r.h),
            kind: HitKind::Click(Action::ClipPaste(e.id)),
            label: "Paste".into(),
        });
        let pin_color = if e.pinned { CODER } else { TEXT_2.with_alpha(0x90) };
        tool(
            b,
            Rect::new(r.right() - 58.0, r.y + 1.0, 28.0, 28.0),
            Icon::Pin,
            pin_color,
            Action::ClipPin(e.id),
            if e.pinned { "Unpin".into() } else { "Pin".into() },
        );
        tool(
            b,
            Rect::new(r.right() - 28.0, r.y + 1.0, 28.0, 28.0),
            Icon::Cross,
            TEXT_2,
            Action::ClipRemove(e.id),
            "Remove from history".into(),
        );
        y += row_h;
    }
    y
}

// ---- downloads --------------------------------------------------------------------

/// Downloads in progress: the name, how much so far, and a moving bar.
pub(super) fn downloads(m: &Model, b: &mut B, mut y: f32) -> f32 {
    for d in &m.downloads {
        b.icon(Icon::Download, Rect::new(b.x, y + 4.0, 16.0, 16.0), PORTS);
        b.text(Rect::new(b.x + 24.0, y, b.w - 24.0 - 80.0, 20.0), d.name.clone(), ROW_TEXT, TEXT);
        b.text_end(Rect::new(b.x + b.w - 80.0, y, 80.0, 20.0), format::bytes(d.bytes), ROW_META, TEXT_2);
        // No total is known: a short bar that slides along as time passes.
        let track = Rect::new(b.x + 24.0, y + 23.0, b.w - 24.0, 3.0);
        b.rect(track, 1.5, TRACK);
        let t = ((m.now_ms.saturating_sub(d.started_ms) / 1000) % 5) as f32 / 5.0;
        let seg = track.w * 0.25;
        b.prims.push(Prim::RoundRect {
            rect: Rect::new(track.x + (track.w - seg) * t, track.y, seg, 3.0),
            radius: 1.5,
            fill: Some(PORTS),
            stroke: None,
        });
        y += 34.0;
    }
    y
}
